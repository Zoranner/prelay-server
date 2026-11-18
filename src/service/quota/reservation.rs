use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::http::StatusCode;
use chrono::Utc;
use prelay_protocol::{QuotaWindow, ShareQuotaWindow};

use crate::{error::AppError, store::storage::Storage};

#[derive(Clone, Default)]
pub struct QuotaReservations {
    held: Arc<Mutex<HashMap<HeldKey, i64>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct HeldKey {
    provider_id: String,
    identity_id: String,
    window: String,
    reset: String,
}

pub struct ReservationGuard {
    reservations: QuotaReservations,
    keys: Vec<HeldKey>,
    tokens: i64,
    active: bool,
}

impl ReservationGuard {
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            reservations: QuotaReservations::default(),
            keys: Vec::new(),
            tokens: 0,
            active: false,
        }
    }
}

impl QuotaReservations {
    /// 请求进入前为每个启用的共享窗口预留额度；没有共享限额时返回空守卫。
    pub async fn reserve(
        &self,
        storage: &Storage,
        provider_id: &str,
        identity_id: &str,
        tokens: i64,
    ) -> Result<ReservationGuard, AppError> {
        let empty = || ReservationGuard {
            reservations: self.clone(),
            keys: Vec::new(),
            tokens: 0,
            active: false,
        };
        if tokens <= 0 {
            return Ok(empty());
        }
        let Some(share) = storage.provider_share(provider_id, identity_id).await? else {
            return Ok(empty());
        };
        let Some(json) = share.quota_json.as_deref() else {
            return Ok(empty());
        };
        let rules = serde_json::from_str::<Vec<ShareQuotaWindow>>(json).map_err(|error| {
            AppError::Internal(anyhow::anyhow!("stored share quota is invalid: {error}"))
        })?;
        let windows = storage
            .shared_quota_windows(
                provider_id,
                identity_id,
                &share.created_at,
                &rules,
                Utc::now(),
            )
            .await?;
        if windows.is_empty() {
            return Ok(empty());
        }
        self.reserve_windows(provider_id, identity_id, &windows, tokens)
    }

    pub(crate) fn reserve_windows(
        &self,
        provider_id: &str,
        identity_id: &str,
        windows: &[QuotaWindow],
        tokens: i64,
    ) -> Result<ReservationGuard, AppError> {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut keys = Vec::with_capacity(windows.len());
        for window in windows {
            let remaining = window
                .amount
                .as_deref()
                .and_then(|amount| amount.parse::<i64>().ok())
                .unwrap_or(0);
            let key = HeldKey {
                provider_id: provider_id.to_string(),
                identity_id: identity_id.to_string(),
                window: window.label.clone(),
                reset: window.resets_at.clone().unwrap_or_default(),
            };
            let already = held.get(&key).copied().unwrap_or(0);
            if remaining.saturating_sub(already) < tokens {
                return Err(AppError::Upstream {
                    status: Some(StatusCode::TOO_MANY_REQUESTS),
                    message: format!(
                        "共享额度不足：{} 剩余 {} tokens，重置 {}",
                        window.label,
                        remaining,
                        window.resets_at.as_deref().unwrap_or("未知")
                    ),
                });
            }
            keys.push(key);
        }
        for key in &keys {
            *held.entry(key.clone()).or_default() += tokens;
        }
        Ok(ReservationGuard {
            reservations: self.clone(),
            keys,
            tokens,
            active: true,
        })
    }

    #[cfg(test)]
    pub(crate) fn held_total(&self, provider_id: &str, identity_id: &str) -> i64 {
        self.held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|(key, _)| key.provider_id == provider_id && key.identity_id == identity_id)
            .map(|(_, tokens)| *tokens)
            .sum()
    }
}

impl Drop for ReservationGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut held = self
            .reservations
            .held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for key in &self.keys {
            if let Some(value) = held.get_mut(key) {
                *value -= self.tokens;
                if *value <= 0 {
                    held.remove(key);
                }
            }
        }
    }
}

/// 请求体字节数的粗略 token 估计；用于并发预留，不用于计费。
pub(crate) fn estimate_tokens(payload: &serde_json::Value) -> i64 {
    let bytes = serde_json::to_vec(payload)
        .map(|body| body.len())
        .unwrap_or_default();
    ((bytes / 4) as i64 + 256).clamp(256, 200_000)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread,
    };

    use prelay_protocol::QuotaWindow;

    use super::{estimate_tokens, QuotaReservations};

    #[test]
    fn estimates_at_least_one_small_request() {
        assert!(estimate_tokens(&serde_json::json!({"model":"k3"})) >= 256);
    }

    #[test]
    fn empty_reservations_can_be_cloned() {
        let reservations = QuotaReservations::default();
        let clone = reservations.clone();
        drop(clone);
    }

    #[test]
    fn reservations_do_not_overshoot_a_window() {
        let reservations = QuotaReservations::default();
        let windows = vec![QuotaWindow {
            label: "1 天".to_string(),
            span_secs: Some(24 * 60 * 60),
            used_percent: 0.0,
            resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
            display: None,
            amount: Some("1000".to_string()),
            limit: Some("1000".to_string()),
            unit: Some("tokens".to_string()),
            aside: false,
            pool: None,
        }];
        let guards = Arc::new(Mutex::new(Vec::new()));
        let successes = Arc::new(AtomicUsize::new(0));
        thread::scope(|scope| {
            for _ in 0..20 {
                let reservations = reservations.clone();
                let windows = windows.clone();
                let guards = Arc::clone(&guards);
                let successes = Arc::clone(&successes);
                scope.spawn(move || {
                    if let Ok(guard) = reservations.reserve_windows("p", "i", &windows, 100) {
                        successes.fetch_add(1, Ordering::SeqCst);
                        guards.lock().expect("guards").push(guard);
                    }
                });
            }
        });
        assert_eq!(successes.load(Ordering::SeqCst), 10);
    }
}
