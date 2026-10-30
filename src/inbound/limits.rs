//! 协议入口（客户端 → Prelay）的请求体上限。
//!
//! 上限同时也是服务端单请求内存的护栏：请求体会先被缓冲成 JSON，再参与裁剪与转发。

use std::sync::OnceLock;

/// 未配置时的上限：256 MiB。
const DEFAULT_MAX_BYTES: usize = 256 * 1024 * 1024;

static PROTOCOL_BODY_LIMIT: OnceLock<usize> = OnceLock::new();

pub fn initialize_from_environment() -> Result<usize, String> {
    let configured_limit = parse_limit(
        std::env::var("PROTOCOL_MAX_REQUEST_BODY_BYTES")
            .ok()
            .as_deref(),
    )?;
    PROTOCOL_BODY_LIMIT
        .set(configured_limit)
        .map_err(|_| "protocol body limit was already initialized".to_owned())?;
    Ok(limit())
}

pub fn limit() -> usize {
    *PROTOCOL_BODY_LIMIT.get_or_init(|| DEFAULT_MAX_BYTES)
}

fn parse_limit(value: Option<&str>) -> Result<usize, String> {
    match value {
        Some(value) => match value.parse::<usize>() {
            Ok(value) if value > 0 => Ok(value),
            _ => Err("PROTOCOL_MAX_REQUEST_BODY_BYTES must be a positive integer".to_string()),
        },
        None => Ok(DEFAULT_MAX_BYTES),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_limit, DEFAULT_MAX_BYTES};

    #[test]
    fn defaults_to_256_mib() {
        assert_eq!(DEFAULT_MAX_BYTES, 256 * 1024 * 1024);
        assert_eq!(parse_limit(None).expect("default limit"), DEFAULT_MAX_BYTES);
    }

    #[test]
    fn parses_configured_limit() {
        assert_eq!(
            parse_limit(Some("536870912")).expect("configured limit"),
            512 * 1024 * 1024
        );
    }

    #[test]
    fn rejects_invalid_limits() {
        assert!(parse_limit(Some("0")).is_err());
        assert!(parse_limit(Some("-1")).is_err());
        assert!(parse_limit(Some("256MiB")).is_err());
    }
}
