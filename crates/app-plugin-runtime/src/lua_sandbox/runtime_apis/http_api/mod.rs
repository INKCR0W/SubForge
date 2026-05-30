use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::time::Duration;

use app_transport::{NetworkProfileFactory, TransportProfile};
use mlua::{Error as LuaError, Lua, LuaSerdeExt, Table, Value as LuaValue};
use reqwest::{Method, Url};

use super::map_lua_error;
use super::{CookieStore, HttpRequestInput, HttpResponseOutput};
use super::{
    HTTP_REQUEST_LIMIT_SENTINEL, HTTP_RESPONSE_LIMIT_SENTINEL, SCRIPT_HTTP_MAX_REDIRECTS,
    SCRIPT_HTTP_MAX_REQUESTS, SCRIPT_HTTP_MAX_RESPONSE_BYTES, SCRIPT_HTTP_TIMEOUT_MS,
};
use crate::lua_sandbox::ExecutionDeadline;
use crate::{PluginRuntimeError, PluginRuntimeResult};

mod cookies;
mod headers;
mod runtime;
pub(crate) mod target_guard;

pub(super) fn register_http_api(
    lua: &Lua,
    network_profile: &str,
    cookie_store: CookieStore,
    request_counter: std::sync::Arc<AtomicUsize>,
    execution_deadline: ExecutionDeadline,
) -> PluginRuntimeResult<()> {
    let transport_profile = NetworkProfileFactory::create(network_profile)
        .map_err(|error| PluginRuntimeError::ScriptRuntime(error.to_string()))?;
    let http_table = lua.create_table().map_err(map_lua_error)?;

    let request_cookie_store = std::sync::Arc::clone(&cookie_store);
    let request_fn = lua
        .create_function(move |lua, request_table: Table| {
            let next = request_counter
                .fetch_add(1, AtomicOrdering::Relaxed)
                .saturating_add(1);
            if next > SCRIPT_HTTP_MAX_REQUESTS {
                return Err(LuaError::runtime(HTTP_REQUEST_LIMIT_SENTINEL));
            }

            let request: HttpRequestInput = lua.from_value(LuaValue::Table(request_table))?;
            let response = execute_http_request(
                transport_profile.as_ref(),
                request,
                std::sync::Arc::clone(&request_cookie_store),
                &execution_deadline,
            )?;
            lua.to_value(&response)
        })
        .map_err(map_lua_error)?;

    http_table
        .set("request", request_fn)
        .map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("http", http_table).map_err(map_lua_error)?;
    Ok(())
}

fn execute_http_request(
    transport_profile: &dyn TransportProfile,
    request: HttpRequestInput,
    cookie_store: CookieStore,
    execution_deadline: &ExecutionDeadline,
) -> Result<HttpResponseOutput, LuaError> {
    execute_http_request_with_target_guard(
        transport_profile,
        request,
        cookie_store,
        execution_deadline,
        target_guard::ensure_allowed_target,
    )
}

fn execute_http_request_with_target_guard<F>(
    transport_profile: &dyn TransportProfile,
    request: HttpRequestInput,
    cookie_store: CookieStore,
    execution_deadline: &ExecutionDeadline,
    ensure_allowed_target: F,
) -> Result<HttpResponseOutput, LuaError>
where
    F: Fn(&Url) -> Result<(), LuaError>,
{
    let url = Url::parse(request.url.trim())
        .map_err(|error| LuaError::runtime(format!("http.request url 非法：{error}")))?;
    ensure_allowed_target(&url)?;

    let requested_timeout_ms = request
        .timeout_ms
        .unwrap_or(SCRIPT_HTTP_TIMEOUT_MS)
        .min(SCRIPT_HTTP_TIMEOUT_MS);
    let timeout = timeout_with_remaining_budget(requested_timeout_ms, execution_deadline)?;
    let redirect_policy = target_guard::redirect_policy(SCRIPT_HTTP_MAX_REDIRECTS);
    let client = transport_profile
        .build_client_with_guarded_dns(
            timeout,
            SCRIPT_HTTP_MAX_REDIRECTS,
            Some(redirect_policy),
            target_guard::is_forbidden_ip,
        )
        .map_err(|error| LuaError::runtime(format!("http.request 客户端初始化失败：{error}")))?;

    let method = request
        .method
        .as_deref()
        .unwrap_or("GET")
        .parse::<Method>()
        .map_err(|error| LuaError::runtime(format!("http.request method 非法：{error}")))?;
    let headers = headers::build_request_headers(
        transport_profile,
        request.headers.as_ref(),
        std::sync::Arc::clone(&cookie_store),
    )?;

    let mut retry_attempt = 0usize;
    loop {
        if retry_attempt > 0 {
            let delay = runtime::retry_backoff(transport_profile.request_delay(), retry_attempt);
            sleep_with_remaining_budget(delay, execution_deadline)?;
        }

        let timeout = timeout.min(remaining_budget(execution_deadline)?);

        let client_cloned = client.clone();
        let url_cloned = url.clone();
        let headers_cloned = headers.clone();
        let method_cloned = method.clone();
        let body = request.body.clone();

        let response = runtime::run_reqwest_blocking(
            async move {
                let mut request_builder = client_cloned
                    .request(method_cloned, url_cloned)
                    .headers(headers_cloned)
                    .timeout(timeout);
                if let Some(body) = body {
                    request_builder = request_builder.body(body);
                }

                let mut response = request_builder
                    .send()
                    .await
                    .map_err(|error| format!("发送请求失败：{error}"))?;
                let status = response.status();
                let final_url = response.url().to_string();
                let response_headers = response.headers().clone();
                if let Some(content_length) = response.content_length()
                    && content_length > SCRIPT_HTTP_MAX_RESPONSE_BYTES as u64
                {
                    return Err(format!(
                        "响应体过大：{} bytes（限制 {} bytes）",
                        content_length, SCRIPT_HTTP_MAX_RESPONSE_BYTES
                    ));
                }

                let mut body = Vec::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|error| format!("读取响应体失败：{error}"))?
                {
                    body.extend_from_slice(&chunk);
                    if body.len() > SCRIPT_HTTP_MAX_RESPONSE_BYTES {
                        return Err(HTTP_RESPONSE_LIMIT_SENTINEL.to_string());
                    }
                }
                Ok((status, final_url, response_headers, body))
            },
            remaining_budget(execution_deadline)?,
        )
        .map_err(|error| LuaError::runtime(format!("http.request 失败：{error}")))?;

        let (status, final_url, response_headers, response_body) = response;
        cookies::apply_response_cookies(&response_headers, std::sync::Arc::clone(&cookie_store))?;
        if !status.is_success() {
            if retry_attempt < transport_profile.max_retries()
                && transport_profile.is_retryable_status(status)
            {
                retry_attempt += 1;
                continue;
            }
            return Err(LuaError::runtime(format!(
                "http.request 返回非成功状态码：{}",
                status.as_u16()
            )));
        }

        if response_body.len() > SCRIPT_HTTP_MAX_RESPONSE_BYTES {
            return Err(LuaError::runtime(HTTP_RESPONSE_LIMIT_SENTINEL));
        }

        let headers = headers::flatten_response_headers(&response_headers);
        let body = String::from_utf8_lossy(&response_body).to_string();
        return Ok(HttpResponseOutput {
            status: status.as_u16(),
            headers,
            body,
            final_url,
        });
    }
}

