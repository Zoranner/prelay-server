use axum::http::HeaderMap;

use crate::{
    db::{self, ResolvedInterfaceProvider},
    error::AppError,
};

pub async fn resolve_interface_model(
    db: &sqlx::SqlitePool,
    headers: &HeaderMap,
    model: &str,
    downstream_protocol: &str,
) -> Result<ResolvedInterfaceProvider, AppError> {
    let token = crate::routes::v1::auth::extract_token(headers).ok_or(AppError::Unauthorized)?;
    let interface = db::get_interface_by_token(db, &token)
        .await?
        .ok_or(AppError::Unauthorized)?;
    let interface_model = db::get_interface_model(db, &interface.id, model)
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(format!("接口 {} 未配置模型 {model}", interface.name))
        })?;
    let provider = db::get_config_by_id(db, &interface_model.provider_id)
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

#[cfg(test)]
pub(crate) struct TestInterfaceAuth {
    pub(crate) headers: HeaderMap,
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
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        axum::http::HeaderValue::from_str(&format!("Bearer {}", interface.token))
            .expect("valid interface authorization header"),
    );
    TestInterfaceAuth {
        headers,
        token: interface.token,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::{create_test_interface_auth, resolve_interface_model, test_interface_auth};
    use crate::{db, error::AppError, providers::spec::UpstreamProtocol};

    #[tokio::test]
    async fn rejects_missing_interface_token_even_when_legacy_model_alias_exists() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek",
            "deepseek",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        db::create_provider_model(&db, &provider.id, "deepseek-chat")
            .await
            .expect("create provider model");
        db::create_model_alias(
            &db,
            "coder",
            &provider.id,
            "deepseek-chat",
            &["chat_completions"],
        )
        .await
        .expect("create legacy model alias");

        let result =
            resolve_interface_model(&db, &HeaderMap::new(), "coder", "chat_completions").await;

        assert!(matches!(result, Err(AppError::Unauthorized)));
    }

    #[tokio::test]
    async fn interface_token_resolves_model_for_any_supported_request_protocol() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek",
            "deepseek",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        let auth = create_test_interface_auth(&db, &provider, "coder", "deepseek-chat").await;

        for (downstream_protocol, expected_upstream_protocol) in [
            ("responses", UpstreamProtocol::ChatCompletions),
            ("chat_completions", UpstreamProtocol::ChatCompletions),
            ("anthropic_messages", UpstreamProtocol::AnthropicMessages),
        ] {
            let resolved =
                resolve_interface_model(&db, &auth.headers, "coder", downstream_protocol)
                    .await
                    .expect("resolve interface model");

            assert_eq!(resolved.provider.id, provider.id);
            assert_eq!(resolved.model_upstream, "deepseek-chat");
            assert_eq!(resolved.upstream_protocol, expected_upstream_protocol);
        }
    }

    #[tokio::test]
    async fn interface_token_cannot_resolve_model_configured_only_on_another_interface() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        db::init_schema(&db).await.expect("init schema");
        let provider = db::create_config(
            &db,
            "DeepSeek",
            "deepseek",
            "https://api.deepseek.com",
            "sk-upstream",
        )
        .await
        .expect("create provider");
        db::create_provider_model(&db, &provider.id, "deepseek-chat")
            .await
            .expect("create provider model");
        let interface_a = db::create_interface(&db, "Interface A", "all")
            .await
            .expect("create interface A");
        let interface_b = db::create_interface(&db, "Interface B", "all")
            .await
            .expect("create interface B");
        db::create_interface_model(&db, &interface_b.id, "coder", &provider.id, "deepseek-chat")
            .await
            .expect("create interface B model");
        let auth_a = test_interface_auth(interface_a);

        let error = match resolve_interface_model(&db, &auth_a.headers, "coder", "chat_completions")
            .await
        {
            Ok(_) => panic!("interface A must not resolve interface B model"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            AppError::BadRequest(message) if message == "接口 Interface A 未配置模型 coder"
        ));
    }
}
