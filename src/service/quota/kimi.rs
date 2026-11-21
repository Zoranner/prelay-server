use prelay_protocol::{
    ProviderQuota, QuotaBalance, QuotaBalancePart, QuotaError, QuotaErrorCode, QuotaWindow,
    QuotaWindowKind,
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
            let span_secs = limit.get("window").and_then(window_span);
            let detail = limit.get("detail").unwrap_or(limit);
            if let Some((used_percent, resets_at)) = read_counted_window(detail) {
                windows.push(QuotaWindow {
                    kind: kind_for_span(span_secs),
                    span_secs,
                    used_percent,
                    resets_at,
                    remaining: None,
                    limit: None,
                    unit: None,
                    display: None,
                });
            }
        }
    }
    if let Some(usage) = root.get("usage") {
        if let Some((used_percent, resets_at)) = read_counted_window(usage) {
            windows.push(QuotaWindow {
                kind: QuotaWindowKind::Week,
                span_secs: Some(WEEK_SECS),
                used_percent,
                resets_at,
                remaining: None,
                limit: None,
                unit: None,
                display: None,
            });
        }
    }
    let shares = read_usage_windows(root.get("usages"));
    if !shares.is_empty() {
        let keys = shares
            .iter()
            .map(|window| slot_key(window.kind, window.span_secs))
            .collect::<Vec<_>>();
        windows.retain(|window| !keys.contains(&slot_key(window.kind, window.span_secs)));
        windows.extend(shares);
        windows.sort_by_key(window_order);
    }
    let balance = read_booster(&root);
    if windows.is_empty() && balance.is_none() {
        return Err(quota_error(
            QuotaErrorCode::InvalidResponse,
            "Kimi 额度响应没有窗口或余额",
        ));
    }
    Ok(ProviderQuota {
        plan: None,
        windows,
        balance,
        error: None,
        read_at: None,
        stale: false,
    })
}

const WEEK_SECS: i64 = 7 * 24 * 60 * 60;

fn window_span(window: &Value) -> Option<i64> {
    let duration = number(window.get("duration"))?;
    let unit = window.get("timeUnit")?.as_str().unwrap_or_default();
    let seconds = if unit.contains("MINUTE") {
        duration * 60.0
    } else if unit.contains("HOUR") {
        duration * 3600.0
    } else if unit.contains("DAY") {
        duration * 86400.0
    } else if unit.contains("WEEK") {
        duration * WEEK_SECS as f64
    } else if unit.contains("MONTH") {
        duration * 30.0 * 86400.0
    } else {
        return None;
    };
    (seconds > 0.0).then_some(seconds as i64)
}

fn read_counted_window(detail: &Value) -> Option<(f64, Option<String>)> {
    let limit = number(detail.get("limit"))?;
    if limit <= 0.0 {
        return None;
    }
    let used = number(detail.get("used"))
        .or_else(|| number(detail.get("remaining")).map(|remaining| limit - remaining))?;
    Some(((used / limit * 100.0).clamp(0.0, 100.0), reset_at(detail)))
}

fn read_usage_windows(usages: Option<&Value>) -> Vec<QuotaWindow> {
    let Some(usages) = usages.and_then(Value::as_object) else {
        return Vec::new();
    };
    let read = |key: &str| {
        usages
            .get(key)
            .or_else(|| usages.get(&camel(key)))
            .and_then(read_share_window)
    };
    let mut windows = Vec::new();
    for (key, kind, span_secs) in [
        ("limit_5h", QuotaWindowKind::Rolling, Some(5 * 60 * 60)),
        ("limit_7d", QuotaWindowKind::Week, Some(WEEK_SECS)),
    ] {
        if let Some((used_percent, resets_at)) = read(key) {
            windows.push(QuotaWindow {
                kind,
                span_secs,
                used_percent,
                resets_at,
                remaining: None,
                limit: None,
                unit: None,
                display: None,
            });
        }
    }
    if let Some((used_percent, resets_at)) = read("limit_month_total") {
        windows.push(QuotaWindow {
            kind: QuotaWindowKind::Month,
            span_secs: None,
            used_percent,
            resets_at,
            remaining: None,
            limit: None,
            unit: None,
            display: None,
        });
    } else if let Some((used_percent, resets_at)) = read("limit_month_code") {
        windows.push(QuotaWindow {
            kind: QuotaWindowKind::Month,
            span_secs: None,
            used_percent,
            resets_at,
            remaining: None,
            limit: None,
            unit: None,
            display: None,
        });
    }
    windows
}

fn read_share_window(detail: &Value) -> Option<(f64, Option<String>)> {
    let ratio = number(detail.get("used_ratio")).or_else(|| number(detail.get("usedRatio")))?;
    Some(((ratio * 100.0).clamp(0.0, 100.0), reset_at(detail)))
}

fn kind_for_span(span_secs: Option<i64>) -> QuotaWindowKind {
    match span_secs {
        Some(86_400) => QuotaWindowKind::Day,
        Some(WEEK_SECS) => QuotaWindowKind::Week,
        Some(span) if span >= 28 * 86_400 => QuotaWindowKind::Month,
        Some(_) => QuotaWindowKind::Rolling,
        None => QuotaWindowKind::Custom,
    }
}

