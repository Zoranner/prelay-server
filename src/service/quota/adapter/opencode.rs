use prelay_protocol::{ProviderQuota, QuotaError, QuotaErrorCode, QuotaWindow, QuotaWindowKind};
use serde_json::Value;

use crate::domain::catalog::CatalogQuota;

use crate::service::quota::{fetch_quota_body, quota_error};

pub(crate) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Result<ProviderQuota, QuotaError> {
    let body = fetch_quota_body(client, base_url, quota, api_key, "OpenCode Go").await?;
    parse(&body)
}

fn parse(body: &str) -> Result<ProviderQuota, QuotaError> {
    let root = serde_json::from_str::<Value>(body).map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("OpenCode Go 额度响应无法解析: {error}"),
        )
    })?;
    let usage = root.get("usage").ok_or_else(|| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            "OpenCode Go 额度响应没有 usage",
        )
    })?;
    let mut windows = Vec::new();
    for (key, kind, span_secs) in [
        ("rolling", QuotaWindowKind::Rolling, Some(5 * 60 * 60)),
        ("weekly", QuotaWindowKind::Week, Some(7 * 24 * 60 * 60)),
        ("monthly", QuotaWindowKind::Month, None),
    ] {
        let Some(window) = usage.get(key) else {
            continue;
        };
        let Some(percent) = number(window.get("percent")) else {
            continue;
        };
        windows.push(QuotaWindow {
            kind,
            span_secs,
            used_percent: percent.clamp(0.0, 100.0),
            resets_at: window
                .get("resetsAt")
                .and_then(Value::as_str)
                .filter(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok())
                .map(str::to_string),
            remaining: None,
            limit: None,
            unit: None,
            display: None,
        });
    }
    if windows.is_empty() {
        return Err(quota_error(
            QuotaErrorCode::InvalidResponse,
            "OpenCode Go 额度响应没有窗口",
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
    fn parses_rolling_weekly_and_monthly_windows() {
        let quota = parse(
            r#"{"usage":{
                "rolling":{"status":"ok","percent":52,"resetsAt":"2026-09-23T00:34:52.704Z"},
                "weekly":{"status":"ok","percent":20,"resetsAt":"2026-09-28T00:00:00.000Z"},
                "monthly":{"status":"ok","percent":61,"resetsAt":"2026-09-25T14:30:41.000Z"}
            }}"#,
        )
        .expect("OpenCode Go quota parses");
        assert_eq!(quota.windows.len(), 3);
        assert_eq!(quota.windows[0].kind, QuotaWindowKind::Rolling);
        assert_eq!(quota.windows[0].used_percent, 52.0);
        assert_eq!(quota.windows[2].kind, QuotaWindowKind::Month);
        assert_eq!(quota.windows[2].used_percent, 61.0);
    }
}
