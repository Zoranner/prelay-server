use reqwest::RequestBuilder;

use crate::provider_catalog::{CatalogRequestHeader, CatalogRequestHeaderValue};

/// 应用目录声明请求头时可用的请求上下文。
#[derive(Clone, Copy, Default)]
pub struct RequestHeaderContext<'a> {
    pub session_id: Option<&'a str>,
}

/// 按目录声明附加静态头或会话头；未配置或值缺失时跳过该头。
pub fn with_provider_headers(
    request: RequestBuilder,
    headers: &[CatalogRequestHeader],
    context: &RequestHeaderContext<'_>,
) -> RequestBuilder {
    headers.iter().fold(request, |request, header| {
        let value = match &header.value {
            CatalogRequestHeaderValue::Literal(value) => Some(value.as_str()),
            CatalogRequestHeaderValue::SessionId => context.session_id,
        };
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return request;
        };
        request.header(header.name.as_str(), value)
    })
}

#[cfg(test)]
mod tests {
    use axum::http::header::HeaderName;

    use crate::provider_catalog::{CatalogRequestHeader, CatalogRequestHeaderValue};

    use super::{with_provider_headers, RequestHeaderContext};

    #[test]
    fn applies_literal_and_session_headers() {
        let headers = [
            header(
                "x-api-version",
                CatalogRequestHeaderValue::Literal("2026-10-28".into()),
            ),
            header("x-session-id", CatalogRequestHeaderValue::SessionId),
        ];
        let request = with_provider_headers(
            reqwest::Client::new().post("https://provider.example/v1/responses"),
            &headers,
            &RequestHeaderContext {
                session_id: Some("ses_test_123"),
            },
        )
        .build()
        .expect("build request");

        assert_eq!(
            request
                .headers()
                .get("x-api-version")
                .and_then(|value| value.to_str().ok()),
            Some("2026-10-28")
        );
        assert_eq!(
            request
                .headers()
                .get("x-session-id")
                .and_then(|value| value.to_str().ok()),
            Some("ses_test_123")
        );
    }

    #[test]
    fn skips_missing_values_without_affecting_other_headers() {
        let headers = [header("x-session-id", CatalogRequestHeaderValue::SessionId)];
        let request = with_provider_headers(
            reqwest::Client::new().post("https://provider.example/v1/responses"),
            &headers,
            &RequestHeaderContext {
                session_id: Some("  "),
            },
        )
        .build()
        .expect("build request without session header");

        assert!(request.headers().get("x-session-id").is_none());
    }

    fn header(name: &'static str, value: CatalogRequestHeaderValue) -> CatalogRequestHeader {
        CatalogRequestHeader {
            name: HeaderName::from_static(name),
            value,
        }
    }
}
