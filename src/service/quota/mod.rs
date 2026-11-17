mod deepseek;
mod minimax;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use base64::Engine;
use chrono::Utc;
use prelay_protocol::{ProviderQuota, ProviderQuotaMode, ProviderQuotaResponse, ShareQuotaWindow};
use sha2::{Digest, Sha256};

use crate::store::{
    entity::identity::provider_configs as identity_provider_configs,
    storage::{Storage, StorageError},
};

const CACHE_TTL: Duration = Duration::from_secs(60);
const ERROR_CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct QuotaService {
    client: reqwest::Client,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
    ttl: Duration,
    error_ttl: Duration,
}

#[derive(Clone)]
struct CacheEntry {
    inserted_at: Instant,
    quota: ProviderQuota,
}

impl QuotaService {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            cache: Arc::new(Mutex::new(HashMap::new())),
            ttl: CACHE_TTL,
            error_ttl: ERROR_CACHE_TTL,
        }
    }

    pub async fn list_for_identity(
        &self,
        storage: &Storage,
        identity_id: &str,
    ) -> Result<Vec<ProviderQuotaResponse>, StorageError> {
        let providers = storage.list_visible_provider_records(identity_id).await?;
        let now = Utc::now();
        let mut responses = Vec::with_capacity(providers.len());
        for visible in providers {
            let (mode, quota) = if visible.can_manage {
                (
                    ProviderQuotaMode::Own,
                    self.external_quota(storage, &visible.owner_identity_id, &visible.provider)
                        .await,
                )
            } else {
                match storage
                    .provider_share(&visible.provider.id, identity_id)
                    .await?
                {
                    Some(share) => match share.quota_json.as_deref() {
                        Some(json) => {
                            let rules = serde_json::from_str::<Vec<ShareQuotaWindow>>(json)
                                .map_err(|error| {
                                    StorageError::InvalidProviderSharing(format!(
                                        "stored share quota is invalid: {error}"
                                    ))
                                })?;
                            let windows = storage
                                .shared_quota_windows(
                                    &visible.provider.id,
                                    identity_id,
                                    &share.created_at,
                                    &rules,
                                    now,
                                )
                                .await?;
                            (
                                ProviderQuotaMode::SharedLimited,
                                ProviderQuota::Shared { windows },
                            )
                        }
                        None => (
                            ProviderQuotaMode::SharedUnlimited,
                            self.external_quota(
                                storage,
                                &visible.owner_identity_id,
                                &visible.provider,
                            )
                            .await,
                        ),
                    },
                    None => {
                        // 旧版 all 可见范围没有 share 行，按直接共享处理。
                        (
                            ProviderQuotaMode::SharedUnlimited,
                            self.external_quota(
                                storage,
                                &visible.owner_identity_id,
                                &visible.provider,
                            )
                            .await,
                        )
                    }
                }
            };
            responses.push(ProviderQuotaResponse {
                provider_id: visible.provider.id.clone(),
                owner_identity_id: visible.owner_identity_id.clone(),
                owner_display_name: visible.owner_display_name.clone(),
                mode,
                quota,
                updated_at: Some(Utc::now().to_rfc3339()),
            });
        }
        Ok(responses)
    }

    async fn external_quota(
        &self,
        storage: &Storage,
        owner_identity_id: &str,
        provider: &identity_provider_configs::Model,
    ) -> ProviderQuota {
        if !supports_provider_type(&provider.provider_type) {
            return ProviderQuota::Unsupported;
        }
        let api_key = match storage
            .decrypt_provider_key(owner_identity_id, &provider.id)
            .await
        {
            Ok(api_key) => api_key,
            Err(_) => {
                return ProviderQuota::Error {
                    message: "无法读取供应商额度凭据".to_string(),
                }
            }
        };
        self.snapshot(provider, &api_key).await
    }

    async fn snapshot(
        &self,
        provider: &identity_provider_configs::Model,
        api_key: &str,
    ) -> ProviderQuota {
        let key = cache_key(
            &provider.id,
            &provider.provider_type,
            &provider.base_url,
            api_key,
        );
        if let Some(cached) = self.cached(&key) {
            return cached;
        }
        let result = match provider.provider_type.as_str() {
            "deepseek" => deepseek::fetch(&self.client, &provider.base_url, api_key).await,
            "minimax" => minimax::fetch(&self.client, &provider.base_url, api_key).await,
            _ => return ProviderQuota::Unsupported,
        };
        let quota = match result {
            Ok(quota) => quota,
            Err(message) => ProviderQuota::Error { message },
        };
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                key,
                CacheEntry {
                    inserted_at: Instant::now(),
                    quota: quota.clone(),
                },
            );
        quota
    }

    fn cached(&self, key: &str) -> Option<ProviderQuota> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.get(key)?;
        let ttl = match &entry.quota {
            ProviderQuota::Error { .. } => self.error_ttl,
            _ => self.ttl,
        };
        if entry.inserted_at.elapsed() > ttl {
            cache.remove(key);
            return None;
        }
        Some(entry.quota.clone())
    }
}

fn cache_key(provider_id: &str, provider_type: &str, base_url: &str, api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    format!(
        "{provider_id}:{provider_type}:{base_url}:{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    )
}

fn supports_provider_type(provider_type: &str) -> bool {
    matches!(provider_type, "deepseek" | "minimax")
}

pub(crate) fn parse_epoch_millis(value: i64) -> Option<String> {
    if value <= 0 {
        return None;
    }
    chrono::DateTime::from_timestamp_millis(value).map(|time| time.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::cache_key;

    #[test]
    fn cache_key_changes_with_the_api_key() {
        assert_ne!(
            cache_key("provider", "deepseek", "https://example.test", "key-a"),
            cache_key("provider", "deepseek", "https://example.test", "key-b")
        );
        assert_eq!(
            cache_key("provider", "deepseek", "https://example.test", "key-a"),
            cache_key("provider", "deepseek", "https://example.test", "key-a")
        );
    }
}
