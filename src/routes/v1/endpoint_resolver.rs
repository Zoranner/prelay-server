use crate::storage::ProtocolAccess;
use crate::{
    error::AppError, models::ProviderConfig, providers::spec::UpstreamProtocol,
    routes::v1::auth::CurrentProtocolAccess, AppState,
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
) -> Result<Vec<ResolvedEndpointProvider>, AppError> {
    let access = ProtocolAccess {
        identity_id: access.identity_id.clone(),
        endpoint_id: access.endpoint_id.clone(),
        endpoint_name: access.endpoint_name.clone(),
    };
    let resolved = state
        .storage
        .select_protocol_model_candidates(&access, model)
        .await?;
    let candidates = resolved
        .into_iter()
        .filter_map(|resolved| {
            let provider_spec =
                crate::providers::spec::ProviderSpec::from_provider_config(&resolved.provider);
            provider_spec
                .upstream_for_downstream(downstream_protocol)
                .map(|upstream_protocol| ResolvedEndpointProvider {
                    provider: resolved.provider,
                    model_upstream: resolved.model.upstream_model,
                    upstream_protocol,
                })
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(AppError::BadRequest(format!(
            "接入点未配置支持 {downstream_protocol} 的模型 {model}"
        )));
    }
    Ok(candidates)
}

#[cfg(test)]
pub(crate) struct TestEndpointAuth {
    pub(crate) access: axum::extract::Extension<CurrentProtocolAccess>,
    pub(crate) token: String,
}

#[cfg(test)]
pub(crate) async fn create_test_endpoint_auth(
    db: &sqlx::SqlitePool,
    provider: &crate::models::ProviderConfig,
    model_name: &str,
    upstream_model: &str,
) -> TestEndpointAuth {
    create_test_endpoint_auth_with_candidates(
        db,
        std::slice::from_ref(provider),
        model_name,
        upstream_model,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn create_test_endpoint_auth_with_candidates(
    db: &sqlx::SqlitePool,
    providers: &[crate::models::ProviderConfig],
    model_name: &str,
    upstream_model: &str,
) -> TestEndpointAuth {
    let storage = test_storage(db).await;
    let identity = storage
        .register_identity(
            "test-machine",
            &uuid::Uuid::new_v4().to_string(),
            &crate::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity");
    let mut models = Vec::with_capacity(providers.len());
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
                    models: vec![upstream_model.to_string()],
                },
            )
            .await
            .expect("create identity provider");
        models.push(prelay_protocol::EndpointModelInput {
            provider_id,
            upstream_model: upstream_model.to_string(),
            model_name: Some(model_name.to_string()),
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
    test_endpoint_auth(identity.identity_id, endpoint)
}

#[cfg(test)]
pub(crate) async fn create_empty_test_endpoint_auth(db: &sqlx::SqlitePool) -> TestEndpointAuth {
    let storage = test_storage(db).await;
    let identity = storage
        .register_identity(
            "test-machine",
            &uuid::Uuid::new_v4().to_string(),
            &crate::identity::credential::generate_credential(),
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
    test_endpoint_auth(identity.identity_id, endpoint)
}

#[cfg(test)]
async fn test_storage(db: &sqlx::SqlitePool) -> crate::storage::Storage {
    crate::storage::Storage::initialize(db.clone(), crate::storage::MasterKey::from_bytes([0; 32]))
        .await
        .expect("initialize identity storage")
}

#[cfg(test)]
fn test_endpoint_auth(
    identity_id: String,
    endpoint: prelay_protocol::EndpointResponse,
) -> TestEndpointAuth {
    TestEndpointAuth {
        access: axum::extract::Extension(CurrentProtocolAccess {
            identity_id,
            endpoint_id: endpoint.id,
            endpoint_name: endpoint.name,
        }),
        token: endpoint.token,
    }
}
