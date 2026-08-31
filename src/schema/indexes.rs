use sea_query::{Index, IndexCreateStatement};

use super::tables::{
    activities::Activities,
    activity_contents::ActivityContents,
    endpoints::{EndpointModels, EndpointRoutes},
    identity::Identities,
    memories::Memories,
    memory_sources::MemorySources,
    model_aliases::ModelAliases,
    providers::ProviderModels,
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

pub(super) fn activities_identity_created_at() -> IndexCreateStatement {
    Index::create()
        .name("idx_identity_activities_identity_created_at")
        .table(Activities::Table)
        .col(Activities::IdentityId)
        .col(Activities::CreatedAt)
        .to_owned()
}

pub(super) fn activity_contents_activity_id() -> IndexCreateStatement {
    Index::create()
        .name("uq_activity_contents_activity_id")
        .table(ActivityContents::Table)
        .col(ActivityContents::ActivityId)
        .unique()
        .to_owned()
}

pub(super) fn activity_contents_due() -> IndexCreateStatement {
    Index::create()
        .name("idx_activity_contents_due")
        .table(ActivityContents::Table)
        .col(ActivityContents::Status)
        .col(ActivityContents::NextAttemptAt)
        .col(ActivityContents::LeaseExpiresAt)
        .to_owned()
}

pub(super) fn memories_normalized_key() -> IndexCreateStatement {
    Index::create()
        .name("uq_memories_normalized_key")
        .table(Memories::Table)
        .col(Memories::NormalizedKey)
        .unique()
        .to_owned()
}

pub(super) fn memory_sources_identity_observed_at() -> IndexCreateStatement {
    Index::create()
        .name("idx_memory_sources_identity_observed_at")
        .table(MemorySources::Table)
        .col(MemorySources::IdentityId)
        .col(MemorySources::ObservedAt)
        .to_owned()
}

pub(super) fn memory_sources_unique() -> IndexCreateStatement {
    Index::create()
        .name("uq_memory_sources_memory_identity_evidence_observed")
        .table(MemorySources::Table)
        .col(MemorySources::MemoryId)
        .col(MemorySources::IdentityId)
        .col(MemorySources::EvidenceHash)
        .col(MemorySources::ObservedAt)
        .unique()
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
