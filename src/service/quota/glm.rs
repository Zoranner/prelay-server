use prelay_protocol::{ProviderQuota, QuotaError, QuotaErrorCode, QuotaWindow, QuotaWindowKind};
use serde::Deserialize;

use crate::domain::catalog::CatalogQuota;

use super::{fetch_quota_body, parse_epoch_millis, quota_error};

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Result<ProviderQuota, QuotaError> {
    let body = fetch_quota_body(client, base_url, quota, api_key, "GLM").await?;
    parse(&body)
}

fn parse(body: &str) -> Result<ProviderQuota, QuotaError> {
    let envelope = serde_json::from_str::<Envelope>(body).map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("GLM 额度响应无法解析: {error}"),
        )
    })?;
    if envelope.success == Some(false) {
        return Err(quota_error(
            QuotaErrorCode::Unauthorized,
            envelope
                .msg
                .unwrap_or_else(|| "GLM 拒绝额度查询".to_string()),
        ));
    }
    if envelope.code.is_some_and(|code| code != 0 && code != 200) {
        return Err(quota_error(
            QuotaErrorCode::Unauthorized,
            envelope
                .msg
                .unwrap_or_else(|| "GLM 拒绝额度查询".to_string()),
        ));
    }
    let data = envelope
        .data
        .ok_or_else(|| quota_error(QuotaErrorCode::InvalidResponse, "GLM 额度响应没有 data"))?;
    let mut windows = Vec::with_capacity(data.limits.len());
    for limit in data.limits {
        if limit.kind.as_deref() == Some("TIME_LIMIT") {
            continue;
        }
        let (kind, span_secs) = match limit.unit {
            Some(3) => {
                let hours = limit.number.unwrap_or(5).max(1);
                (QuotaWindowKind::Rolling, Some(hours * 60 * 60))
            }
            Some(6) => (QuotaWindowKind::Week, Some(7 * 24 * 60 * 60)),
            _ => (QuotaWindowKind::Custom, None),
        };
        let display = match (limit.current_value, limit.usage) {
            (Some(used), Some(total)) => Some(format!("{used} / {total}")),
            _ => None,
        };
        windows.push(QuotaWindow {
            kind,
            span_secs,
            used_percent: limit.percentage.unwrap_or(0.0).clamp(0.0, 100.0),
            resets_at: limit.next_reset_time.and_then(parse_epoch_millis),
            remaining: None,
            limit: None,
            unit: None,
            display,
        });
    }
    if windows.is_empty() {
        return Err(quota_error(
            QuotaErrorCode::InvalidResponse,
            "GLM 额度响应没有窗口",
        ));
    }
    Ok(ProviderQuota {
        plan: data.level,
        windows,
        balance: None,
        error: None,
        read_at: None,
        stale: false,
    })
}

#[derive(Deserialize)]
struct Envelope {
    code: Option<i64>,
    success: Option<bool>,
    msg: Option<String>,
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    level: Option<String>,
    #[serde(default)]
    limits: Vec<Limit>,
}

#[derive(Deserialize)]
struct Limit {
    #[serde(rename = "type")]
    kind: Option<String>,
    unit: Option<i64>,
    number: Option<i64>,
    percentage: Option<f64>,
    #[serde(rename = "currentValue")]
    current_value: Option<f64>,
    usage: Option<f64>,
    #[serde(rename = "nextResetTime")]
    next_reset_time: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::{parse, QuotaWindowKind};

    #[test]
    fn parses_five_hour_and_week_windows() {
        let quota = parse(
            r#"{
                "code": 200,
                "success": true,
                "data": {
                    "level": "pro",
                    "limits": [
                        {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":12.5,"nextResetTime":1785182400000},
                        {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":40,"nextResetTime":1785715200000},
                        {"type":"TIME_LIMIT","unit":5,"number":1,"percentage":3,"nextResetTime":1785715200000}
                    ]
                }
            }"#,
        )
        .expect("GLM quota parses");
        assert_eq!(quota.plan.as_deref(), Some("pro"));
        assert_eq!(quota.windows.len(), 2);
        assert_eq!(quota.windows[0].kind, QuotaWindowKind::Rolling);
        assert_eq!(quota.windows[0].span_secs, Some(5 * 60 * 60));
        assert_eq!(quota.windows[0].used_percent, 12.5);
        assert_eq!(quota.windows[1].kind, QuotaWindowKind::Week);
        assert_eq!(quota.windows[1].span_secs, Some(7 * 24 * 60 * 60));
        assert!(quota.balance.is_none());
    }

    #[test]
    fn rejects_a_refused_envelope() {
        let error = parse(r#"{"code":401,"success":false,"msg":"not logged in"}"#)
            .expect_err("refusal is an error");
        assert_eq!(error.code, prelay_protocol::QuotaErrorCode::Unauthorized);
    }
}
