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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageBudgetReport {
    pub total_bytes: usize,
    pub remaining_bytes: usize,
    pub dropped_images: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageBudgetOutcome {
    Disabled,
    WithinBudget(ImageBudgetReport),
    Trimmed {
        payload: Value,
        report: ImageBudgetReport,
    },
    CannotMeetBudget(ImageBudgetReport),
}

/// 按预算丢弃最老的历史图片。
///
/// 丢弃顺序是从最老的图片开始，直到总量落到预算内；如果丢到只剩最近 `keep_last` 张仍然
/// 超预算，返回 `CannotMeetBudget`，调用方必须拒绝请求，不能把超预算的请求体继续发给上游。
/// `keep_last` 是硬保留窗口；预算与保留窗口冲突时，宁可失败也不静默改动用户刚贴的图片。
pub fn apply(payload: &Value, policy: &ImageBudgetPolicy) -> ImageBudgetOutcome {
    if !policy.enabled {
        return ImageBudgetOutcome::Disabled;
    }

    let images = collect_images(payload);
    let total_bytes: usize = images.iter().map(|image| image.bytes).sum();
    let report = |remaining_bytes, dropped_images| ImageBudgetReport {
        total_bytes,
        remaining_bytes,
        dropped_images,
    };
    if total_bytes <= policy.max_bytes {
        return ImageBudgetOutcome::WithinBudget(report(total_bytes, 0));
    }

    let droppable = images.len().saturating_sub(policy.keep_last);
    if droppable == 0 {
        return ImageBudgetOutcome::CannotMeetBudget(report(total_bytes, 0));
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

    if dropped == 0 || remaining_bytes > policy.max_bytes {
        return ImageBudgetOutcome::CannotMeetBudget(report(remaining_bytes, dropped));
    }

    ImageBudgetOutcome::Trimmed {
        payload: pruned,
        report: report(remaining_bytes, dropped),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PathSegment {
    Key(String),
    Index(usize),
}

struct ImageSlot {
    path: Vec<PathSegment>,
    bytes: usize,
}

fn collect_images(payload: &Value) -> Vec<ImageSlot> {
    let mut images = Vec::new();
    if let Some(input) = payload.get("input") {
        collect_images_in_value(input, &mut Vec::new(), &mut images);
    }
    images
}

fn collect_images_in_value(
    value: &Value,
    path: &mut Vec<PathSegment>,
    images: &mut Vec<ImageSlot>,
) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("input_image") {
                images.push(ImageSlot {
                    path: path.clone(),
                    bytes: image_body_bytes(value),
                });
                return;
            }
            for (key, child) in object {
                path.push(PathSegment::Key(key.clone()));
                collect_images_in_value(child, path, images);
                path.pop();
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                path.push(PathSegment::Index(index));
                collect_images_in_value(child, path, images);
                path.pop();
            }
        }
        _ => {}
    }
}

/// 图片在请求体里占的字节数：内联图片取 `image_url` 的长度，文件引用不占请求体，
/// 但仍参与保留窗口计数。
fn image_body_bytes(part: &Value) -> usize {
    match part.get("image_url") {
        Some(Value::String(url)) => url.len(),
        Some(Value::Object(object)) => object
            .get("url")
            .and_then(Value::as_str)
            .map_or(0, str::len),
        _ => 0,
    }
}

fn replace_with_placeholder(payload: &mut Value, slot: &ImageSlot, keep_last: usize) -> bool {
    let Some(mut part) = payload.get_mut("input") else {
        return false;
    };
    for segment in &slot.path {
        part = match segment {
            PathSegment::Key(key) => match part.get_mut(key) {
                Some(value) => value,
                None => return false,
            },
            PathSegment::Index(index) => match part.get_mut(*index) {
                Some(value) => value,
                None => return false,
            },
        };
    }
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
#[path = "image_budget/tests.rs"]
mod tests;
