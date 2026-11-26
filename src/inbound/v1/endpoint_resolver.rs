use crate::store::storage::ProtocolAccess;
use crate::{
    codec::providers::spec::UpstreamProtocol, domain::registry::ProviderConfig, error::AppError,
    inbound::v1::auth::CurrentProtocolAccess, AppState,
};

#[derive(Clone)]
pub(crate) struct ResolvedEndpointProvider {
    pub provider: ProviderConfig,
    pub model_upstream: String,
    pub upstream_protocol: UpstreamProtocol,
}

pub async fn resolve_endpoint_model_candidates(
    state: &AppState,
    access: &CurrentProtocolAccess,
    model: &str,
    downstream_protocol: &str,
    session_key: Option<&str>,
) -> Result<Vec<ResolvedEndpointProvider>, AppError> {
    let endpoint_id = access.endpoint_id.clone();
    let access = ProtocolAccess {
        identity_id: access.identity_id.clone(),
        endpoint_id: access.endpoint_id.clone(),
        endpoint_name: access.endpoint_name.clone(),
    };
    let resolved = state
        .storage
        .resolve_protocol_models(&access, model)
        .await?;
    let mut not_enabled = false;
    let mut candidates = resolved
        .into_iter()
        .filter_map(|resolved| {
            if !resolved.provider.provides_model(model) {
                not_enabled = true;
                return None;
            }
            let provider_spec = crate::codec::providers::spec::ProviderSpec::from_provider_config(
                &state.provider_catalog,
                &resolved.provider,
            );
            provider_spec
                .upstream_for_downstream(downstream_protocol)
                .map(|upstream_protocol| ResolvedEndpointProvider {
                    provider: resolved.provider,
                    model_upstream: resolved.model.upstream_model,
                    upstream_protocol,
                })
        })
        .collect::<Vec<_>>();
    prefer_session_provider(state, &endpoint_id, model, session_key, &mut candidates);
    if candidates.is_empty() {
        if not_enabled {
            return Err(AppError::BadRequest(format!(
                "model={model} is not enabled by the provider"
            )));
        }
        return Err(AppError::BadRequest(format!(
            "endpoint has no model={model} for protocol={downstream_protocol}"
        )));
    }
    Ok(candidates)
}

pub async fn resolve_language_model_candidates(
    state: &AppState,
    access: &CurrentProtocolAccess,
    model: &str,
    downstream_protocol: &str,
    session_key: Option<&str>,
) -> Result<Vec<ResolvedEndpointProvider>, AppError> {
    resolve_endpoint_model_candidates(state, access, model, downstream_protocol, session_key).await
}

fn prefer_session_provider(
    state: &AppState,
    endpoint_id: &str,
    model: &str,
    session_key: Option<&str>,
    candidates: &mut Vec<ResolvedEndpointProvider>,
) {
    let Some(session_key) = session_key else {
        return;
    };
    let Some(provider_id) = state
        .session_routes
        .provider_for(endpoint_id, model, session_key)
    else {
        return;
    };
    let Some(index) = candidates
        .iter()
        .position(|candidate| candidate.provider.id == provider_id)
    else {
        return;
    };
    let preferred = candidates.remove(index);
    candidates.insert(0, preferred);
}

#[cfg(test)]
pub(crate) struct TestEndpointAuth {
    pub(crate) access: axum::extract::Extension<CurrentProtocolAccess>,
    pub(crate) token: String,
    pub(crate) provider_ids: Vec<String>,
}

