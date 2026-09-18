use crate::{
    stats::{ActivityInsert, ModelStatsScope, StatsRange},
    storage::Storage,
};

#[tokio::test]
async fn model_stats_resolve_display_names_without_splitting_model_ids() {
    let storage = test_storage().await;
    let identity = register_identity(&storage, "model-display-names").await;
    for id in ["model-known-1", "model-known-2"] {
        let mut log = test_log(Some(3), Some(4));
        log.model_requested = "gpt-5.6-luna".to_string();
        storage
            .insert_activity_with_id(&identity, id.to_string(), log)
            .await
            .expect("insert known model log");
    }
    let mut unknown = test_log(Some(5), Some(6));
    unknown.model_requested = "unknown-model".to_string();
    storage
        .insert_activity_with_id(&identity, "model-unknown".to_string(), unknown)
        .await
        .expect("insert unknown model log");

    let catalog = crate::test_support::fixture_catalog();
    let rows = storage
        .model_stats_with_catalog(
            &identity,
            ModelStatsScope::Personal,
            StatsRange::All,
            &catalog,
        )
        .await
        .expect("load model stats with catalog");

    let known = rows
        .iter()
        .find(|row| row.model_requested.as_deref() == Some("gpt-5.6-luna"))
        .expect("known model stats");
    assert_eq!(
        known.model_requested_display_name.as_deref(),
        Some("GPT-5.6 Luna")
    );
    assert_eq!(known.total_requests, 2);
    let unknown = rows
        .iter()
        .find(|row| row.model_requested.as_deref() == Some("unknown-model"))
        .expect("unknown model stats");
    assert_eq!(
        unknown.model_requested_display_name.as_deref(),
        Some("unknown-model")
    );
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn model_stats_scope_selects_personal_or_team_aggregation() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "scope-a").await;
    let identity_b = register_identity(&storage, "scope-b").await;
    for (identity, id, model, input_tokens, output_tokens) in [
        (&identity_a, "scope-a-shared", "shared-model", 3, 4),
        (&identity_b, "scope-b-shared", "shared-model", 5, 6),
        (&identity_b, "scope-b-other", "other-model", 7, 8),
    ] {
        let mut log = test_log(Some(input_tokens), Some(output_tokens));
        log.model_requested = model.to_string();
        storage
            .insert_activity_with_id(identity, id.to_string(), log)
            .await
            .expect("insert scoped model log");
    }

    let catalog = crate::test_support::fixture_catalog();
    let personal = storage
        .model_stats_with_catalog(
            &identity_a,
            ModelStatsScope::Personal,
            StatsRange::All,
            &catalog,
        )
        .await
        .expect("load personal model stats");
    assert_eq!(personal.len(), 1);
    assert_eq!(personal[0].model_requested.as_deref(), Some("shared-model"));
    assert_eq!(personal[0].total_requests, 1);
    assert_eq!(personal[0].input_tokens, 3);
    assert_eq!(personal[0].output_tokens, 4);

    let team = storage
        .model_stats_with_catalog(
            &identity_a,
            ModelStatsScope::Team,
            StatsRange::All,
            &catalog,
        )
        .await
        .expect("load team model stats");
    assert_eq!(team.len(), 2);
    assert_eq!(team[0].model_requested.as_deref(), Some("shared-model"));
    assert_eq!(team[0].total_requests, 2);
    assert_eq!(team[0].input_tokens, 8);
    assert_eq!(team[0].output_tokens, 10);
    assert_eq!(team[1].model_requested.as_deref(), Some("other-model"));
    assert_eq!(team[1].total_requests, 1);
}

#[tokio::test]
async fn overview_is_scoped_to_one_identity() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "stats-a").await;
    let identity_b = register_identity(&storage, "stats-b").await;
    storage
        .insert_activity_with_id(
            &identity_a,
            "stats-a-success".to_string(),
            test_log(Some(3), Some(4)),
        )
        .await
        .expect("insert identity A log");
    let mut failed = test_log(Some(50), Some(60));
    failed.status = "failed".to_string();
    failed.http_status = 502;
    storage
        .insert_activity_with_id(&identity_b, "stats-b-failed".to_string(), failed)
        .await
        .expect("insert identity B log");

    let overview = storage
        .stats_overview(&identity_a, ModelStatsScope::Personal, StatsRange::Today)
        .await
        .expect("load identity A overview");

    assert_eq!(overview.total_requests, 1);
    assert_eq!(overview.successful_requests, 1);
    assert_eq!(overview.failed_requests, 0);
    assert_eq!(overview.input_tokens, 3);
    assert_eq!(overview.output_tokens, 4);
}

