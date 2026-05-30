use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tokio::runtime::Builder as TokioRuntimeBuilder;

use crate::lua_sandbox::HOOK_TIMEOUT_SENTINEL;

pub(super) fn run_reqwest_blocking<T, F>(future: F, remaining_budget: Duration) -> Result<T, String>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    if remaining_budget.is_zero() {
        return Err(HOOK_TIMEOUT_SENTINEL.to_string());
    }

    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = TokioRuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("初始化异步运行时失败：{error}"))
            .and_then(|runtime| runtime.block_on(future));
        let _ = sender.send(result);
    });

    match receiver.recv_timeout(remaining_budget) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(HOOK_TIMEOUT_SENTINEL.to_string()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("HTTP 请求线程异常退出".to_string()),
    }
}

pub(super) fn retry_backoff(base_delay: Duration, retry_attempt: usize) -> Duration {
    let base_delay = if base_delay.is_zero() {
        Duration::from_millis(100)
    } else {
        base_delay
    };
    let shift = retry_attempt.saturating_sub(1).min(8);
    base_delay.saturating_mul(1_u32 << shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_reqwest_blocking_respects_remaining_budget() {
        let started = std::time::Instant::now();
        let error = run_reqwest_blocking(
            async {
                tokio::time::sleep(Duration::from_millis(250)).await;
                Ok::<(), String>(())
            },
            Duration::from_millis(40),
        )
        .expect_err("超过剩余预算的异步请求应提前返回超时错误");

        assert!(
            error.contains(HOOK_TIMEOUT_SENTINEL),
            "错误信息应说明命中了脚本剩余时间预算，实际为：{error}"
        );

        assert!(
            started.elapsed() < Duration::from_millis(200),
            "阻塞等待不应持续到内部 future 完成"
        );
    }
}
