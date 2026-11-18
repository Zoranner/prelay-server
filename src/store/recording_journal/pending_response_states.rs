use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::store::storage::ResponseState;

#[derive(Clone, Default)]
pub(crate) struct PendingResponseStates {
    inner: Arc<RwLock<HashMap<(String, String), ResponseState>>>,
}

impl PendingResponseStates {
    pub(crate) fn insert(&self, state: ResponseState) {
        if let Ok(mut states) = self.inner.write() {
            states.insert(
                (state.identity_id.clone(), state.response_id.clone()),
                state,
            );
        }
    }

    pub(crate) fn get(&self, identity_id: &str, response_id: &str) -> Option<ResponseState> {
        self.inner.read().ok().and_then(|states| {
            states
                .get(&(identity_id.to_string(), response_id.to_string()))
                .cloned()
        })
    }

    pub(crate) fn remove(&self, identity_id: &str, response_id: &str) {
        if let Ok(mut states) = self.inner.write() {
            states.remove(&(identity_id.to_string(), response_id.to_string()));
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.inner
            .read()
            .map(|states| states.len())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::PendingResponseStates;
    use crate::store::storage::ResponseState;

    #[test]
    fn keeps_pending_state_until_materialization_removes_it() {
        let pending = PendingResponseStates::default();
        let state = ResponseState {
            identity_id: "identity-1".to_string(),
            response_id: "resp_1".to_string(),
            previous_response_id: None,
            conversation_id: Some("conversation-1".to_string()),
            provider_id: "provider-1".to_string(),
            model: "model-1".to_string(),
            upstream_response_id: None,
            request_items: json!([]),
            response_items: json!([]),
            created_at: "2026-11-17T12:00:00Z".to_string(),
            expires_at: "2026-12-17T12:00:00Z".to_string(),
        };

        pending.insert(state.clone());
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.get("identity-1", "resp_1").as_ref(), Some(&state));

        pending.remove("identity-1", "resp_1");
        assert_eq!(pending.len(), 0);
        assert!(pending.get("identity-1", "resp_1").is_none());
    }
}
