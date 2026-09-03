use prelay_protocol::{CreateEndpointRequest, EndpointModelInput};
use prelay_server::{identity::credential::generate_credential, storage::ProtocolAccess};

use crate::support;

use super::fixtures::{activity, provider_input};

#[tokio::test]
async fn model_candidates_keep_mapping_order_then_prefer_observed_latency() {
    let storage = support::test_storage().await;
    let identity = storage
        .register_identity(
            "machine-priority",
            "S-1-5-21-priority",
            &generate_credential(),
        )
        .await
        .expect("register identity");
    let primary_provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input("Primary provider", "test-primary-key"),
        )
        .await
        .expect("create primary provider");
    let backup_provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input("Backup provider", "test-backup-key"),
        )
        .await
        .expect("create backup provider");
    let endpoint = storage
        .create_interface(
            &identity.identity_id,
            CreateEndpointRequest {
                name: "Priority endpoint".to_string(),
                protocol: Some("all".to_string()),
                models: vec![
                    EndpointModelInput {
                        provider_id: primary_provider_id.clone(),
                        upstream_model: "test-model".to_string(),
                        model_name: Some("test-model".to_string()),
                    },
                    EndpointModelInput {
                        provider_id: backup_provider_id.clone(),
                        upstream_model: "test-model".to_string(),
                        model_name: Some("test-model".to_string()),
                    },
                ],
            },
        )
        .await
        .expect("create endpoint with primary and backup");
    let access = ProtocolAccess {
        identity_id: identity.identity_id,
        endpoint_id: endpoint.id,
        endpoint_name: endpoint.name,
    };

    let candidates = storage
        .resolve_protocol_models(&access, "test-model")
        .await
        .expect("resolve candidates");
    assert_eq!(candidates[0].provider.id, primary_provider_id);
    assert_eq!(candidates[1].provider.id, backup_provider_id);

    storage
        .insert_activity_with_id(
            &access.identity_id,
            "primary-latency".to_string(),
            activity(&primary_provider_id, 120),
        )
        .await
        .expect("record primary latency");
    storage
        .insert_activity_with_id(
            &access.identity_id,
            "backup-latency".to_string(),
            activity(&backup_provider_id, 20),
        )
        .await
        .expect("record backup latency");

    let selected = storage
        .select_protocol_model_candidates(&access, "test-model")
        .await
        .expect("select candidates by latency");
    assert_eq!(selected[0].provider.id, backup_provider_id);

    storage
        .remember_protocol_model_provider(&access, "test-model", &primary_provider_id)
        .await
        .expect("remember current provider");
    let selected = storage
        .select_protocol_model_candidates(&access, "test-model")
        .await
        .expect("select candidates with remembered provider");
    assert_eq!(selected[0].provider.id, primary_provider_id);
}
