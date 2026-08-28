use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use prelay_protocol::{ExtensionKind, ExtensionSummary, ExtensionVersion, ProtocolErrorCode};

use crate::{
    error::AppError,
    extensions::{CatalogError, ExtensionCatalog},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/extensions/rules", get(list_rules))
        .route("/extensions/skills", get(list_skills))
        .route("/extensions/plugins", get(list_plugins))
        .route("/extensions/mcp", get(list_mcp))
        .route("/extensions/:name/versions", get(list_versions))
        .route("/extensions/:name/versions/:tag/readme", get(readme))
        .route(
            "/extensions/:name/versions/:tag/install",
            get(install_bundle),
        )
}

async fn list_rules(
    State(state): State<AppState>,
) -> Result<Json<Vec<ExtensionSummary>>, AppError> {
    list(&state.extensions, ExtensionKind::Rule).await
}

async fn list_skills(
    State(state): State<AppState>,
) -> Result<Json<Vec<ExtensionSummary>>, AppError> {
    list(&state.extensions, ExtensionKind::Skill).await
}

async fn list_plugins(
    State(state): State<AppState>,
) -> Result<Json<Vec<ExtensionSummary>>, AppError> {
    list(&state.extensions, ExtensionKind::Plugin).await
}

async fn list_mcp(State(state): State<AppState>) -> Result<Json<Vec<ExtensionSummary>>, AppError> {
    list(&state.extensions, ExtensionKind::Mcp).await
}

async fn list(
    catalog: &ExtensionCatalog,
    kind: ExtensionKind,
) -> Result<Json<Vec<ExtensionSummary>>, AppError> {
    Ok(Json(catalog.list(kind).await.map_err(catalog_error)?))
}

async fn list_versions(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<ExtensionVersion>>, AppError> {
    Ok(Json(
        state
            .extensions
            .versions(&name)
            .await
            .map_err(catalog_error)?,
    ))
}

async fn readme(
    State(state): State<AppState>,
    Path((name, tag)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let content = state
        .extensions
        .readme(&name, &tag)
        .await
        .map_err(catalog_error)?;
    Ok((
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        content,
    ))
}

async fn install_bundle(
    State(state): State<AppState>,
    Path((name, tag)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        state
            .extensions
            .install_bundle(&name, &tag)
            .await
            .map_err(catalog_error)?,
    ))
}

fn catalog_error(error: CatalogError) -> AppError {
    let (code, message) = match error {
        CatalogError::Unavailable => (
            ProtocolErrorCode::ExtensionCatalogUnavailable,
            "扩展目录暂不可用",
        ),
        CatalogError::ExtensionNotFound => (ProtocolErrorCode::ExtensionNotFound, "扩展不存在"),
        CatalogError::VersionNotFound => (
            ProtocolErrorCode::ExtensionVersionNotFound,
            "扩展版本不存在",
        ),
        CatalogError::InstallUnsupported => (
            ProtocolErrorCode::ExtensionInstallUnsupported,
            "当前扩展类型不支持安装",
        ),
    };
    AppError::Protocol {
        code,
        message: message.to_string(),
    }
}
