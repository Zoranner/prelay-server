#[cfg(test)]
use crate::db;
#[cfg(not(test))]
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
    #[cfg(test)]
    {
        let interface_model =
            crate::db::get_interface_model(&state.db, &access.interface_id, model)
                .await?
                .ok_or_else(|| {
                    AppError::BadRequest(format!("接口 Test Interface 未配置模型 {model}"))
                })?;
        let provider = crate::db::get_config_by_id(&state.db, &interface_model.provider_id)
            .await?
            .ok_or_else(|| AppError::BadRequest("模型关联的供应商不存在".to_string()))?;
        let provider_spec = crate::providers::spec::ProviderSpec::from_provider_config(&provider);
        let upstream_protocol = provider_spec
            .upstream_for_downstream(downstream_protocol)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "供应商 {} 不支持接口协议 {}",
                    provider.name, downstream_protocol
                ))
            })?;
        Ok(ResolvedInterfaceProvider {
            provider,
            model_upstream: interface_model.upstream_model,
            upstream_protocol,
        })
    }
    #[cfg(not(test))]
    {
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
    db::create_provider_model(db, &provider.id, upstream_model)
        .await
        .expect("create provider model");
    let interface = db::create_interface(db, "Test Interface", "test")
        .await
        .expect("create interface");
    db::create_interface_model(db, &interface.id, model_name, &provider.id, upstream_model)
        .await
        .expect("create interface model");
    test_interface_auth(interface)
}

#[cfg(test)]
pub(crate) async fn create_empty_test_interface_auth(db: &sqlx::SqlitePool) -> TestInterfaceAuth {
    let interface = db::create_interface(db, "Test Interface", "test")
        .await
        .expect("create interface");
    test_interface_auth(interface)
}

#[cfg(test)]
fn test_interface_auth(interface: crate::models::InterfaceConfig) -> TestInterfaceAuth {
    TestInterfaceAuth {
        access: axum::extract::Extension(CurrentProtocolAccess {
            identity_id: "test-identity".to_string(),
            interface_id: interface.id,
        }),
        token: interface.token,
    }
}
