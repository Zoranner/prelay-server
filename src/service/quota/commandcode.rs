use std::time::Duration;

use prelay_protocol::{
    ProviderQuota, QuotaBalance, QuotaBalancePart, QuotaError, QuotaErrorCode, QuotaWindow,
};
use serde_json::Value;

use crate::domain::catalog::CatalogQuota;

use super::{fetch_quota_body, fetch_quota_url_body, quota_error, resolve_quota_url};

const PLAN_MONTHLY_CAP: &[(&str, f64)] = &[
    ("max-20x", 200.0),
    ("max-10x", 100.0),
    ("individual-ultra", 300.0),
    ("individual-goat", 70.0),
    ("individual-pro", 80.0),
    ("individual-go", 10.0),
    ("individual-max", 150.0),
    ("teams-pro", 40.0),
    ("team-pro", 40.0),
];

// 订阅信息只用于月度窗口；查询失败或超时不能拖垮 5 小时/周读数。
const SUBSCRIPTION_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct Subscription {
    plan_id: Option<String>,
    current_period_end: Option<String>,
}

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Result<ProviderQuota, QuotaError> {
    let (body, subscription) = tokio::join!(
        fetch_quota_body(client, base_url, quota, api_key, "Command Code"),
        fetch_subscription(client, base_url, quota, api_key),
    );
    parse(&body?, subscription.as_ref())
}

