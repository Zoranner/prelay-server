use std::time::Duration;

use prelay_protocol::{ProviderQuota, QuotaWindowKind, QuotaWindowResponse};
use serde::Deserialize;

use super::parse_epoch_millis;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) async fn fetch(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<ProviderQuota, String> {
    let url = remains_url(base_url);
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|_| "MiniMax 额度接口请求失败".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "MiniMax 额度接口返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = response
        .text()
        .await
        .map_err(|error| format!("MiniMax 额度响应无法解析: {error}"))?;
    parse_remains(&body)
}

fn parse_remains(body: &str) -> Result<ProviderQuota, String> {
    let body = serde_json::from_str::<RemainsEnvelope>(body)
        .map_err(|error| format!("MiniMax 额度响应无法解析: {error}"))?;
    if body.base_resp.status_code != 0 {
        return Err(format!(
            "MiniMax 额度接口返回状态 {}",
            body.base_resp.status_code
        ));
    }
    let general = body
        .model_remains
        .iter()
        .find(|row| row.model_name == "general")
        .or_else(|| {
            body.model_remains
                .iter()
                .find(|row| row.model_name != "video")
        })
        .ok_or_else(|| "MiniMax 额度响应没有可用的模型桶".to_string())?;
    Ok(ProviderQuota::Plan {
        windows: vec![
            QuotaWindowResponse {
                window: QuotaWindowKind::FiveHour,
                accumulate: false,
                remaining_percent: general.current_interval_remaining_percent as f64,
                remaining: None,
                limit: None,
                used: None,
                carry_over: None,
                resets_at: parse_epoch_millis(general.end_time),
            },
            QuotaWindowResponse {
                window: QuotaWindowKind::Week,
                accumulate: false,
                remaining_percent: general.current_weekly_remaining_percent as f64,
                remaining: None,
                limit: None,
                used: None,
                carry_over: None,
                resets_at: parse_epoch_millis(general.weekly_end_time),
            },
        ],
    })
}

fn remains_url(base_url: &str) -> String {
    let host = reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    let origin = if host.contains("minimax.io") {
        "https://www.minimax.io"
    } else {
        "https://www.minimax.cn"
    };
    format!("{origin}/v1/token_plan/remains")
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
    weekly_end_time: i64,
    current_weekly_remaining_percent: i64,
}

#[cfg(test)]
mod tests {
    use super::{parse_remains, remains_url};

    #[test]
    fn selects_the_regional_endpoint() {
        assert_eq!(
            remains_url("https://api.minimax.cn/v1"),
            "https://www.minimax.cn/v1/token_plan/remains"
        );
        assert_eq!(
            remains_url("https://api.minimax.io/v1"),
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
                    "weekly_end_time": 1785715200000,
                    "current_weekly_remaining_percent": 48
                }],
                "base_resp": { "status_code": 0, "status_msg": "success" }
            }"#,
        )
        .expect("remains parses");
        let prelay_protocol::ProviderQuota::Plan { windows } = quota else {
            panic!("expected a plan quota");
        };
        assert_eq!(windows.len(), 2);
        assert_eq!(
            windows[0].window,
            prelay_protocol::QuotaWindowKind::FiveHour
        );
        assert_eq!(windows[0].remaining_percent, 99.0);
        assert_eq!(windows[1].window, prelay_protocol::QuotaWindowKind::Week);
        assert_eq!(windows[1].remaining_percent, 48.0);
    }
}