fn timeout_with_remaining_budget(
    requested_timeout_ms: u64,
    execution_deadline: &ExecutionDeadline,
) -> Result<Duration, LuaError> {
    let requested_timeout = Duration::from_millis(requested_timeout_ms);
    Ok(requested_timeout.min(remaining_budget(execution_deadline)?))
}

fn remaining_budget(execution_deadline: &ExecutionDeadline) -> Result<Duration, LuaError> {
    let remaining = execution_deadline.remaining().unwrap_or_else(|| {
        // http.request 正常只会在脚本执行中调用；未设置 deadline 时保守使用单次请求上限。
        Duration::from_millis(SCRIPT_HTTP_TIMEOUT_MS)
    });
    if remaining.is_zero() {
        return Err(LuaError::runtime(super::super::HOOK_TIMEOUT_SENTINEL));
    }
    Ok(remaining)
}

fn sleep_with_remaining_budget(
    delay: Duration,
    execution_deadline: &ExecutionDeadline,
) -> Result<(), LuaError> {
    let remaining = remaining_budget(execution_deadline)?;
    if delay >= remaining {
        return Err(LuaError::runtime(super::super::HOOK_TIMEOUT_SENTINEL));
    }
    std::thread::sleep(delay);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{IpAddr, TcpListener};
    use std::thread;
    use std::time::{Duration, Instant};

    use app_transport::TransportResult;
    use reqwest::redirect::Policy;

    use super::*;
    use crate::lua_sandbox::HOOK_TIMEOUT_SENTINEL;
    use crate::lua_sandbox::runtime_apis::new_cookie_store;

    #[test]
    fn execute_http_request_uses_remaining_script_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("测试 HTTP 服务应可监听");
        let address = listener.local_addr().expect("应能读取测试服务地址");
        let server = thread::spawn(move || {
            if let Ok((mut stream, _peer)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                thread::sleep(Duration::from_millis(250));
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            }
        });

        let deadline = ExecutionDeadline::new();
        deadline.set(
            Instant::now()
                .checked_add(Duration::from_millis(40))
                .expect("deadline 应可计算"),
        );
        let started = Instant::now();
        let error = execute_http_request_with_target_guard(
            &LocalTestProfile,
            HttpRequestInput {
                url: format!("http://{address}/slow"),
                method: None,
                headers: None,
                body: None,
                timeout_ms: Some(SCRIPT_HTTP_TIMEOUT_MS),
            },
            new_cookie_store(),
            &deadline,
            |_url| Ok(()),
        )
        .expect_err("HTTP 请求应被脚本总剩余时间预算中断");

        assert!(
            error.to_string().contains(HOOK_TIMEOUT_SENTINEL),
            "错误应携带脚本总超时哨兵，实际为：{error}"
        );
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "http.request 不应等待底层请求或 15s 单次请求超时"
        );

        let _ = server.join();
        deadline.clear();
    }

    #[derive(Debug)]
    struct LocalTestProfile;

    impl TransportProfile for LocalTestProfile {
        fn profile_name(&self) -> &'static str {
            "local_test"
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(15)
        }

        fn max_redirects(&self) -> usize {
            0
        }

        fn default_user_agent(&self) -> &'static str {
            "subforge-test"
        }

        fn request_delay(&self) -> Duration {
            Duration::ZERO
        }

        fn build_client_with_guarded_dns(
            &self,
            timeout: Duration,
            max_redirects: usize,
            redirect_policy: Option<Policy>,
            _is_forbidden_ip: fn(IpAddr) -> bool,
        ) -> TransportResult<reqwest::Client> {
            self.build_client_with_limits(timeout, max_redirects, redirect_policy)
        }
    }
}