#[tokio::test]
async fn overview_scope_selects_personal_or_team_aggregation() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "overview-scope-a").await;
    let identity_b = register_identity(&storage, "overview-scope-b").await;
    storage
        .insert_activity_with_id(
            &identity_a,
            "overview-scope-a-log".to_string(),
            test_log(Some(3), Some(4)),
        )
        .await
        .expect("insert identity A log");
    storage
        .insert_activity_with_id(
            &identity_b,
            "overview-scope-b-log".to_string(),
            test_log(Some(5), Some(6)),
        )
        .await
        .expect("insert identity B log");

    let personal = storage
        .stats_overview(&identity_a, ModelStatsScope::Personal, StatsRange::Today)
        .await
        .expect("load personal overview");
    assert_eq!(personal.total_requests, 1);
    assert_eq!(personal.successful_requests, 1);
    assert_eq!(personal.input_tokens, 3);
    assert_eq!(personal.output_tokens, 4);

    let team = storage
        .stats_overview(&identity_a, ModelStatsScope::Team, StatsRange::Today)
        .await
        .expect("load team overview");
    assert_eq!(team.total_requests, 2);
    assert_eq!(team.successful_requests, 2);
    assert_eq!(team.failed_requests, 0);
    assert_eq!(team.input_tokens, 8);
    assert_eq!(team.output_tokens, 10);
    assert_eq!(team.average_latency_ms, Some(10));
}

#[tokio::test]
async fn provider_stats_scope_selects_personal_or_team_aggregation() {
    let storage = test_storage().await;
    let identity_a = register_identity(&storage, "provider-scope-a").await;
    let identity_b = register_identity(&storage, "provider-scope-b").await;
    let mut identity_a_log = test_log(Some(3), Some(4));
    identity_a_log.provider_id = "provider-shared".to_string();
    identity_a_log.provider_name = "Provider Shared".to_string();
    identity_a_log.first_token_ms = Some(100);
    let mut identity_b_log = identity_a_log.clone();
    identity_b_log.input_tokens = Some(5);
    identity_b_log.output_tokens = Some(6);
    identity_b_log.first_token_ms = Some(300);
    storage
        .insert_activity_with_id(
            &identity_a,
            "provider-scope-a-log".to_string(),
            identity_a_log,
        )
        .await
        .expect("insert identity A log");
    storage
        .insert_activity_with_id(
            &identity_b,
            "provider-scope-b-log".to_string(),
            identity_b_log,
        )
        .await
        .expect("insert identity B log");

    let personal = storage
        .provider_stats(&identity_a, ModelStatsScope::Personal, StatsRange::Today)
        .await
        .expect("load personal provider stats");
    assert_eq!(personal.len(), 1);
    assert_eq!(personal[0].provider_id.as_deref(), Some("provider-shared"));
    assert_eq!(
        personal[0].provider_name.as_deref(),
        Some("Provider Shared")
    );
    assert_eq!(personal[0].total_requests, 1);
    assert_eq!(personal[0].input_tokens, 3);
    assert_eq!(personal[0].output_tokens, 4);
    assert_eq!(personal[0].average_first_token_ms, Some(100.0));

    let team = storage
        .provider_stats(&identity_a, ModelStatsScope::Team, StatsRange::Today)
        .await
        .expect("load team provider stats");
    assert_eq!(team.len(), 1);
    assert_eq!(team[0].provider_id.as_deref(), Some("provider-shared"));
    assert_eq!(team[0].total_requests, 2);
    assert_eq!(team[0].input_tokens, 8);
    assert_eq!(team[0].output_tokens, 10);
    assert_eq!(team[0].average_first_token_ms, Some(200.0));
}

pub(super) async fn test_storage() -> Storage {
    crate::test_support::test_storage().await
}

pub(super) async fn register_identity(storage: &Storage, suffix: &str) -> String {
    storage
        .register_identity(
            &format!("machine-{suffix}"),
            &format!("sid-{suffix}"),
            &crate::identity::credential::generate_credential(),
        )
        .await
        .expect("register identity")
        .identity_id
}

pub(super) fn test_log(input_tokens: Option<i64>, output_tokens: Option<i64>) -> ActivityInsert {
    ActivityInsert {
        protocol_in: "responses".to_string(),
        protocol_out: "responses".to_string(),
        protocol_upstream: "chat_completions".to_string(),
        endpoint_name: String::new(),
        provider_id: "provider-1".to_string(),
        provider_name: "Provider One".to_string(),
        model_requested: "model-1".to_string(),
        model_upstream: "model-1".to_string(),
        status: "success".to_string(),
        http_status: 200,
        error_code: None,
        error_message: None,
        is_streaming: false,
        input_tokens,
        output_tokens,
        reasoning_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        latency_ms: 10,
        upstream_latency_ms: None,
        first_token_ms: None,
        tool_call_count: None,
        upstream_request_id: None,
    }
}
