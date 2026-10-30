use serde_json::{json, Value};

use super::{apply, default_max_bytes, ImageBudgetOutcome, ImageBudgetPolicy};

fn enabled_policy(keep_last: usize, max_bytes: usize) -> ImageBudgetPolicy {
    ImageBudgetPolicy {
        enabled: true,
        keep_last,
        max_bytes,
    }
}

/// 图片地址长度固定为 `bytes`，便于按字节推算预算。
fn image_url(bytes: usize, marker: &str) -> String {
    let mut url = format!("data:image/png;base64,{marker}");
    url.push_str(&"a".repeat(bytes.saturating_sub(url.len())));
    url
}

fn payload_with_four_images(bytes: usize) -> Value {
    json!({
        "model": "gpt-4.1",
        "input": [
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "第一张" },
                    { "type": "input_image", "image_url": image_url(bytes, "one") }
                ]
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": [
                    { "type": "input_text", "text": "工具截图" },
                    { "type": "input_image", "image_url": image_url(bytes, "two") }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_image", "image_url": image_url(bytes, "three") }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_image", "image_url": image_url(bytes, "four") }
                ]
            }
        ]
    })
}

fn payload_with_image_sizes(sizes: &[usize]) -> Value {
    json!({
        "model": "gpt-4.1",
        "input": sizes
            .iter()
            .map(|bytes| {
                json!({
                    "role": "user",
                    "content": [
                        { "type": "input_image", "image_url": image_url(*bytes, "x") }
                    ]
                })
            })
            .collect::<Vec<_>>()
    })
}

fn trimmed_payload(outcome: ImageBudgetOutcome) -> Value {
    match outcome {
        ImageBudgetOutcome::Trimmed { payload, .. } => payload,
        other => panic!("expected trimmed outcome, got {other:?}"),
    }
}

fn part<'a>(payload: &'a Value, item: usize, field: &str, index: usize) -> &'a Value {
    &payload["input"][item][field][index]
}

#[test]
fn disabled_policy_never_rewrites() {
    let payload = payload_with_four_images(4096);

    assert_eq!(
        apply(&payload, &ImageBudgetPolicy::default()),
        ImageBudgetOutcome::Disabled
    );
}

#[test]
fn payload_within_budget_is_untouched() {
    let payload = payload_with_four_images(512);
    let policy = enabled_policy(10, 4 * 512);

    assert_eq!(
        apply(&payload, &policy),
        ImageBudgetOutcome::WithinBudget(super::ImageBudgetReport {
            total_bytes: 4 * 512,
            remaining_bytes: 4 * 512,
            dropped_images: 0,
        })
    );
}

#[test]
fn drops_the_oldest_images_until_the_budget_fits() {
    let payload = payload_with_four_images(512);
    let policy = enabled_policy(2, 3 * 512 - 1);

    let outcome = apply(&payload, &policy);
    let (pruned, report) = match outcome {
        ImageBudgetOutcome::Trimmed { payload, report } => (payload, report),
        other => panic!("expected trimmed outcome, got {other:?}"),
    };

    assert_eq!(report.total_bytes, 4 * 512);
    assert_eq!(report.remaining_bytes, 2 * 512);
    assert_eq!(report.dropped_images, 2);
    assert_eq!(pruned["input"].as_array().map(Vec::len), Some(4));
    assert_eq!(part(&pruned, 0, "content", 1)["type"], "input_text");
    assert_eq!(part(&pruned, 1, "output", 1)["type"], "input_text");
    assert_eq!(part(&pruned, 2, "content", 0)["type"], "input_image");
    assert_eq!(part(&pruned, 3, "content", 0)["type"], "input_image");
    assert_eq!(part(&pruned, 1, "output", 0)["text"], "工具截图");
    assert_eq!(part(&pruned, 0, "content", 0)["text"], "第一张");
    assert_eq!(
        part(&pruned, 0, "content", 1)["text"],
        "历史图片已被服务端图片预算省略（仅保留最近 2 张图片）"
    );
}

#[test]
fn retention_window_wins_when_the_budget_cannot_be_met() {
    let payload = payload_with_four_images(512);
    let policy = enabled_policy(3, 1024);

    assert_eq!(
        apply(&payload, &policy),
        ImageBudgetOutcome::CannotMeetBudget(super::ImageBudgetReport {
            total_bytes: 4 * 512,
            remaining_bytes: 3 * 512,
            dropped_images: 1,
        })
    );
}

