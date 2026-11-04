use super::*;
use crate::domain::activity::ContentEventBody;
use crate::inbound::v1::responses::image_budget::ImageBudgetPolicy;
use crate::inbound::v1::responses::native_input::{budget_input_items, prepare_native_input};

#[test]
fn image_budget_trims_the_exact_items_sent_upstream() {
    let items = vec![
        json!({
            "role": "user",
            "content": [
                { "type": "input_text", "text": "first" },
                { "type": "input_image", "image_url": "a".repeat(512) }
            ]
        }),
        json!({
            "role": "user",
            "content": [
                { "type": "input_text", "text": "second" },
                { "type": "input_image", "image_url": "b".repeat(512) }
            ]
        }),
    ];
    let policy = ImageBudgetPolicy {
        enabled: true,
        keep_last: 1,
        max_bytes: 512,
    };

    let budgeted = budget_input_items(&items, &policy).expect("budgeted items");

    assert_eq!(budgeted[0]["content"][1]["type"], "input_text");
    assert_eq!(budgeted[1]["content"][1]["type"], "input_image");
}

#[test]
fn request_events_follow_the_budgeted_upstream_items() {
    let payload = json!({
        "model": "gpt-4.1",
        "input": [
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "first" },
                    { "type": "input_image", "image_url": "a".repeat(512) }
                ]
            },
            {
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "second" },
                    { "type": "input_image", "image_url": "b".repeat(512) }
                ]
            }
        ]
    });
    let items = payload["input"].as_array().expect("input items").clone();
    let policy = ImageBudgetPolicy {
        enabled: true,
        keep_last: 1,
        max_bytes: 512,
    };

    let (budgeted, events) =
        prepare_native_input(&payload, &items, &policy).expect("prepare native input");

    assert_eq!(budgeted[0]["content"][1]["type"], "input_text");
    let first_content = events
        .iter()
        .find_map(|event| match &event.body {
            ContentEventBody::Message(message) if message.content.is_array() => {
                Some(message.content.clone())
            }
            _ => None,
        })
        .expect("first message content");
    assert_eq!(first_content[0]["type"], "input_text");
    assert_eq!(
        first_content[1], budgeted[0]["content"][1],
        "recorded content must match the upstream budgeted item"
    );
    let last_content = events
        .iter()
        .filter_map(|event| match &event.body {
            ContentEventBody::Message(message) if message.content.is_array() => {
                Some(message.content.clone())
            }
            _ => None,
        })
        .next_back()
        .expect("last message content");
    assert_eq!(last_content[1]["type"], "input_image");
}
