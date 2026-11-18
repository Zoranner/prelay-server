use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use chrono::Utc;
use prelay_protocol::{QuotaWindow, ShareQuotaWindow};

use crate::{error::AppError, store::storage::Storage};

const DEFAULT_PENDING_SETTLEMENT_TTL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct QuotaReservations {
    // 成功响应结束后，预留会保留为待物化额度。15 秒覆盖默认 5 秒物化 SLO，
    // 避免活动尚未写入数据库时连续请求重复使用同一份额度。
    held: Arc<Mutex<HashMap<HeldKey, i64>>>,
    pending: Arc<Mutex<HashMap<HeldKey, PendingUsage>>>,
    pending_ttl: Duration,
    durable_sequence: Option<Arc<AtomicU64>>,
    applied_sequence: Option<Arc<AtomicU64>>,
}

impl Default for QuotaReservations {
    fn default() -> Self {
        Self {
            held: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            pending_ttl: DEFAULT_PENDING_SETTLEMENT_TTL,
            durable_sequence: None,
            applied_sequence: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct HeldKey {
    provider_id: String,
    identity_id: String,
    window: String,
    reset: String,
}

#[derive(Clone, Copy)]
struct PendingUsage {
    tokens: i64,
    sequence: u64,
    expires_at: Option<Instant>,
}

pub struct ReservationGuard {
    reservations: QuotaReservations,
    keys: Vec<HeldKey>,
    tokens: i64,
    active: bool,
    settle_on_drop: bool,
}

impl ReservationGuard {
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            reservations: QuotaReservations::default(),
            keys: Vec::new(),
            tokens: 0,
            active: false,
            settle_on_drop: false,
        }
    }

    pub(crate) fn set_settle_on_drop(&mut self, value: bool) {
        self.settle_on_drop = value;
    }
}

impl QuotaReservations {
    pub(crate) fn track_sequences(
        &mut self,
        durable_sequence: Arc<AtomicU64>,
        applied_sequence: Arc<AtomicU64>,
    ) {
        self.durable_sequence = Some(durable_sequence);
        self.applied_sequence = Some(applied_sequence);
    }

    #[cfg(test)]
    fn with_pending_ttl(pending_ttl: Duration) -> Self {
        Self {
            pending_ttl,
            ..Self::default()
        }
    }

