use prelay_protocol::{ProviderQuota, QuotaError, QuotaErrorCode, QuotaWindow};
use serde_json::Value;

use crate::domain::catalog::CatalogQuota;

use super::{fetch_quota_body, quota_error};

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Result<ProviderQuota, QuotaError> {
    let body = fetch_quota_body(client, base_url, quota, api_key, "Kimi").await?;
    parse(&body)
}

fn parse(body: &str) -> Result<ProviderQuota, QuotaError> {
    let root = serde_json::from_str::<Value>(body).map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("Kimi 额度响应无法解析: {error}"),
        )
    })?;
    let mut windows = Vec::new();
    if let Some(limits) = root.get("limits").and_then(Value::as_array) {
        for limit in limits {
            let window = limit.get("window");
            let detail = limit.get("detail").unwrap_or(limit);
            let span_secs = window.and_then(window_span);
            if let Some(mut item) = read_window(detail, span_secs) {
                item.label = label_for(span_secs.unwrap_or_default());
                windows.push(item);
            }
        }
    }
    if let Some(usage) = root.get("usage") {
        if let Some(mut item) = read_window(usage, Some(7 * 24 * 60 * 60)) {
            item.label = "7 天".to_string();
            item.span_secs = Some(7 * 24 * 60 * 60);
            windows.push(item);
        }
    }
    if windows.is_empty() {
        return Err(quota_error(
            QuotaErrorCode::InvalidResponse,
            "Kimi 额度响应没有窗口",
        ));
    }
    Ok(ProviderQuota {
        plan: None,
        windows,
        balance: None,
        error: None,
        read_at: None,
        stale: false,
    })
}

fn window_span(window: &Value) -> Option<i64> {
    let duration = number(window.get("duration"))?;
    let unit = window.get("timeUnit")?.as_str().unwrap_or_default();
    let seconds = if unit.contains("MINUTE") {
        duration * 60.0
    } else if unit.contains("HOUR") {
        duration * 3600.0
    } else if unit.contains("DAY") {
        duration * 86400.0
    } else {
        return None;
    };
    (seconds > 0.0).then_some(seconds as i64)
}

fn read_window(detail: &Value, span_secs: Option<i64>) -> Option<QuotaWindow> {
    let limit = number(detail.get("limit"))?;
    if limit <= 0.0 {
        return None;
    }
    let used = number(detail.get("used"))
        .or_else(|| number(detail.get("remaining")).map(|remaining| limit - remaining))?;
    let resets_at = ["resetTime", "resetAt", "reset_at", "reset_time"]
        .iter()
        .find_map(|key| detail.get(*key).and_then(Value::as_str))
        .filter(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok())
        .map(str::to_string);
    Some(QuotaWindow {
        label: String::new(),
        span_secs,
        used_percent: (used / limit * 100.0).clamp(0.0, 100.0),
        resets_at,
        display: Some(format!("{used:.0} / {limit:.0}")),
        amount: Some(format!("{:.0}", (limit - used).max(0.0))),
        limit: Some(format!("{limit:.0}")),
        unit: Some("requests".to_string()),
        aside: false,
        pool: None,
    })
}

fn label_for(span_secs: i64) -> String {
    match span_secs {
        value if value >= 86_400 && value % 86_400 == 0 => {
            format!("{} 天", value / 86_400)
        }
        value if value >= 3_600 && value % 3_600 == 0 => {
            format!("{} 小时", value / 3_600)
        }
        value if value > 0 => format!("{} 分钟", value / 60),
        _ => "额度".to_string(),
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_the_five_hour_and_weekly_windows() {
        let quota = parse(
            r#"{
                "usage": {"limit":"100","used":"12","resetTime":"2026-09-30T05:24:18.44Z"},
                "limits": [{
                    "window": {"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
                    "detail": {"limit":"100","remaining":"88","resetTime":"2026-09-30T05:24:18.44Z"}
                }]
            }"#,
        )
        .expect("Kimi quota parses");
        assert_eq!(quota.windows.len(), 2);
        assert_eq!(quota.windows[0].label, "5 小时");
        assert_eq!(quota.windows[0].used_percent, 12.0);
        assert_eq!(quota.windows[1].label, "7 天");
        assert_eq!(quota.windows[1].used_percent, 12.0);
    }
}
