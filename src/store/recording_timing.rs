use std::time::Instant;

/// 记录路径的基线计时。默认使用 debug 级别，避免改变生产日志量；
/// 采样时通过 `RUST_LOG=prelay_server::recording_timing=debug` 打开。
pub(crate) fn log_elapsed(operation: &'static str, started_at: Instant, subject: Option<&str>) {
    tracing::debug!(
        target: "prelay_server::recording_timing",
        operation,
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        subject = subject.unwrap_or("")
    );
}