#[test]
fn images_inside_the_retention_window_are_not_dropped() {
    let payload = payload_with_four_images(4096);
    let policy = enabled_policy(4, 1024);

    assert_eq!(
        apply(&payload, &policy),
        ImageBudgetOutcome::CannotMeetBudget(super::ImageBudgetReport {
            total_bytes: 4 * 4096,
            remaining_bytes: 4 * 4096,
            dropped_images: 0,
        })
    );
}

#[test]
fn placeholder_text_is_stable_for_the_same_payload() {
    let payload = payload_with_four_images(512);
    let policy = enabled_policy(1, 1024);

    let first = trimmed_payload(apply(&payload, &policy));
    let second = trimmed_payload(apply(&payload, &policy));

    assert_eq!(
        serde_json::to_vec(&first).expect("first body"),
        serde_json::to_vec(&second).expect("second body")
    );
}

#[test]
fn file_reference_images_count_toward_the_window() {
    let payload = json!({
        "model": "gpt-4.1",
        "input": [
            {
                "role": "user",
                "content": [
                    { "type": "input_image", "file_id": "file_old" },
                    { "type": "input_image", "image_url": image_url(4096, "fresh") }
                ]
            }
        ]
    });
    let policy = enabled_policy(1, 1024);

    assert_eq!(
        apply(&payload, &policy),
        ImageBudgetOutcome::CannotMeetBudget(super::ImageBudgetReport {
            total_bytes: 4096,
            remaining_bytes: 4096,
            dropped_images: 1,
        })
    );
}

#[test]
fn counts_nested_image_url_objects() {
    let payload = json!({
        "model": "gpt-4.1",
        "input": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_image",
                        "image_url": { "url": image_url(512, "old") }
                    },
                    {
                        "type": "input_image",
                        "image_url": image_url(512, "new")
                    }
                ]
            }
        ]
    });
    let policy = enabled_policy(1, 512);

    let pruned = trimmed_payload(apply(&payload, &policy));

    assert_eq!(pruned["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(pruned["input"][0]["content"][1]["type"], "input_image");
}

#[test]
fn incident_sized_payload_cannot_meet_the_default_retention_window() {
    // 2026-10-22 会话里 23 张图片的 image_url 字节数。
    let sizes = [
        759_490, 115_594, 67_646, 1_131_846, 487_690, 89_330, 87_722, 567_730, 1_065_410, 81_658,
        20_918, 81_658, 410_682, 93_094, 361_870, 1_404_354, 895_466, 1_569_466, 1_760_574,
        579_090, 1_445_726, 1_730_006, 739_286,
    ];
    let payload = payload_with_image_sizes(&sizes);
    let max_bytes = 8 * 1024 * 1024;

    assert_eq!(
        apply(&payload, &enabled_policy(10, max_bytes)),
        ImageBudgetOutcome::CannotMeetBudget(super::ImageBudgetReport {
            total_bytes: 15_546_306,
            remaining_bytes: 10_578_932,
            dropped_images: 13,
        })
    );

    let outcome = apply(&payload, &enabled_policy(3, max_bytes));
    let report = match outcome {
        ImageBudgetOutcome::Trimmed { report, .. } => report,
        other => panic!("expected trimmed outcome, got {other:?}"),
    };
    assert_eq!(report.total_bytes, 15_546_306);
    assert_eq!(report.dropped_images, 17);
    assert!(report.remaining_bytes <= max_bytes);
}

#[test]
fn policy_reads_configured_values() {
    let policy =
        ImageBudgetPolicy::from_values(Some("true"), Some("3"), Some("1024")).expect("policy");

    assert_eq!(policy, enabled_policy(3, 1024));
    assert!(ImageBudgetPolicy::from_values(Some("yes"), None, None).is_err());
    assert!(ImageBudgetPolicy::from_values(None, Some("0"), None).is_err());
}

#[test]
fn policy_defaults_to_disabled() {
    let policy = ImageBudgetPolicy::from_values(None, None, None).expect("policy");

    assert_eq!(policy, ImageBudgetPolicy::default());
    assert!(!policy.enabled);
    assert_eq!(policy.keep_last, 10);
    assert_eq!(policy.max_bytes, default_max_bytes());
}
