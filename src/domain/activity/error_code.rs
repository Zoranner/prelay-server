//! 活动记录的错误码。
//!
//! 活动码是独立于管理 API 与 `/v1` 的码空间。新增码必须同时加入 [`ActivityErrorCode::ALL`]，
//! 并在客户端 `activityErrorMessages` 中提供文案。

pub(crate) struct ActivityErrorCode;

impl ActivityErrorCode {
    pub(crate) const UPSTREAM_STATUS: &'static str = "upstream_status";
    pub(crate) const UPSTREAM_CONNECT: &'static str = "upstream_connect";
    pub(crate) const UPSTREAM_RESPONSE_HEAD_TIMEOUT: &'static str =
        "upstream_response_head_timeout";
    pub(crate) const UPSTREAM_INVALID_RESPONSE: &'static str = "upstream_invalid_response";
    pub(crate) const UPSTREAM_BODY: &'static str = "upstream_body";
    pub(crate) const UPSTREAM_ERROR: &'static str = "upstream_error";
    pub(crate) const EMPTY_STREAM: &'static str = "empty_stream";
    pub(crate) const STREAM_ERROR: &'static str = "stream_error";
    pub(crate) const DOWNSTREAM_DISCONNECT: &'static str = "downstream_disconnect";
    pub(crate) const ACTIVITY_ROLLOUT_INCOMPLETE: &'static str = "activity_rollout_incomplete";
    pub(crate) const ACTIVITY_INTERRUPTED: &'static str = "activity_interrupted";
    #[cfg(test)]
    pub(crate) const ALL: [&'static str; 11] = [
        Self::UPSTREAM_STATUS,
        Self::UPSTREAM_CONNECT,
        Self::UPSTREAM_RESPONSE_HEAD_TIMEOUT,
        Self::UPSTREAM_INVALID_RESPONSE,
        Self::UPSTREAM_BODY,
        Self::UPSTREAM_ERROR,
        Self::EMPTY_STREAM,
        Self::STREAM_ERROR,
        Self::DOWNSTREAM_DISCONNECT,
        Self::ACTIVITY_ROLLOUT_INCOMPLETE,
        Self::ACTIVITY_INTERRUPTED,
    ];
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::ActivityErrorCode;

    #[test]
    fn activity_error_codes_are_registered_and_unique() {
        let mut seen = BTreeSet::new();

        for code in ActivityErrorCode::ALL {
            assert!(seen.insert(code), "duplicate activity error code: {code}");
        }

        assert_eq!(seen.len(), ActivityErrorCode::ALL.len());
    }
}