fn slot_key(kind: QuotaWindowKind, span_secs: Option<i64>) -> (QuotaWindowKind, Option<i64>) {
    match kind {
        QuotaWindowKind::Day => (kind, Some(86_400)),
        QuotaWindowKind::Week => (kind, Some(WEEK_SECS)),
        QuotaWindowKind::Month => (kind, None),
        _ => (kind, span_secs),
    }
}

fn window_order(window: &QuotaWindow) -> (u8, i64) {
    let rank = match window.kind {
        QuotaWindowKind::Rolling | QuotaWindowKind::Day => 0,
        QuotaWindowKind::Week => 1,
        QuotaWindowKind::Month => 2,
        QuotaWindowKind::Custom => 3,
    };
    (rank, window.span_secs.unwrap_or(i64::MAX))
}

fn read_booster(root: &Value) -> Option<QuotaBalance> {
    let wallet = root
        .get("booster_wallet")
        .or_else(|| root.get("boosterWallet"))?;
    let balance = wallet.get("balance")?;
    if balance.get("type").and_then(Value::as_str) != Some("BOOSTER") {
        return None;
    }
    if number(balance.get("amount")).unwrap_or_default() <= 0.0 {
        return None;
    }
    let left = number(balance.get("amountLeft"))
        .unwrap_or_default()
        .max(0.0);
    // amountLeft 以“分 × 10^6”计。
    let amount = (left / 1_000_000.0).round() / 100.0;
    let currency = ["monthlyChargeLimit", "monthlyUsed"]
        .iter()
        .find_map(|key| {
            wallet
                .get(*key)
                .and_then(|value| value.get("currency"))
                .and_then(Value::as_str)
        })
        .unwrap_or("USD")
        .to_string();
    Some(QuotaBalance {
        parts: vec![QuotaBalancePart {
            currency,
            amount: format!("{amount:.2}"),
        }],
        available: amount > 0.0,
    })
}

fn reset_at(detail: &Value) -> Option<String> {
    for key in ["resetTime", "resetAt", "reset_at", "reset_time"] {
        let Some(value) = detail.get(key) else {
            continue;
        };
        if let Some(text) = value.as_str() {
            if chrono::DateTime::parse_from_rfc3339(text).is_ok() {
                return Some(text.to_string());
            }
        }
        if let Some(number) = number(Some(value)) {
            if number > 0.0 {
                let millis = if number > 1e11 {
                    number as i64
                } else {
                    (number * 1000.0) as i64
                };
                return chrono::DateTime::from_timestamp_millis(millis)
                    .map(|time| time.to_rfc3339());
            }
        }
    }
    None
}

fn camel(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut upper = false;
    for character in value.chars() {
        if character == '_' {
            upper = true;
        } else if upper {
            result.extend(character.to_uppercase());
            upper = false;
        } else {
            result.push(character);
        }
    }
    result
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
    use super::{parse, QuotaWindowKind};

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
        assert_eq!(quota.windows[0].kind, QuotaWindowKind::Rolling);
        assert_eq!(quota.windows[0].span_secs, Some(5 * 60 * 60));
        assert_eq!(quota.windows[0].used_percent, 12.0);
        assert_eq!(quota.windows[1].kind, QuotaWindowKind::Week);
        assert_eq!(quota.windows[1].span_secs, Some(7 * 24 * 60 * 60));
        assert_eq!(quota.windows[1].used_percent, 12.0);
    }

    #[test]
    fn reads_month_and_booster() {
        let quota = parse(
            r#"{
                "usage": {"limit":"100","used":"12","resetTime":"2026-09-30T05:24:18Z"},
                "limits": [{
                    "window": {"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
                    "detail": {"limit":"100","remaining":"88","resetTime":"2026-09-30T05:24:18Z"}
                }],
                "usages": {
                    "limit_5h": {"used_ratio": 0.44, "reset_time": "2026-09-30T05:24:18Z"},
                    "limit_7d": {"used_ratio": 0.18, "reset_time": "2026-10-02T00:00:00Z"},
                    "limit_month_total": {"used_ratio": 0.31, "reset_time": "2026-10-01T00:00:00Z"},
                    "limit_month_code": {"used_ratio": 0.9, "reset_time": "2026-10-01T00:00:00Z"}
                },
                "booster_wallet": {
                    "balance": {"type": "BOOSTER", "amount": 100000000, "amountLeft": 2500000000},
                    "monthlyChargeLimit": {"currency": "CNY"}
                }
            }"#,
        )
        .expect("Kimi quota parses");
        assert_eq!(quota.windows.len(), 3);
        assert_eq!(quota.windows[0].kind, QuotaWindowKind::Rolling);
        assert_eq!(quota.windows[0].used_percent, 44.0);
        assert_eq!(quota.windows[1].kind, QuotaWindowKind::Week);
        assert_eq!(quota.windows[1].used_percent, 18.0);
        assert_eq!(quota.windows[2].kind, QuotaWindowKind::Month);
        assert_eq!(quota.windows[2].used_percent, 31.0);
        let balance = quota.balance.expect("booster");
        assert_eq!(balance.parts[0].currency, "CNY");
        assert_eq!(balance.parts[0].amount, "25.00");
        assert!(balance.available);
    }
}
