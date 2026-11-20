mod commandcode;
mod deepseek;
mod fetch;
mod glm;
mod kimi;
mod minimax;
mod opencode;
mod reservation;

pub(crate) use fetch::{fetch_quota_body, fetch_quota_url_body};
pub(crate) use reservation::{estimate_tokens, QuotaReservations, ReservationGuard};

use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, Mutex},
    time::{Duration, Instant},
};

use base64::Engine;
use chrono::Utc;
use prelay_protocol::{
    ProviderQuota, ProviderQuotaMode, ProviderQuotaResponse, QuotaError, QuotaErrorCode,
    ShareQuotaWindow,
};
use sha2::{Digest, Sha256};

use crate::{
    domain::catalog::{CatalogQuota, ProviderCatalog, QuotaAdapterKind},
    error::AppError,
    store::{
        entity::identity::provider_configs as identity_provider_configs,
        storage::{Storage, StorageError},
    },
};

const CACHE_TTL: Duration = Duration::from_secs(60);
const ERROR_CACHE_TTL: Duration = Duration::from_secs(15);
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(4);
const LAST_GOOD_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
pub struct QuotaService {
    client: reqwest::Client,
    catalog: Arc<ProviderCatalog>,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
    last_good: Arc<Mutex<HashMap<String, LastGood>>>,
    reservations: QuotaReservations,
    ttl: Duration,
    error_ttl: Duration,
}

#[derive(Clone)]
struct CacheEntry {
    inserted_at: Instant,
    quota: ProviderQuota,
    is_error: bool,
}

#[derive(Clone)]
struct LastGood {
    at: Instant,
    quota: ProviderQuota,
}

impl QuotaService {
    pub fn new(client: reqwest::Client, catalog: Arc<ProviderCatalog>) -> Self {
        Self {
            client,
            catalog,
            cache: Arc::new(Mutex::new(HashMap::new())),
            last_good: Arc::new(Mutex::new(HashMap::new())),
            reservations: QuotaReservations::default(),
            ttl: CACHE_TTL,
            error_ttl: ERROR_CACHE_TTL,
        }
    }

    pub(crate) fn track_recording_sequences(
        &mut self,
        durable_sequence: Arc<AtomicU64>,
        applied_sequence: Arc<AtomicU64>,
    ) {
        self.reservations
            .track_sequences(durable_sequence, applied_sequence);
    }

