use std::time::Duration;

const BASE_DELAY: Duration = Duration::from_millis(200);
const MAX_DELAY: Duration = Duration::from_secs(5);

/// 物化失败后的重试间隔。
///
/// 物化卡在一条记录上时，每个 tick 都会重试并记录同样的错误；指数退避并封顶后，
/// 日志不再刷屏，同时仍能在故障解除后自动恢复。
pub(super) fn retry_delay(consecutive_failures: u32) -> Duration {
    let factor = 1_u32.checked_shl(consecutive_failures.min(5)).unwrap_or(32);
    (BASE_DELAY * factor).min(MAX_DELAY)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::retry_delay;

    #[test]
    fn backs_off_exponentially_and_caps_the_delay() {
        assert_eq!(retry_delay(1), Duration::from_millis(400));
        assert_eq!(retry_delay(2), Duration::from_millis(800));
        assert!(retry_delay(3) < retry_delay(4));
        assert_eq!(retry_delay(9), Duration::from_secs(5));
    }
}