    fn prune_pending(&self, pending: &mut HashMap<HeldKey, PendingUsage>) {
        let now = Instant::now();
        let applied = self
            .applied_sequence
            .as_ref()
            .map(|sequence| sequence.load(Ordering::Acquire))
            .unwrap_or(0);
        pending.retain(|_, usage| {
            let expired = usage.expires_at.is_some_and(|expires_at| expires_at <= now);
            let materialized = usage.sequence > 0 && applied >= usage.sequence;
            !expired && !materialized
        });
    }

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
            settle_on_drop: false,
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
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.prune_pending(&mut pending);
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
            let pending_tokens = pending.get(&key).map(|usage| usage.tokens).unwrap_or(0);
            let available = remaining
                .saturating_sub(already)
                .saturating_sub(pending_tokens);
            if available < tokens {
                return Err(AppError::Upstream {
                    status: Some(StatusCode::TOO_MANY_REQUESTS),
                    message: format!(
                        "共享额度不足：{} 可用 {} tokens，重置 {}",
                        window.label,
                        available,
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
            settle_on_drop: false,
        })
    }

    pub(crate) fn apply_pending(
        &self,
        provider_id: &str,
        identity_id: &str,
        windows: &mut [QuotaWindow],
    ) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.prune_pending(&mut pending);
        for window in windows {
            let key = HeldKey {
                provider_id: provider_id.to_string(),
                identity_id: identity_id.to_string(),
                window: window.label.clone(),
                reset: window.resets_at.clone().unwrap_or_default(),
            };
            let Some(usage) = pending.get(&key) else {
                continue;
            };
            if usage.tokens <= 0 {
                continue;
            }
            let Some(limit) = window
                .limit
                .as_deref()
                .and_then(|value| value.parse::<i64>().ok())
            else {
                continue;
            };
            if limit <= 0 {
                continue;
            }
            let remaining = window
                .amount
                .as_deref()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0);
            let adjusted = remaining.saturating_sub(usage.tokens).max(0);
            window.amount = Some(adjusted.to_string());
            window.used_percent =
                (((limit - adjusted) as f64 / limit as f64) * 100.0).clamp(0.0, 100.0);
            window.display = Some(format!("{} / {}", limit - adjusted, limit));
        }
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
        drop(held);
        if self.settle_on_drop && self.tokens > 0 {
            let sequence = self
                .reservations
                .durable_sequence
                .as_ref()
                .map(|value| value.load(Ordering::Acquire))
                .unwrap_or(0);
            let expires_at = if sequence > 0 || self.reservations.pending_ttl.is_zero() {
                None
            } else {
                Some(Instant::now() + self.reservations.pending_ttl)
            };
            if sequence == 0 && expires_at.is_none() {
                return;
            }
            let mut pending = self
                .reservations
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for key in &self.keys {
                let entry = pending.entry(key.clone()).or_insert(PendingUsage {
                    tokens: 0,
                    sequence: 0,
                    expires_at: None,
                });
                entry.tokens = entry.tokens.saturating_add(self.tokens);
                entry.sequence = entry.sequence.max(sequence);
                if sequence > 0 {
                    entry.expires_at = None;
                } else if let Some(expires_at) = expires_at {
                    entry.expires_at = Some(
                        entry
                            .expires_at
                            .map_or(expires_at, |current| current.max(expires_at)),
                    );
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
    let body_estimate = ((bytes / 4) as i64 + 256).max(256);
    let output_budget = ["max_output_tokens", "max_completion_tokens", "max_tokens"]
        .iter()
        .filter_map(|key| payload.get(*key).and_then(serde_json::Value::as_i64))
        .max()
        .unwrap_or(0)
        .max(0);
    body_estimate.max(output_budget).clamp(256, 2_000_000)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
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
    fn estimates_at_least_the_declared_output_budget() {
        assert!(estimate_tokens(&serde_json::json!({"max_output_tokens": 5_000})) >= 5_000);
        assert!(estimate_tokens(&serde_json::json!({"max_completion_tokens": 7_000})) >= 7_000);
        assert!(estimate_tokens(&serde_json::json!({"max_tokens": 9_000})) >= 9_000);
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

    #[test]
    fn settled_reservations_hold_capacity_until_pending_expires() {
        let reservations =
            QuotaReservations::with_pending_ttl(std::time::Duration::from_millis(30));
        let windows = vec![QuotaWindow {
            label: "1 天".to_string(),
            span_secs: Some(24 * 60 * 60),
            used_percent: 0.0,
            resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
            display: None,
            amount: Some("100".to_string()),
            limit: Some("100".to_string()),
            unit: Some("tokens".to_string()),
            aside: false,
            pool: None,
        }];

        let mut guard = reservations
            .reserve_windows("p", "i", &windows, 60)
            .expect("first reservation");
        guard.set_settle_on_drop(true);
        drop(guard);

        assert!(
            reservations
                .reserve_windows("p", "i", &windows, 50)
                .is_err(),
            "pending usage must reduce the available capacity"
        );

        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            reservations.reserve_windows("p", "i", &windows, 50).is_ok(),
            "expired pending usage must release capacity"
        );
    }

    #[test]
    fn pending_usage_is_visible_in_the_quota_window() {
        let reservations = QuotaReservations::default();
        let mut windows = vec![QuotaWindow {
            label: "1 天".to_string(),
            span_secs: Some(24 * 60 * 60),
            used_percent: 0.0,
            resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
            display: Some("0 / 100".to_string()),
            amount: Some("100".to_string()),
            limit: Some("100".to_string()),
            unit: Some("tokens".to_string()),
            aside: false,
            pool: None,
        }];
        let mut guard = reservations
            .reserve_windows("p", "i", &windows, 60)
            .expect("reservation");
        guard.set_settle_on_drop(true);
        drop(guard);

        reservations.apply_pending("p", "i", &mut windows);
        assert_eq!(windows[0].amount.as_deref(), Some("40"));
        assert_eq!(windows[0].display.as_deref(), Some("60 / 100"));
        assert_eq!(windows[0].used_percent, 60.0);
    }

    #[test]
    fn pending_usage_clears_when_materialization_reaches_its_sequence() {
        let durable = Arc::new(AtomicU64::new(0));
        let applied = Arc::new(AtomicU64::new(0));
        let mut reservations = QuotaReservations::default();
        reservations.track_sequences(Arc::clone(&durable), Arc::clone(&applied));
        let windows = vec![QuotaWindow {
            label: "1 天".to_string(),
            span_secs: Some(24 * 60 * 60),
            used_percent: 0.0,
            resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
            display: None,
            amount: Some("100".to_string()),
            limit: Some("100".to_string()),
            unit: Some("tokens".to_string()),
            aside: false,
            pool: None,
        }];

        let mut guard = reservations
            .reserve_windows("p", "i", &windows, 60)
            .expect("reservation");
        guard.set_settle_on_drop(true);
        durable.store(7, Ordering::Release);
        drop(guard);

        assert!(
            reservations
                .reserve_windows("p", "i", &windows, 50)
                .is_err(),
            "unmaterialized usage must still hold capacity"
        );
        applied.store(6, Ordering::Release);
        assert!(
            reservations
                .reserve_windows("p", "i", &windows, 50)
                .is_err(),
            "usage must remain held before its sequence is materialized"
        );
        applied.store(7, Ordering::Release);
        assert!(
            reservations.reserve_windows("p", "i", &windows, 50).is_ok(),
            "materialized usage must release the pending capacity"
        );
    }
}