    pub async fn list_for_identity(
        &self,
        storage: &Storage,
        identity_id: &str,
    ) -> Result<Vec<ProviderQuotaResponse>, StorageError> {
        let providers = storage.list_visible_provider_records(identity_id).await?;
        let identity_id = identity_id.to_string();
        let now = Utc::now();
        let results: Vec<Result<ProviderQuotaResponse, StorageError>> =
            futures::future::join_all(providers.into_iter().map(|visible| {
                let identity_id = identity_id.clone();
                async move {
                    let (mode, quota) = if visible.is_owner {
                        (
                            ProviderQuotaMode::Own,
                            self.external_quota(
                                storage,
                                &visible.owner_identity_id,
                                &visible.provider,
                            )
                            .await,
                        )
                    } else {
                        match visible.grant.as_ref() {
                            Some(grant) => match grant.quota_json.as_deref() {
                                Some(json) => {
                                    let rules = serde_json::from_str::<Vec<ShareQuotaWindow>>(json)
                                        .map_err(|error| {
                                            StorageError::InvalidProviderSharing(format!(
                                                "stored share quota is invalid: {error}"
                                            ))
                                        })?;
                                    let mut windows = storage
                                        .shared_quota_windows(
                                            &visible.provider.id,
                                            &identity_id,
                                            &grant.created_at,
                                            &rules,
                                            now,
                                        )
                                        .await?;
                                    self.reservations.apply_pending(
                                        &visible.provider.id,
                                        &identity_id,
                                        &mut windows,
                                    );
                                    (
                                        ProviderQuotaMode::SharedLimited,
                                        ProviderQuota {
                                            plan: None,
                                            windows,
                                            balance: None,
                                            error: None,
                                            read_at: Some(now.to_rfc3339()),
                                            stale: false,
                                        },
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
                            None => (
                                ProviderQuotaMode::SharedUnlimited,
                                self.external_quota(
                                    storage,
                                    &visible.owner_identity_id,
                                    &visible.provider,
                                )
                                .await,
                            ),
                        }
                    };
                    Ok(ProviderQuotaResponse {
                        provider_id: visible.provider.id.clone(),
                        owner_identity_id: visible.owner_identity_id.clone(),
                        owner_display_name: visible.owner_display_name.clone(),
                        mode,
                        quota,
                        updated_at: Some(Utc::now().to_rfc3339()),
                    })
                }
            }))
            .await;
        let mut responses = Vec::with_capacity(results.len());
        for result in results {
            responses.push(result?);
        }
        Ok(responses)
    }

    pub(crate) async fn reserve(
        &self,
        storage: &Storage,
        provider_id: &str,
        identity_id: &str,
        tokens: i64,
    ) -> Result<ReservationGuard, AppError> {
        self.reservations
            .reserve(storage, provider_id, identity_id, tokens)
            .await
    }

    async fn external_quota(
        &self,
        storage: &Storage,
        owner_identity_id: &str,
        provider: &identity_provider_configs::Model,
    ) -> ProviderQuota {
        let Some(catalog_provider) = self.catalog.provider(&provider.provider_type) else {
            return quota_with_error(QuotaErrorCode::Unsupported, "该供应商没有接入额度接口");
        };
        let Some(quota) = catalog_provider.quota.as_ref() else {
            return quota_with_error(QuotaErrorCode::Unsupported, "该供应商没有接入额度接口");
        };
        let api_key = match storage
            .decrypt_provider_key(owner_identity_id, &provider.id)
            .await
        {
            Ok(api_key) => api_key,
            Err(_) => {
                return quota_with_error(QuotaErrorCode::Unconfigured, "无法读取供应商额度凭据")
            }
        };
        self.snapshot(provider, quota, &api_key).await
    }

    async fn snapshot(
        &self,
        provider: &identity_provider_configs::Model,
        quota: &CatalogQuota,
        api_key: &str,
    ) -> ProviderQuota {
        let key = cache_key(&provider.id, &provider.base_url, &quota.url, api_key);
        if let Some(cached) = self.cached(&key) {
            return cached;
        }
        let result = tokio::time::timeout(PROVIDER_TIMEOUT, async {
            match quota.adapter {
                QuotaAdapterKind::DeepSeekBalance => {
                    deepseek::fetch(&self.client, &provider.base_url, quota, api_key).await
                }
                QuotaAdapterKind::MiniMaxPlan => {
                    minimax::fetch(&self.client, &provider.base_url, quota, api_key).await
                }
                QuotaAdapterKind::GlmCodingPlan => {
                    glm::fetch(&self.client, &provider.base_url, quota, api_key).await
                }
                QuotaAdapterKind::KimiCodeUsages => {
                    kimi::fetch(&self.client, &provider.base_url, quota, api_key).await
                }
                QuotaAdapterKind::CommandCodeCredits => {
                    commandcode::fetch(&self.client, &provider.base_url, quota, api_key).await
                }
                QuotaAdapterKind::OpenCodeGoUsage => {
                    opencode::fetch(&self.client, &provider.base_url, quota, api_key).await
                }
            }
        })
        .await;
        let result = match result {
            Ok(result) => result,
            Err(_) => Err(quota_error(QuotaErrorCode::Unreachable, "额度查询超时")),
        };
        let mut quota = match result {
            Ok(quota) => quota,
            Err(error) => ProviderQuota {
                plan: None,
                windows: Vec::new(),
                balance: None,
                error: Some(error),
                read_at: None,
                stale: false,
            },
        };
        quota.read_at = Some(Utc::now().to_rfc3339());
        if let Some(error) = &quota.error {
            if matches!(
                error.code,
                QuotaErrorCode::Unreachable | QuotaErrorCode::InvalidResponse
            ) {
                if let Some(mut last) = self.last_good(&key) {
                    last.stale = true;
                    // 旧读数继续展示，但按错误 TTL 退避，避免上游故障时反复同步请求。
                    self.cache
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(
                            key,
                            CacheEntry {
                                inserted_at: Instant::now(),
                                quota: last.clone(),
                                is_error: true,
                            },
                        );
                    return last;
                }
            }
        } else {
            self.last_good
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    key.clone(),
                    LastGood {
                        at: Instant::now(),
                        quota: quota.clone(),
                    },
                );
        }
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                key,
                CacheEntry {
                    inserted_at: Instant::now(),
                    quota: quota.clone(),
                    is_error: quota.error.is_some(),
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
        let ttl = if entry.is_error {
            self.error_ttl
        } else {
            self.ttl
        };
        if entry.inserted_at.elapsed() > ttl {
            cache.remove(key);
            return None;
        }
        Some(entry.quota.clone())
    }

    fn last_good(&self, key: &str) -> Option<ProviderQuota> {
        let mut last_good = self
            .last_good
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = last_good.get(key)?;
        if entry.at.elapsed() > LAST_GOOD_TTL {
            last_good.remove(key);
            return None;
        }
        Some(entry.quota.clone())
    }
}

fn cache_key(provider_id: &str, base_url: &str, url: &str, api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    format!(
        "{provider_id}:{base_url}:{url}:{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    )
}

fn quota_with_error(code: QuotaErrorCode, message: impl Into<String>) -> ProviderQuota {
    ProviderQuota {
        plan: None,
        windows: Vec::new(),
        balance: None,
        error: Some(QuotaError {
            code,
            message: message.into(),
        }),
        read_at: None,
        stale: false,
    }
}

pub(crate) fn quota_error(code: QuotaErrorCode, message: impl Into<String>) -> QuotaError {
    QuotaError {
        code,
        message: message.into(),
    }
}

pub(crate) fn http_quota_error(vendor: &str, status: reqwest::StatusCode) -> QuotaError {
    let code = match status.as_u16() {
        401 | 403 => QuotaErrorCode::Unauthorized,
        500..=599 => QuotaErrorCode::Unreachable,
        _ => QuotaErrorCode::InvalidResponse,
    };
    quota_error(
        code,
        format!("{vendor} 额度接口返回 HTTP {}", status.as_u16()),
    )
}

pub(crate) fn resolve_quota_url(base_url: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|base| base.join(url).ok())
        .map(|resolved| resolved.to_string())
        .unwrap_or_else(|| url.to_string())
}

pub(crate) fn parse_epoch_millis(value: i64) -> Option<String> {
    if value <= 0 {
        return None;
    }
    chrono::DateTime::from_timestamp_millis(value).map(|time| time.to_rfc3339())
}

#[cfg(test)]
mod tests;
