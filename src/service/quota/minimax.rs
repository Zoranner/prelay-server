use std::time::Duration;

use crate::domain::catalog::{CatalogQuota, QuotaAuth};
use prelay_protocol::{ProviderQuota, QuotaError, QuotaErrorCode, QuotaWindow, QuotaWindowKind};
use serde::Deserialize;

use super::{parse_epoch_millis, quota_error, resolve_quota_url};

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    quota: &CatalogQuota,
    api_key: &str,
) -> Result<ProviderQuota, QuotaError> {
    let url = resolve_quota_url(base_url, &quota.url);
    let request = match quota.auth {
        QuotaAuth::Bearer => client.get(&url).bearer_auth(api_key),
        QuotaAuth::Raw => client
            .get(&url)
            .header(reqwest::header::AUTHORIZATION, api_key),
    };
    let response = request
        .timeout(Duration::from_secs(quota.timeout_secs))
        .send()
        .await
        .map_err(|_| quota_error(QuotaErrorCode::Unreachable, "MiniMax 额度接口请求失败"))?;
    if !response.status().is_success() {
        return Err(http_error(response.status()));
    }
    let body = response.text().await.map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("MiniMax 额度响应无法解析: {error}"),
        )
    })?;
    parse_remains(&body)
}

fn parse_remains(body: &str) -> Result<ProviderQuota, QuotaError> {
    let body = serde_json::from_str::<RemainsEnvelope>(body).map_err(|error| {
        quota_error(
            QuotaErrorCode::InvalidResponse,
            format!("MiniMax 额度响应无法解析: {error}"),
        )
    })?;
    if body.base_resp.status_code != 0 {
        return Err(quota_error(
            QuotaErrorCode::Unauthorized,
            format!("MiniMax 额度接口返回状态 {}", body.base_resp.status_code),
        ));
    }
    let general = body
        .model_remains
        .iter()
        .find(|row| row.model_name == "general")
        .ok_or_else(|| {
            quota_error(
                QuotaErrorCode::InvalidResponse,
                "MiniMax 额度响应没有可用的模型桶",
            )
        })?;
    let mut windows = vec![window(
        QuotaWindowKind::Rolling,
        5 * 60 * 60,
        percent_used(general.current_interval_remaining_percent),
        general.end_time,
    )];
    if let Some(used_percent) = weekly_used_percent(
        general.current_weekly_status,
        general.current_weekly_remaining_percent,
    ) {
        windows.push(window(
            QuotaWindowKind::Week,
            7 * 24 * 60 * 60,
            used_percent,
            general.weekly_end_time,
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

fn http_error(status: reqwest::StatusCode) -> QuotaError {
    let code = match status.as_u16() {
        401 | 403 => QuotaErrorCode::Unauthorized,
        500..=599 => QuotaErrorCode::Unreachable,
        _ => QuotaErrorCode::InvalidResponse,
    };
    quota_error(
        code,
        format!("MiniMax 额度接口返回 HTTP {}", status.as_u16()),
    )
}

fn percent_used(remaining: i64) -> f64 {
    (100 - remaining.clamp(0, 100)) as f64
}

fn weekly_used_percent(status: i64, remaining: i64) -> Option<f64> {
    match status {
        1 => Some(percent_used(remaining)),
        // 2 表示当期额度已用尽；3 表示套餐没有周窗口。
        2 => Some(100.0),
        _ => None,
    }
}

fn window(kind: QuotaWindowKind, span_secs: i64, used_percent: f64, resets_at: i64) -> QuotaWindow {
    QuotaWindow {
        kind,
        span_secs: Some(span_secs),
        used_percent,
        resets_at: parse_epoch_millis(resets_at),
        remaining: None,
        limit: None,
        unit: None,
        display: None,
    }
}

#[derive(Deserialize)]
struct RemainsEnvelope {
    #[serde(default)]
    model_remains: Vec<ModelRemains>,
    base_resp: BaseResp,
}

#[derive(Deserialize)]
struct BaseResp {
    status_code: i64,
}

#[derive(Deserialize)]
struct ModelRemains {
    model_name: String,
    end_time: i64,
    current_interval_remaining_percent: i64,
    #[serde(default)]
    current_weekly_status: i64,
    weekly_end_time: i64,
    current_weekly_remaining_percent: i64,
}

#[cfg(test)]
mod tests {
    use super::{parse_remains, QuotaWindowKind};
    use crate::service::quota::resolve_quota_url;

    #[test]
    fn resolves_the_configured_endpoint() {
        assert_eq!(
            resolve_quota_url(
                "https://api.minimax.cn/v1",
                "https://www.minimax.cn/v1/token_plan/remains"
            ),
            "https://www.minimax.cn/v1/token_plan/remains"
        );
        assert_eq!(
            resolve_quota_url(
                "https://api.minimax.io/v1",
                "https://www.minimax.io/v1/token_plan/remains"
            ),
            "https://www.minimax.io/v1/token_plan/remains"
        );
    }

    #[test]
    fn parses_the_five_hour_and_weekly_windows() {
        let quota = parse_remains(
            r#"{
                "model_remains": [{
                    "model_name": "general",
                    "end_time": 1785182400000,
                    "current_interval_remaining_percent": 99,
                    "current_weekly_status": 1,
                    "weekly_end_time": 1785715200000,
                    "current_weekly_remaining_percent": 48
                }],
                "base_resp": { "status_code": 0, "status_msg": "success" }
            }"#,
        )
        .expect("remains parses");
        let windows = quota.windows;
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].kind, QuotaWindowKind::Rolling);
        assert_eq!(windows[0].span_secs, Some(5 * 60 * 60));
        assert_eq!(windows[0].used_percent, 1.0);
        assert_eq!(windows[1].kind, QuotaWindowKind::Week);
        assert_eq!(windows[1].span_secs, Some(7 * 24 * 60 * 60));
        assert_eq!(windows[1].used_percent, 52.0);
    }

    #[test]
    fn skips_weekly_when_the_plan_has_no_weekly_window() {
        let quota = parse_remains(
            r#"{
                "model_remains": [{
                    "model_name": "general",
                    "end_time": 1785182400000,
                    "current_interval_remaining_percent": 99,
                    "current_weekly_status": 3,
                    "weekly_end_time": 1785715200000,
                    "current_weekly_remaining_percent": 100
                }],
                "base_resp": { "status_code": 0 }
            }"#,
        )
        .expect("remains parses");
        assert_eq!(quota.windows.len(), 1);
        assert_eq!(quota.windows[0].kind, QuotaWindowKind::Rolling);
    }

    #[test]
    fn exhausted_weekly_window_stays_full() {
        let quota = parse_remains(
            r#"{
                "model_remains": [{
                    "model_name": "general",
                    "end_time": 1785182400000,
                    "current_interval_remaining_percent": 99,
                    "current_weekly_status": 2,
                    "weekly_end_time": 1785715200000,
                    "current_weekly_remaining_percent": 50
                }],
                "base_resp": { "status_code": 0 }
            }"#,
        )
        .expect("remains parses");
        assert_eq!(quota.windows.len(), 2);
        assert_eq!(quota.windows[1].kind, QuotaWindowKind::Week);
        assert_eq!(quota.windows[1].used_percent, 100.0);
    }
}
