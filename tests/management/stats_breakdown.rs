use axum::http::StatusCode;
use prelay_protocol::{ModelStatsSummary, ProviderStatsSummary};

use crate::{auth::register, http::request_json, test_context};

use super::stats::{seed_activity, ActivitySeed};

#[tokio::test]
async fn provider_stats_merge_same_catalog_type_and_report_deleted_bucket() {
    let context = test_context::test_context().await;
    let app = context.app;
    let identity = register(&app, "merge-machine", "S-1-5-21-merge").await;
    let credential = identity["credential"].as_str().expect("credential");
    let identity_id = identity["identity_id"].as_str().expect("identity id");

    for (id, provider_id, provider_name, provider_type, input_tokens) in [
        ("merge-a", "provider-a", "我的 DeepSeek", "deepseek", 3),
        ("merge-b", "provider-b", "备用 DeepSeek", "deepseek", 5),
        (
            "merge-gone",
            "provider-gone",
            "已删除实例",
            "removed_provider",
            7,
        ),
    ] {
        seed_activity(
            &context.storage,
            ActivitySeed {
                id,
                identity_id,
                provider_id,
                provider_name,
                provider_type,
                model_requested: "deepseek-v4-flash",
                status: "success",
                input_tokens,
                output_tokens: 1,
            },
        )
        .await;
    }

    let (status, providers): (StatusCode, Vec<ProviderStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/providers?range=today",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].provider_type.as_deref(), Some("deepseek"));
    assert_eq!(providers[0].provider_name.as_deref(), Some("DeepSeek"));
    assert_eq!(providers[0].total_requests, 2);
    assert_eq!(providers[0].input_tokens, 8);
    assert_eq!(providers[1].provider_type, None);
    assert_eq!(providers[1].provider_name.as_deref(), Some("已删除供应商"));
    assert_eq!(providers[1].total_requests, 1);
    assert_eq!(providers[1].input_tokens, 7);
}

#[tokio::test]
async fn model_stats_merge_deleted_models_into_one_bucket() {
    let context = test_context::test_context().await;
    let app = context.app;
    let identity = register(&app, "deleted-model-machine", "S-1-5-21-deleted").await;
    let credential = identity["credential"].as_str().expect("credential");
    let identity_id = identity["identity_id"].as_str().expect("identity id");

    for (id, model, input_tokens) in [
        ("deleted-known-1", "deepseek-v4-flash", 3),
        ("deleted-known-2", "deepseek-v4-flash", 5),
        ("deleted-retired-a", "retired-model-a", 7),
        ("deleted-retired-b", "retired-model-b", 11),
    ] {
        seed_activity(
            &context.storage,
            ActivitySeed {
                id,
                identity_id,
                provider_id: "provider-deleted-model",
                provider_name: "Deleted Model Provider",
                provider_type: "deepseek",
                model_requested: model,
                status: "success",
                input_tokens,
                output_tokens: 1,
            },
        )
        .await;
    }

    let (status, models): (StatusCode, Vec<ModelStatsSummary>) = request_json(
        &app,
        "GET",
        "/api/stats/models?range=today",
        Some(credential),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].model_requested.as_deref(),
        Some("deepseek-v4-flash")
    );
    assert_eq!(
        models[0].model_requested_display_name.as_deref(),
        Some("DeepSeek V4 Flash")
    );
    assert_eq!(models[0].total_requests, 2);
    assert_eq!(models[1].model_requested, None);
    assert_eq!(
        models[1].model_requested_display_name.as_deref(),
        Some("已删除模型")
    );
    assert_eq!(models[1].total_requests, 2);
    assert_eq!(models[1].input_tokens, 18);
    assert_eq!(models[1].output_tokens, 2);
}