#[cfg(test)]
pub(crate) async fn create_test_endpoint_auth(
    storage: &crate::store::storage::Storage,
    provider: &crate::domain::registry::ProviderConfig,
    model_name: &str,
    upstream_model: &str,
) -> TestEndpointAuth {
    create_test_endpoint_auth_with_candidates(
        storage,
        std::slice::from_ref(provider),
        model_name,
        upstream_model,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn create_test_endpoint_auth_with_candidates(
    storage: &crate::store::storage::Storage,
    providers: &[crate::domain::registry::ProviderConfig],
    _model_name: &str,
    upstream_model: &str,
) -> TestEndpointAuth {
    let identity = storage
        .register_identity(
            "test-machine",
            &uuid::Uuid::new_v4().to_string(),
            &crate::domain::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    let mut models = Vec::with_capacity(providers.len());
    let mut provider_ids = Vec::with_capacity(providers.len());
    for provider in providers {
        let provider_id = storage
            .create_provider(
                &identity.identity_id,
                prelay_protocol::CreateProviderRequest {
                    name: provider.name.clone(),
                    provider_type: provider.provider_type.clone(),
                    base_url: provider.base_url.clone(),
                    api_key: provider.api_key.clone(),
                    capabilities: provider
                        .capabilities_json
                        .as_deref()
                        .and_then(|value| serde_json::from_str(value).ok()),
                    models: Some(vec![upstream_model.to_string()]),
                },
            )
            .await
            .expect("create identity provider");
        provider_ids.push(provider_id.clone());
        models.push(prelay_protocol::EndpointModelInput {
            provider_id,
            upstream_model: upstream_model.to_string(),
        });
    }
    let endpoint = storage
        .create_interface(
            &identity.identity_id,
            prelay_protocol::CreateEndpointRequest {
                name: "Test Endpoint".to_string(),
                protocol: Some("test".to_string()),
                models,
            },
        )
        .await
        .expect("create identity endpoint");
    test_endpoint_auth(identity.identity_id, endpoint, provider_ids)
}

#[cfg(test)]
pub(crate) async fn create_empty_test_endpoint_auth(
    storage: &crate::store::storage::Storage,
) -> TestEndpointAuth {
    let identity = storage
        .register_identity(
            "test-machine",
            &uuid::Uuid::new_v4().to_string(),
            &crate::domain::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    let endpoint = storage
        .create_interface(
            &identity.identity_id,
            prelay_protocol::CreateEndpointRequest {
                name: "Test Endpoint".to_string(),
                protocol: Some("test".to_string()),
                models: Vec::new(),
            },
        )
        .await
        .expect("create identity endpoint");
    test_endpoint_auth(identity.identity_id, endpoint, Vec::new())
}

#[cfg(test)]
pub(crate) async fn test_provider(
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
) -> anyhow::Result<crate::domain::registry::ProviderConfig> {
    test_provider_with_capabilities(name, provider_type, base_url, api_key, None).await
}

#[cfg(test)]
pub(crate) async fn test_provider_with_capabilities(
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    capabilities: Option<&crate::domain::registry::ProviderCapabilityOverrides>,
) -> anyhow::Result<crate::domain::registry::ProviderConfig> {
    Ok(crate::domain::registry::ProviderConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        provider_type: provider_type.to_string(),
        base_url: base_url.to_string(),
        api_key: api_key.to_string(),
        capabilities_json: capabilities.map(serde_json::to_string).transpose()?,
        models: Vec::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
fn test_endpoint_auth(
    identity_id: String,
    endpoint: prelay_protocol::EndpointResponse,
    provider_ids: Vec<String>,
) -> TestEndpointAuth {
    TestEndpointAuth {
        access: axum::extract::Extension(CurrentProtocolAccess {
            identity_id,
            endpoint_id: endpoint.id,
            endpoint_name: endpoint.name,
            session_id: None,
            thread_id: None,
            window_id: None,
            window_generation: None,
        }),
        token: endpoint.token,
        provider_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stats::ActivityInsert;

    fn activity(provider_id: &str, upstream_latency_ms: i64) -> ActivityInsert {
        ActivityInsert {
            protocol_in: "chat_completions".to_string(),
            protocol_out: "chat_completions".to_string(),
            protocol_upstream: "chat_completions".to_string(),
            endpoint_name: "Test Endpoint".to_string(),
            provider_id: provider_id.to_string(),
            provider_name: "test".to_string(),
            provider_type: "openai_compatible".to_string(),
            model_requested: "deepseek-chat".to_string(),
            model_upstream: "deepseek-chat".to_string(),
            status: "success".to_string(),
            http_status: 200,
            latency_ms: upstream_latency_ms,
            upstream_latency_ms: Some(upstream_latency_ms),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn ignores_observed_latency_and_keeps_the_configured_order() {
        let state = crate::test_support::test_state().await;
        let primary = test_provider(
            "Primary",
            "openai_compatible",
            "http://127.0.0.1:1",
            "sk-primary",
        )
        .await
        .expect("create primary provider");
        let backup = test_provider(
            "Backup",
            "openai_compatible",
            "http://127.0.0.1:1",
            "sk-backup",
        )
        .await
        .expect("create backup provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[primary, backup],
            "deepseek-chat",
            "deepseek-chat",
        )
        .await;
        let primary_id = auth.provider_ids[0].clone();
        let backup_id = auth.provider_ids[1].clone();
        let identity_id = auth.access.0.identity_id.clone();
        state
            .storage
            .insert_activity_with_id(
                &identity_id,
                "slow-primary".to_string(),
                activity(&primary_id, 900),
            )
            .await
            .expect("record primary latency");
        state
            .storage
            .insert_activity_with_id(
                &identity_id,
                "fast-backup".to_string(),
                activity(&backup_id, 10),
            )
            .await
            .expect("record backup latency");

        let candidates = resolve_endpoint_model_candidates(
            &state,
            &auth.access.0,
            "deepseek-chat",
            "chat_completions",
            None,
        )
        .await
        .expect("resolve endpoint candidates");

        assert_eq!(candidates[0].provider.id, primary_id);
    }

    #[tokio::test]
    async fn prefers_the_session_provider_over_the_configured_order() {
        let state = crate::test_support::test_state().await;
        let primary = test_provider(
            "Primary",
            "openai_compatible",
            "http://127.0.0.1:1",
            "sk-primary",
        )
        .await
        .expect("create primary provider");
        let backup = test_provider(
            "Backup",
            "openai_compatible",
            "http://127.0.0.1:1",
            "sk-backup",
        )
        .await
        .expect("create backup provider");
        let auth = create_test_endpoint_auth_with_candidates(
            &state.storage,
            &[primary, backup],
            "deepseek-chat",
            "deepseek-chat",
        )
        .await;
        let primary_id = auth.provider_ids[0].clone();
        let backup_id = auth.provider_ids[1].clone();
        state.session_routes.remember(
            &auth.access.0.endpoint_id,
            "deepseek-chat",
            "session-a",
            &backup_id,
        );

        let sticky = resolve_endpoint_model_candidates(
            &state,
            &auth.access.0,
            "deepseek-chat",
            "chat_completions",
            Some("session-a"),
        )
        .await
        .expect("resolve sticky session");
        assert_eq!(sticky[0].provider.id, backup_id);

        let new_session = resolve_endpoint_model_candidates(
            &state,
            &auth.access.0,
            "deepseek-chat",
            "chat_completions",
            Some("session-b"),
        )
        .await
        .expect("resolve new session");
        assert_eq!(new_session[0].provider.id, primary_id);
    }
}
