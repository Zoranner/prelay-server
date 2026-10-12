//! 服务端图片预算：原生 Responses 请求的历史图片总量超出预算时，丢弃最老的图片。
//!
//! 只改写上行的请求体，不改客户端会话；未超预算时原样透传，尽量不打扰上游前缀缓存。

use std::sync::OnceLock;

use serde_json::{json, Value};

/// 未配置时保留的最近图片数量。
const DEFAULT_KEEP_LAST: usize = 10;
/// 未配置时的图片字节预算：协议入口上限的四分之一。
///
/// 入口上限表示客户端允许产生多大的会话，预算按固定比例跟随它；上游真实能收多少
/// 是硬约束，需要用 `IMAGE_BUDGET_MAX_BYTES` 显式对齐（详见 `deploy/.env.example`）。
fn default_max_bytes() -> usize {
    crate::protocol_body_limit::limit() / 4
}

static IMAGE_BUDGET: OnceLock<ImageBudgetPolicy> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageBudgetPolicy {
    pub enabled: bool,
    /// 无论预算是否达标都保留的最近图片数量，这一窗口内的图片不会被丢弃。
    pub keep_last: usize,
    /// 单次请求里内联图片地址的总字节预算。
    pub max_bytes: usize,
}

impl Default for ImageBudgetPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_last: DEFAULT_KEEP_LAST,
            max_bytes: default_max_bytes(),
        }
    }
}

impl ImageBudgetPolicy {
    pub fn from_environment() -> Result<Self, String> {
        Self::from_values(
            std::env::var("IMAGE_BUDGET_ENABLED").ok().as_deref(),
            std::env::var("IMAGE_BUDGET_KEEP_LAST").ok().as_deref(),
            std::env::var("IMAGE_BUDGET_MAX_BYTES").ok().as_deref(),
        )
    }

    pub fn from_values(
        enabled: Option<&str>,
        keep_last: Option<&str>,
        max_bytes: Option<&str>,
    ) -> Result<Self, String> {
        let enabled = parse_flag("IMAGE_BUDGET_ENABLED", enabled)?.unwrap_or(false);
        let keep_last = parse_positive("IMAGE_BUDGET_KEEP_LAST", keep_last)?
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_KEEP_LAST);
        let max_bytes = parse_positive("IMAGE_BUDGET_MAX_BYTES", max_bytes)?
            .map(|value| value as usize)
            .unwrap_or_else(default_max_bytes);

        Ok(Self {
            enabled,
            keep_last,
            max_bytes,
        })
    }
}

pub fn initialize_from_environment() -> Result<&'static ImageBudgetPolicy, String> {
    let configured_policy = ImageBudgetPolicy::from_environment()?;
    IMAGE_BUDGET
        .set(configured_policy)
        .map_err(|_| "image budget policy was already initialized".to_owned())?;
    Ok(policy())
}

pub fn policy() -> &'static ImageBudgetPolicy {
    IMAGE_BUDGET.get_or_init(ImageBudgetPolicy::default)
}

/// 按预算丢弃最老的历史图片；没有需要改写的内容时返回 `None`，调用方继续用原请求体。
///
/// 丢弃顺序是从最老的图片开始，直到总量落到预算内；如果丢到只剩最近 `keep_last` 张仍然
/// 超预算，就停在保留窗口上，不动用户刚贴的图片。
pub fn apply(payload: &Value, policy: &ImageBudgetPolicy) -> Option<Value> {
    if !policy.enabled {
        return None;
    }

    let mut images = Vec::new();
    collect_images(payload, &mut images);
    let total_bytes: usize = images.iter().map(|image| image.bytes).sum();
    if total_bytes <= policy.max_bytes {
        return None;
    }

    let droppable = images.len().saturating_sub(policy.keep_last);
    if droppable == 0 {
        tracing::warn!(
            total_bytes,
            max_bytes = policy.max_bytes,
            keep_last = policy.keep_last,
            "image budget exceeded but every image is inside the retention window"
        );
        return None;
    }

    let mut pruned = payload.clone();
    let mut remaining_bytes = total_bytes;
    let mut dropped = 0;
    for image in images.iter().take(droppable) {
        if remaining_bytes <= policy.max_bytes {
            break;
        }
        if !replace_with_placeholder(&mut pruned, image, policy.keep_last) {
            continue;
        }
        // 占位文本自身也占字节，这里只按图片地址折算，误差远小于 MiB 级预算。
        remaining_bytes = remaining_bytes.saturating_sub(image.bytes);
        dropped += 1;
    }

    if dropped == 0 {
        return None;
    }

    tracing::info!(
        dropped,
        remaining_bytes,
        max_bytes = policy.max_bytes,
        "trimmed historical images for the upstream image budget"
    );
    Some(pruned)
}

