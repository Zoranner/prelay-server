use prelay_protocol::{
    ProviderQuota, QuotaBalance, QuotaBalancePart, QuotaError, QuotaErrorCode, QuotaWindow,
};
use serde_json::Value;

use crate::domain::catalog::CatalogQuota;

use super::{fetch_quota_body, quota_error};

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Result<ProviderQuota, QuotaError> {
    let body = fetch_quota_body(client, base_url, quota, api_key, "Command Code").await?;
    parse(&body)
}

fn parse(body: &str) -> Result<ProviderQuota, QuotaError> {
    let root = serde_json::from_str::<Value>(body).map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("Command Code 额度响应无法解析: {error}"),
        )
    })?;
    let mut windows = Vec::new();
    for (key, label, span_secs) in [
        ("fiveHour", "5 小时", 5 * 60 * 60),
        ("weekly", "7 天", 7 * 24 * 60 * 60),
    ] {
        let Some(window) = root.get("windowLimits").and_then(|limits| limits.get(key)) else {
            continue;
        };
        let (Some(used), Some(cap)) = (number(window.get("used")), number(window.get("cap")))
        else {
            continue;
        };
        if cap <= 0.0 {
            continue;
        }
        windows.push(QuotaWindow {
            label: label.to_string(),
            span_secs: Some(span_secs),
            used_percent: (used / cap * 100.0).clamp(0.0, 100.0),
            resets_at: timestamp(window.get("resetAt")),
            display: Some(format!("${used:.2} / ${cap:.2}")),
            amount: Some(format!("{:.2}", (cap - used).max(0.0))),
            limit: Some(format!("{cap:.2}")),
            unit: Some("USD".to_string()),
            aside: false,
            pool: None,
        });
    }
    let credits = root.get("credits");
    let plan = credits
        .and_then(|value| value.get("planId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let left = ["monthlyCredits", "purchasedCredits", "freeCredits"]
        .iter()
        .filter_map(|key| credits.and_then(|value| number(value.get(*key))))
        .sum::<f64>();
    let balance = credits.and_then(|value| {
        value.get("monthlyCredits").map(|_| QuotaBalance {
            text: format!("${left:.2}"),
            parts: vec![QuotaBalancePart {
                currency: "USD".to_string(),
                amount: format!("{left:.2}"),
            }],
            available: left > 0.0,
        })
    });
    if windows.is_empty() && balance.is_none() {
        return Err(quota_error(
            QuotaErrorCode::InvalidResponse,
            "Command Code 额度响应没有窗口或余额",
        ));
    }
    Ok(ProviderQuota {
        plan,
        windows,
        balance,
        error: None,
        read_at: None,
        stale: false,
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn timestamp(value: Option<&Value>) -> Option<String> {
    if let Some(text) = value.and_then(Value::as_str) {
        if chrono::DateTime::parse_from_rfc3339(text).is_ok() {
            return Some(text.to_string());
        }
    }
    let value = number(value)?;
    if value <= 0.0 {
        return None;
    }
    let millis = if value < 1e12 { value * 1000.0 } else { value };
    chrono::DateTime::from_timestamp_millis(millis as i64).map(|time| time.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_windows_and_the_remaining_credits() {
        let quota = parse(
            r#"{
                "credits": {"planId":"individual-go-v1","monthlyCredits":10,"purchasedCredits":2,"freeCredits":0},
                "windowLimits": {
                    "fiveHour": {"used":1,"cap":2,"resetAt":1785182400000},
                    "weekly": {"used":3,"cap":5,"resetAt":1785715200000}
                }
            }"#,
        )
        .expect("Command Code quota parses");
        assert_eq!(quota.plan.as_deref(), Some("individual-go-v1"));
        assert_eq!(quota.windows.len(), 2);
        assert_eq!(quota.windows[0].used_percent, 50.0);
        assert_eq!(quota.windows[1].used_percent, 60.0);
        let balance = quota.balance.expect("balance");
        assert_eq!(balance.text, "$12.00");
    }
}
