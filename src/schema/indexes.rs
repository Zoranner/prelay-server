use sea_query::{Index, IndexCreateStatement};

use super::tables::{
    endpoints::{EndpointModels, EndpointRoutes},
    identity::Identities,
    model_aliases::ModelAliases,
    providers::ProviderModels,
    request_logs::RequestLogs,
    sessions::ResponseSessions,
};

pub(super) fn identities_machine_sid() -> IndexCreateStatement {
    Index::create()
        .name("uq_identities_machine_sid")
        .col(Identities::MachineId)
        .col(Identities::AccountSid)
        .unique()
        .to_owned()
}

pub(super) fn provider_models_name() -> IndexCreateStatement {
    Index::create()
        .name("uq_provider_models_name")
        .col(ProviderModels::ProviderId)
        .col(ProviderModels::ModelName)
        .unique()
        .to_owned()
}

pub(super) fn endpoint_models_candidate() -> IndexCreateStatement {
    Index::create()
        .name("uq_endpoint_models_candidate")
        .col(EndpointModels::EndpointId)
        .col(EndpointModels::ModelName)
        .col(EndpointModels::ProviderId)
        .col(EndpointModels::UpstreamModel)
        .unique()
        .to_owned()
}

pub(super) fn endpoint_routes_primary_key() -> IndexCreateStatement {
    Index::create()
        .col(EndpointRoutes::EndpointId)
        .col(EndpointRoutes::ModelName)
        .to_owned()
}

pub(super) fn response_sessions_primary_key() -> IndexCreateStatement {
    Index::create()
        .col(ResponseSessions::ResponseId)
        .col(ResponseSessions::IdentityId)
        .to_owned()
}

pub(super) fn request_logs_identity_created_at() -> IndexCreateStatement {
    Index::create()
        .name("idx_identity_request_logs_identity_created_at")
        .table(RequestLogs::Table)
        .col(RequestLogs::IdentityId)
        .col(RequestLogs::CreatedAt)
        .to_owned()
}

pub(super) fn model_aliases_alias() -> IndexCreateStatement {
    Index::create()
        .name("uq_identity_model_aliases_alias")
        .col(ModelAliases::IdentityId)
        .col(ModelAliases::Alias)
        .unique()
        .to_owned()
}
