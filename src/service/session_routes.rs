use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// 会话粘性的保留时长。上游提示缓存通常只有几分钟，三十分钟足够覆盖一次连续对话。
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

/// 进程内最多保留的会话路由数，避免长时间运行时无界增长。
const MAX_SESSIONS: usize = 4096;

/// 进程内的会话路由表：同一会话优先复用上次成功的供应商，新会话不继承。
///
/// 粘性只在会话内生效，并且只是选路偏好；失败冷却仍由 `FailoverRegistry` 决定。
#[derive(Clone, Debug)]
pub struct SessionRouteRegistry {
    ttl: Duration,
    capacity: usize,
    routes: Arc<Mutex<HashMap<RouteKey, RouteEntry>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RouteKey {
    endpoint_id: String,
    model_name: String,
    session_key: String,
}

#[derive(Clone, Debug)]
struct RouteEntry {
    provider_id: String,
    expires_at: Instant,
}

impl SessionRouteRegistry {
    fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity: capacity.max(1),
            routes: Default::default(),
        }
    }

    /// 同一会话内上次成功的供应商；过期或不存在时返回 `None`。
    pub fn provider_for(
        &self,
        endpoint_id: &str,
        model_name: &str,
        session_key: &str,
    ) -> Option<String> {
        let key = RouteKey {
            endpoint_id: endpoint_id.to_string(),
            model_name: model_name.to_string(),
            session_key: session_key.to_string(),
        };
        let mut routes = self.lock();
        let entry = routes.get(&key)?;
        if entry.expires_at > Instant::now() {
            return Some(entry.provider_id.clone());
        }
        routes.remove(&key);
        None
    }

    /// 记录本次成功响应的供应商；空会话标识不记录。
    pub fn remember(
        &self,
        endpoint_id: &str,
        model_name: &str,
        session_key: &str,
        provider_id: &str,
    ) {
        if session_key.is_empty() {
            return;
        }
        let key = RouteKey {
            endpoint_id: endpoint_id.to_string(),
            model_name: model_name.to_string(),
            session_key: session_key.to_string(),
        };
        let now = Instant::now();
        let mut routes = self.lock();
        if routes.len() >= self.capacity {
            routes.retain(|_, entry| entry.expires_at > now);
        }
        while routes.len() >= self.capacity {
            let Some(oldest) = routes
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            routes.remove(&oldest);
        }
        routes.insert(
            key,
            RouteEntry {
                provider_id: provider_id.to_string(),
                expires_at: now + self.ttl,
            },
        );
    }

    /// 接入点配置变化后，旧会话不再沿用之前的供应商。
    pub fn clear_endpoint(&self, endpoint_id: &str) {
        self.lock().retain(|key, _| key.endpoint_id != endpoint_id);
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<RouteKey, RouteEntry>> {
        self.routes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for SessionRouteRegistry {
    fn default() -> Self {
        Self::new(SESSION_TTL, MAX_SESSIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::SessionRouteRegistry;
    use std::time::Duration;

    #[test]
    fn keeps_affinity_within_a_session_only() {
        let routes = SessionRouteRegistry::default();
        routes.remember("endpoint-1", "model-a", "session-a", "provider-1");

        assert_eq!(
            routes.provider_for("endpoint-1", "model-a", "session-a"),
            Some("provider-1".to_string())
        );
        assert_eq!(
            routes.provider_for("endpoint-1", "model-a", "session-b"),
            None
        );
        assert_eq!(
            routes.provider_for("endpoint-2", "model-a", "session-a"),
            None
        );
        assert_eq!(
            routes.provider_for("endpoint-1", "model-b", "session-a"),
            None
        );
    }

    #[test]
    fn expires_affinity_after_its_ttl() {
        let routes = SessionRouteRegistry::new(Duration::from_millis(1), 8);
        routes.remember("endpoint-1", "model-a", "session-a", "provider-1");
        std::thread::sleep(Duration::from_millis(5));

        assert_eq!(
            routes.provider_for("endpoint-1", "model-a", "session-a"),
            None
        );
    }

    #[test]
    fn clearing_an_endpoint_drops_only_its_sessions() {
        let routes = SessionRouteRegistry::default();
        routes.remember("endpoint-1", "model-a", "session-a", "provider-1");
        routes.remember("endpoint-2", "model-a", "session-a", "provider-2");

        routes.clear_endpoint("endpoint-1");

        assert_eq!(
            routes.provider_for("endpoint-1", "model-a", "session-a"),
            None
        );
        assert_eq!(
            routes.provider_for("endpoint-2", "model-a", "session-a"),
            Some("provider-2".to_string())
        );
    }
}
