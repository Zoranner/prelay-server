use crate::storage::ProtocolAccess;
use crate::{
    db::ResolvedInterfaceProvider, error::AppError, routes::v1::auth::CurrentProtocolAccess,
    AppState,
};

pub async fn resolve_interface_model(
    state: &AppState,
    access: &CurrentProtocolAccess,
    model: &str,
    downstream_protocol: &str,
) -> Result<ResolvedInterfaceProvider, AppError> {
    let access = ProtocolAccess {
        identity_id: access.identity_id.clone(),
        interface_id: access.interface_id.clone(),
    };
    let resolved = state
        .storage
        .resolve_protocol_model(&access, model)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("接口未配置模型 {model}")))?;
    let provider_spec =
        crate::providers::spec::ProviderSpec::from_provider_config(&resolved.provider);
    let upstream_protocol = provider_spec
        .upstream_for_downstream(downstream_protocol)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "供应商 {} 不支持接口协议 {}",
                resolved.provider.name, downstream_protocol
            ))
        })?;

    Ok(ResolvedInterfaceProvider {
        provider: resolved.provider,
        model_upstream: resolved.model.upstream_model,
        upstream_protocol,
    })
}

#[cfg(test)]
pub(crate) struct TestInterfaceAuth {
    pub(crate) access: axum::extract::Extension<CurrentProtocolAccess>,
    pub(crate) token: String,
}

#[cfg(test)]
pub(crate) async fn create_test_interface_auth(
    db: &sqlx::SqlitePool,
    provider: &crate::models::ProviderConfig,
    model_name: &str,
    upstream_model: &str,
) -> TestInterfaceAuth {
    let storage = test_storage(db).await;
    let identity = storage
        .register_identity("test-machine", &uuid::Uuid::new_v4().to_string())
        .await
        .expect("register identity");
    let provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_relay_protocol::CreateProviderRequest {
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
    let interface = storage
        .create_interface(
            &identity.identity_id,
            provider_relay_protocol::CreateInterfaceRequest {
                name: "Test Interface".to_string(),
                protocol: Some("test".to_string()),
                models: vec![provider_relay_protocol::InterfaceModelInput {
                    provider_id,
                    upstream_model: upstream_model.to_string(),
                    model_name: Some(model_name.to_string()),
                }],
            },
        )
        .await
        .expect("create identity interface");
    test_interface_auth(identity.identity_id, interface)
}

#[cfg(test)]
pub(crate) async fn create_empty_test_interface_auth(db: &sqlx::SqlitePool) -> TestInterfaceAuth {
    let storage = test_storage(db).await;
    let identity = storage
        .register_identity("test-machine", &uuid::Uuid::new_v4().to_string())
        .await
        .expect("register identity");
    let interface = storage
        .create_interface(
            &identity.identity_id,
            provider_relay_protocol::CreateInterfaceRequest {
                name: "Test Interface".to_string(),
                protocol: Some("test".to_string()),
                models: Vec::new(),
            },
        )
        .await
        .expect("create identity interface");
    test_interface_auth(identity.identity_id, interface)
}

#[cfg(test)]
async fn test_storage(db: &sqlx::SqlitePool) -> crate::storage::Storage {
    crate::storage::Storage::initialize(db.clone(), crate::storage::MasterKey::from_bytes([0; 32]))
        .await
        .expect("initialize identity storage")
}

#[cfg(test)]
fn test_interface_auth(
    identity_id: String,
    interface: provider_relay_protocol::InterfaceResponse,
) -> TestInterfaceAuth {
    TestInterfaceAuth {
        access: axum::extract::Extension(CurrentProtocolAccess {
            identity_id,
            interface_id: interface.id,
        }),
        token: interface.token,
    }
}