struct ImageSlot {
    item: usize,
    field: &'static str,
    part: usize,
    bytes: usize,
}

fn collect_images(payload: &Value, images: &mut Vec<ImageSlot>) {
    let Some(items) = payload.get("input").and_then(Value::as_array) else {
        return;
    };
    for (item_index, item) in items.iter().enumerate() {
        let field = if item.get("type").and_then(Value::as_str) == Some("function_call_output") {
            "output"
        } else {
            "content"
        };
        let Some(parts) = item.get(field).and_then(Value::as_array) else {
            continue;
        };
        for (part_index, part) in parts.iter().enumerate() {
            if part.get("type").and_then(Value::as_str) != Some("input_image") {
                continue;
            }
            images.push(ImageSlot {
                item: item_index,
                field,
                part: part_index,
                bytes: image_body_bytes(part),
            });
        }
    }
}

/// 图片在请求体里占的字节数：内联图片取 `image_url` 的长度，文件引用不占请求体，
/// 但仍参与保留窗口计数。
fn image_body_bytes(part: &Value) -> usize {
    part.get("image_url")
        .and_then(Value::as_str)
        .map_or(0, str::len)
}

fn replace_with_placeholder(payload: &mut Value, slot: &ImageSlot, keep_last: usize) -> bool {
    let Some(part) = payload
        .get_mut("input")
        .and_then(|input| input.get_mut(slot.item))
        .and_then(|item| item.get_mut(slot.field))
        .and_then(|parts| parts.get_mut(slot.part))
    else {
        return false;
    };
    *part = json!({
        "type": "input_text",
        "text": placeholder_text(keep_last),
    });
    true
}

/// 占位文案必须确定：同一会话状态重复请求要生成逐字节相同的请求体，否则上游缓存必失效。
fn placeholder_text(keep_last: usize) -> String {
    format!("历史图片已被服务端图片预算省略（仅保留最近 {keep_last} 张图片）")
}

fn parse_positive(name: &str, value: Option<&str>) -> Result<Option<u64>, String> {
    match value {
        Some(value) => match value.parse::<u64>() {
            Ok(value) if value > 0 => Ok(Some(value)),
            _ => Err(format!("{name} must be a positive integer")),
        },
        None => Ok(None),
    }
}

fn parse_flag(name: &str, value: Option<&str>) -> Result<Option<bool>, String> {
    match value.map(str::trim) {
        None => Ok(None),
        Some(value) if value.eq_ignore_ascii_case("true") || value == "1" => Ok(Some(true)),
        Some(value) if value.eq_ignore_ascii_case("false") || value == "0" => Ok(Some(false)),
        Some(_) => Err(format!("{name} must be true or false")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{apply, default_max_bytes, ImageBudgetPolicy};

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

    fn part<'a>(payload: &'a Value, item: usize, field: &str, index: usize) -> &'a Value {
        &payload["input"][item][field][index]
    }

    #[test]
    fn disabled_policy_never_rewrites() {
        let payload = payload_with_four_images(4096);

        assert!(apply(&payload, &ImageBudgetPolicy::default()).is_none());
    }

    #[test]
    fn payload_within_budget_is_untouched() {
        let payload = payload_with_four_images(512);
        let policy = enabled_policy(10, 4 * 512);

        assert!(apply(&payload, &policy).is_none());
    }

    #[test]
    fn drops_the_oldest_images_until_the_budget_fits() {
        let payload = payload_with_four_images(512);
        let policy = enabled_policy(2, 3 * 512 - 1);

        let pruned = apply(&payload, &policy).expect("payload is pruned");

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

        let pruned = apply(&payload, &policy).expect("payload is pruned");

        assert_eq!(part(&pruned, 0, "content", 1)["type"], "input_text");
        assert_eq!(part(&pruned, 1, "output", 1)["type"], "input_image");
        assert_eq!(part(&pruned, 2, "content", 0)["type"], "input_image");
        assert_eq!(part(&pruned, 3, "content", 0)["type"], "input_image");
    }

    #[test]
    fn images_inside_the_retention_window_are_not_dropped() {
        let payload = payload_with_four_images(4096);
        let policy = enabled_policy(4, 1024);

        assert!(apply(&payload, &policy).is_none());
    }

    #[test]
    fn placeholder_text_is_stable_for_the_same_payload() {
        let payload = payload_with_four_images(512);
        let policy = enabled_policy(1, 1024);

        let first = apply(&payload, &policy).expect("payload is pruned");
        let second = apply(&payload, &policy).expect("payload is pruned");

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

        let pruned = apply(&payload, &policy).expect("payload is pruned");

        assert_eq!(part(&pruned, 0, "content", 0)["type"], "input_text");
        assert_eq!(part(&pruned, 0, "content", 1)["type"], "input_image");
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
}
