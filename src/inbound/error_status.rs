use axum::http::StatusCode;
use prelay_protocol::ProtocolErrorCode;

pub(crate) fn http_status_for(code: ProtocolErrorCode) -> StatusCode {
    match code {
        ProtocolErrorCode::NotFound => StatusCode::NOT_FOUND,
        ProtocolErrorCode::ClientUpdateUnavailable => StatusCode::NOT_FOUND,
        ProtocolErrorCode::ExtensionCatalogUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ProtocolErrorCode::ExtensionContentInvalid => StatusCode::UNPROCESSABLE_ENTITY,
        ProtocolErrorCode::ExtensionNotFound | ProtocolErrorCode::ExtensionVersionNotFound => {
            StatusCode::NOT_FOUND
        }
        ProtocolErrorCode::InvalidProviderSharing => StatusCode::BAD_REQUEST,
        ProtocolErrorCode::ProviderSharingNotAllowed => StatusCode::FORBIDDEN,
        ProtocolErrorCode::ProviderNotVisible => StatusCode::NOT_FOUND,
        ProtocolErrorCode::ProviderNotUsable => StatusCode::UNPROCESSABLE_ENTITY,
        ProtocolErrorCode::ProviderRouteUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ProtocolErrorCode::ProviderUpstreamUnavailable => StatusCode::BAD_GATEWAY,
        ProtocolErrorCode::ProviderModelRetired
        | ProtocolErrorCode::ProviderTypeUnsupported
        | ProtocolErrorCode::EndpointModelDuplicate
        | ProtocolErrorCode::EndpointModelNotInProviderModels => StatusCode::BAD_REQUEST,
        ProtocolErrorCode::InvalidCredential => StatusCode::UNAUTHORIZED,
        ProtocolErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        ProtocolErrorCode::ExtensionInstallUnsupported
        | ProtocolErrorCode::IdentityAlreadyRegistered
        | ProtocolErrorCode::ValidationFailed => StatusCode::BAD_REQUEST,
    }
}
