use prelay_protocol::{CreateEndpointRequest, EndpointModelInput};
use prelay_server::{identity::credential::generate_credential, storage::StorageError};

use crate::support;

use super::fixtures::provider_input;

#[tokio::test]
async fn provider_and_endpoint_writes_leave_no_partial_resources_after_validation_errors() {
    let storage = support::test_storage().await;
    let identity = storage
        .register_identity("machine-a", "S-1-5-21-100", &generate_credential())
        .await
        .expect("register identity");

    let mut invalid_provider = provider_input("Invalid provider", "test-provider-key");
    invalid_provider.models = vec!["test-model".to_string(), " test-model ".to_string()];
    let error = storage
        .create_provider(&identity.identity_id, invalid_provider)
        .await
        .expect_err("reject duplicate provider model");
    assert!(matches!(error, StorageError::ValidationFailed(_)));
    assert!(storage
        .list_providers(&identity.identity_id)
        .await
        .expect("list providers")
        .is_empty());

    let provider_id = storage
        .create_provider(
            &identity.identity_id,
            provider_input("Valid provider", "test-provider-key"),
        )
        .await
        .expect("create provider");
    let duplicate = EndpointModelInput {
        provider_id: provider_id.clone(),
        upstream_model: "test-model".to_string(),
    };
    let error = storage
        .create_interface(
            &identity.identity_id,
            CreateEndpointRequest {
                name: "Invalid endpoint".to_string(),
                protocol: None,
                models: vec![
                    duplicate.clone(),
                    EndpointModelInput {
                        upstream_model: " test-model ".to_string(),
                        ..duplicate
                    },
                ],
            },
        )
        .await
        .expect_err("reject duplicate endpoint mapping");
    assert!(matches!(error, StorageError::ValidationFailed(_)));
    assert!(storage
        .list_endpoints(&identity.identity_id)
        .await
        .expect("list endpoints")
        .is_empty());
}