async fn fetch_subscription(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Option<Subscription> {
    tokio::time::timeout(SUBSCRIPTION_TIMEOUT, async {
        let url = resolve_quota_url(base_url, "/alpha/billing/subscriptions");
        let body = fetch_quota_url_body(client, &url, quota, api_key, "Command Code")
            .await
            .ok()?;
        parse_subscription(&body)
    })
    .await
    .ok()
    .flatten()
}

fn parse_subscription(body: &str) -> Option<Subscription> {
    let root = serde_json::from_str::<Value>(body).ok()?;
    let data = root.get("data")?;
    Some(Subscription {
        plan_id: data
            .get("planId")
            .and_then(Value::as_str)
            .map(str::to_string),
        current_period_end: timestamp(data.get("currentPeriodEnd")),
    })
}

fn parse(body: &str, subscription: Option<&Subscription>) -> Result<ProviderQuota, QuotaError> {
    let root = serde_json::from_str::<Value>(body).map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("Command Code 额度响应无法解析: {error}"),
        )
    })?;
    let five_hour_cap = root
        .get("windowLimits")
        .and_then(|limits| limits.get("fiveHour"))
        .and_then(|window| number(window.get("cap")));
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
        .map(str::to_string)
        .or_else(|| {
            subscription
                .and_then(|value| value.plan_id.as_deref())
                .map(str::to_string)
        });
    let monthly_remaining = credits.and_then(|value| number(value.get("monthlyCredits")));
    if let (Some(remaining), Some(cap)) = (
        monthly_remaining,
        monthly_cap_for_plan(plan.as_deref(), five_hour_cap),
    ) {
        let remaining = remaining.clamp(0.0, cap);
        let used = (cap - remaining).max(0.0);
        windows.push(QuotaWindow {
            label: "月".to_string(),
            span_secs: None,
            used_percent: (used / cap * 100.0).clamp(0.0, 100.0),
            resets_at: subscription.and_then(|value| value.current_period_end.clone()),
            display: Some(format!("${used:.2} / ${cap:.2}")),
            amount: Some(format!("{remaining:.2}")),
            limit: Some(format!("{cap:.2}")),
            unit: Some("USD".to_string()),
            aside: false,
            pool: None,
        });
    }
    // monthlyCredits 是月度套餐剩余，已经由月度窗口表达；这里的余额只表示
    // purchased/free 这类额外额度，避免把月度剩余重复算成额外余额。
    let left = ["purchasedCredits", "freeCredits"]
        .iter()
        .filter_map(|key| credits.and_then(|value| number(value.get(*key))))
        .sum::<f64>();
    let balance = credits.map(|_| QuotaBalance {
        text: format!("${left:.2}"),
        parts: vec![QuotaBalancePart {
            currency: "USD".to_string(),
            amount: format!("{left:.2}"),
        }],
        available: left > 0.0,
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

fn monthly_cap_for_plan(plan: Option<&str>, five_hour_cap: Option<f64>) -> Option<f64> {
    let plan = plan?.trim().to_ascii_lowercase();
    // Go/GOAT/Pro 的 5 小时 cap 是月度额度的五分之一，按实时 cap 推导可
    // 跟随账号实际使用的模型档位；Max/Team 比例不同，使用固定档位表。
    if ["individual-goat", "individual-go", "individual-pro"]
        .iter()
        .any(|name| plan.contains(name))
    {
        if let Some(cap) = five_hour_cap.filter(|cap| *cap > 0.0) {
            return Some(cap * 5.0);
        }
    }
    PLAN_MONTHLY_CAP
        .iter()
        .find_map(|(name, cap)| plan.contains(name).then_some(*cap))
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
    use super::{parse, parse_subscription, resolve_quota_url};

    #[test]
    fn resolves_the_subscription_endpoint_on_the_api_origin() {
        assert_eq!(
            resolve_quota_url(
                "https://api.commandcode.ai/provider/v1",
                "/alpha/billing/subscriptions"
            ),
            "https://api.commandcode.ai/alpha/billing/subscriptions"
        );
    }

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
            Some(&super::Subscription {
                plan_id: Some("individual-go-v1".to_string()),
                current_period_end: Some("2026-11-21T00:00:00+08:00".to_string()),
            }),
        )
        .expect("Command Code quota parses");
        assert_eq!(quota.plan.as_deref(), Some("individual-go-v1"));
        assert_eq!(quota.windows.len(), 3);
        assert_eq!(quota.windows[0].used_percent, 50.0);
        assert_eq!(quota.windows[1].used_percent, 60.0);
        assert_eq!(quota.windows[2].label, "月");
        assert_eq!(quota.windows[2].limit.as_deref(), Some("10.00"));
        assert_eq!(quota.windows[2].amount.as_deref(), Some("10.00"));
        assert_eq!(quota.windows[2].used_percent, 0.0);
        assert_eq!(
            quota.windows[2].resets_at.as_deref(),
            Some("2026-11-21T00:00:00+08:00")
        );
        let balance = quota.balance.expect("balance");
        assert_eq!(balance.text, "$2.00");
    }

    #[test]
    fn monthly_window_uses_the_subscription_period_and_plan_cap() {
        let subscription = parse_subscription(
            r#"{"data":{"planId":"individual-goat","currentPeriodEnd":"2026-11-21T00:00:00+08:00"}}"#,
        )
        .expect("subscription parses");
        let quota = parse(
            r#"{
                "credits": {"monthlyCredits":0.06,"purchasedCredits":0,"freeCredits":0},
                "windowLimits": {
                    "fiveHour": {"used":0,"cap":14},
                    "weekly": {"used":0,"cap":35}
                }
            }"#,
            Some(&subscription),
        )
        .expect("Command Code quota parses");

        assert_eq!(quota.windows.len(), 3);
        let monthly = &quota.windows[2];
        assert_eq!(monthly.label, "月");
        assert_eq!(monthly.limit.as_deref(), Some("70.00"));
        assert_eq!(monthly.amount.as_deref(), Some("0.06"));
        assert!((monthly.used_percent - 99.914_285_714_285_7).abs() < 0.001);
        assert_eq!(
            monthly.resets_at.as_deref(),
            Some("2026-11-21T00:00:00+08:00")
        );
        assert_eq!(
            quota.balance.as_ref().map(|balance| balance.text.as_str()),
            Some("$0.00")
        );
    }

    #[test]
    fn monthly_cap_follows_the_live_five_hour_cap_for_goat_models() {
        let subscription = parse_subscription(
            r#"{"data":{"planId":"individual-goat","currentPeriodEnd":"2026-11-21T00:00:00+08:00"}}"#,
        )
        .expect("subscription parses");
        let quota = parse(
            r#"{
                "credits": {"monthlyCredits":50,"purchasedCredits":0,"freeCredits":0},
                "windowLimits": {
                    "fiveHour": {"used":0,"cap":20},
                    "weekly": {"used":0,"cap":50}
                }
            }"#,
            Some(&subscription),
        )
        .expect("Command Code quota parses");

        let monthly = &quota.windows[2];
        assert_eq!(monthly.limit.as_deref(), Some("100.00"));
        assert_eq!(monthly.used_percent, 50.0);
    }

    #[test]
    fn skips_monthly_window_when_the_plan_is_unknown() {
        let quota = parse(
            r#"{
                "credits": {"planId":"unknown-plan","monthlyCredits":5},
                "windowLimits": {"fiveHour":{"used":1,"cap":2}}
            }"#,
            None,
        )
        .expect("Command Code quota parses");

        assert_eq!(quota.windows.len(), 1);
        assert_eq!(quota.windows[0].label, "5 小时");
        assert_eq!(quota.plan.as_deref(), Some("unknown-plan"));
    }
}
