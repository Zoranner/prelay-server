use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::AppError;

/// 连续失败后的冷却上限：十分钟。
const MAX_REST: Duration = Duration::from_secs(600);

/// 该失败应让供应商冷却的基础时长；返回 None 表示失败不能归因于供应商
/// （例如请求内容或模型名问题），不做冷却。
pub(crate) fn rest_base(error: &AppError) -> Option<Duration> {
    match error {
        // 连接失败、连接超时或读取超时：无法建立到上游的可靠连接。
        AppError::Upstream { status: None, .. } => Some(Duration::from_secs(60)),
        AppError::Upstream {
            status: Some(status),
            ..
        } => match status.as_u16() {
            // 凭据无效、欠费或权限问题：冷却更久，避免每个请求都白撞。
            401..=403 => Some(Duration::from_secs(300)),
            // 限流、请求超时与上游故障：短暂冷却后重试。
            408 | 429 | 500..=599 => Some(Duration::from_secs(60)),
            _ => None,
        },
        // 上游返回了无法解析的响应（例如网页或空体）：按上游故障冷却。
        AppError::UpstreamInvalidResponse { .. } => Some(Duration::from_secs(60)),
        _ => None,
    }
}

/// 进程内的失败冷却表：provider_config id -> 冷却截止时间。
///
/// 一次请求的某个候选失败后按失败类别冷却一段时间；冷却中的候选在排序时
/// 排到最后但不会被丢弃，冷却到期自动恢复。成功响应会清除该供应商的冷却。
#[derive(Clone, Debug, Default)]
pub struct FailoverRegistry {
    rests: Arc<Mutex<HashMap<String, RestEntry>>>,
}

#[derive(Clone, Copy, Debug)]
struct RestEntry {
    /// 冷却截止时间；到达后该供应商恢复。
    until: Instant,
    /// 本轮连续失败次数，用于退避。
    consecutive: u32,
}

impl FailoverRegistry {
    /// 记录一次失败并返回本次冷却时长；连续失败按倍率退避，上限 `MAX_REST`。
    pub fn rest(&self, provider_id: &str, base: Duration) -> Duration {
        let mut rests = self.lock();
        let now = Instant::now();
        let consecutive = match rests.get(provider_id) {
            Some(entry) if entry.until > now => entry.consecutive.saturating_add(1),
            _ => 1,
        };
        let shift = consecutive.saturating_sub(1).min(10);
        let duration = base.saturating_mul(1u32 << shift).min(MAX_REST);
        rests.insert(
            provider_id.to_string(),
            RestEntry {
                until: now + duration,
                consecutive,
            },
        );
        duration
    }

    /// 供应商成功响应后清除它的冷却记录。
    pub fn clear(&self, provider_id: &str) {
        self.lock().remove(provider_id);
    }

    /// 该供应商是否仍在冷却中；冷却到期自动失效。
    pub fn resting(&self, provider_id: &str) -> bool {
        let mut rests = self.lock();
        match rests.get(provider_id) {
            Some(entry) if entry.until > Instant::now() => true,
            Some(_) => {
                rests.remove(provider_id);
                false
            }
            None => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RestEntry>> {
        self.rests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::{rest_base, FailoverRegistry, MAX_REST};
    use crate::error::AppError;
    use axum::http::StatusCode;
    use std::time::Duration;

    fn upstream(status: Option<u16>) -> AppError {
        AppError::Upstream {
            status: status.map(|code| StatusCode::from_u16(code).expect("valid status")),
            message: String::new(),
        }
    }

    #[test]
    fn rest_base_covers_failure_kinds() {
        assert_eq!(rest_base(&upstream(None)), Some(Duration::from_secs(60)));
        assert_eq!(
            rest_base(&upstream(Some(502))),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            rest_base(&upstream(Some(429))),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            rest_base(&upstream(Some(401))),
            Some(Duration::from_secs(300))
        );
        assert_eq!(
            rest_base(&upstream(Some(402))),
            Some(Duration::from_secs(300))
        );
        assert_eq!(
            rest_base(&upstream(Some(403))),
            Some(Duration::from_secs(300))
        );
        assert_eq!(rest_base(&upstream(Some(400))), None);
        assert_eq!(rest_base(&upstream(Some(404))), None);
        assert_eq!(
            rest_base(&AppError::UpstreamInvalidResponse {
                message: String::new()
            }),
            Some(Duration::from_secs(60))
        );
        assert_eq!(rest_base(&AppError::BadRequest(String::new())), None);
    }

    #[test]
    fn rest_expires() {
        let registry = FailoverRegistry::default();
        assert!(!registry.resting("p1"));
        registry.rest("p1", Duration::from_millis(40));
        assert!(registry.resting("p1"));
        std::thread::sleep(Duration::from_millis(60));
        assert!(!registry.resting("p1"));
    }

    #[test]
    fn consecutive_failures_back_off_and_cap() {
        let registry = FailoverRegistry::default();
        let first = registry.rest("p1", Duration::from_millis(50));
        let second = registry.rest("p1", Duration::from_millis(50));
        let third = registry.rest("p1", Duration::from_millis(50));
        assert_eq!(first, Duration::from_millis(50));
        assert_eq!(second, Duration::from_millis(100));
        assert_eq!(third, Duration::from_millis(200));

        let capped = registry.rest("p2", Duration::from_secs(600));
        assert_eq!(capped, MAX_REST);
        let capped_again = registry.rest("p2", Duration::from_secs(600));
        assert_eq!(capped_again, MAX_REST);
    }

    #[test]
    fn clear_forgets_rest() {
        let registry = FailoverRegistry::default();
        registry.rest("p1", Duration::from_millis(500));
        assert!(registry.resting("p1"));
        registry.clear("p1");
        assert!(!registry.resting("p1"));
    }
}
