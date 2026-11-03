use super::*;
use crate::inbound::v1::responses::image_budget::ImageBudgetPolicy;
use crate::inbound::v1::responses::native::budget_input_items;

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
