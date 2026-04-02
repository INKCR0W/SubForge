use std::net::{IpAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::thread;
use std::time::Duration;

use app_transport::{NetworkProfileFactory, TransportProfile};
use mlua::{Error as LuaError, Lua, LuaSerdeExt, Table, Value as LuaValue};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Url};
use tokio::runtime::Builder as TokioRuntimeBuilder;

use super::map_lua_error;
use super::{CookieEntry, CookieStore, HttpRequestInput, HttpResponseOutput};
use super::{
    HTTP_REQUEST_LIMIT_SENTINEL, HTTP_RESPONSE_LIMIT_SENTINEL, SCRIPT_HTTP_MAX_REDIRECTS,
    SCRIPT_HTTP_MAX_REQUESTS, SCRIPT_HTTP_MAX_RESPONSE_BYTES, SCRIPT_HTTP_TIMEOUT_MS,
};
use crate::{PluginRuntimeError, PluginRuntimeResult};

pub(super) fn register_http_api(
    lua: &Lua,
    network_profile: &str,
    cookie_store: CookieStore,
    request_counter: std::sync::Arc<AtomicUsize>,
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
) -> Result<HttpResponseOutput, LuaError> {
    let url = Url::parse(request.url.trim())
        .map_err(|error| LuaError::runtime(format!("http.request url 非法：{error}")))?;
    ensure_allowed_target(&url)?;

    let timeout_ms = request
        .timeout_ms
        .unwrap_or(SCRIPT_HTTP_TIMEOUT_MS)
        .min(SCRIPT_HTTP_TIMEOUT_MS);
    let timeout = Duration::from_millis(timeout_ms);
    let client = transport_profile
        .build_client_with_limits(timeout, SCRIPT_HTTP_MAX_REDIRECTS)
        .map_err(|error| LuaError::runtime(format!("http.request 客户端初始化失败：{error}")))?;

    let method = request
        .method
        .as_deref()
        .unwrap_or("GET")
        .parse::<Method>()
        .map_err(|error| LuaError::runtime(format!("http.request method 非法：{error}")))?;
    let headers = build_request_headers(
        transport_profile,
        request.headers.as_ref(),
        std::sync::Arc::clone(&cookie_store),
    )?;

    let mut retry_attempt = 0usize;
    loop {
        if retry_attempt > 0 {
            thread::sleep(retry_backoff(
                transport_profile.request_delay(),
                retry_attempt,
            ));
        }

        let client_cloned = client.clone();
        let url_cloned = url.clone();
        let headers_cloned = headers.clone();
        let method_cloned = method.clone();
        let body = request.body.clone();

        let response = run_reqwest_blocking(async move {
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
        })
        .map_err(|error| LuaError::runtime(format!("http.request 失败：{error}")))?;

        let (status, final_url, response_headers, response_body) = response;
        apply_response_cookies(&response_headers, std::sync::Arc::clone(&cookie_store))?;
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

        let headers = flatten_response_headers(&response_headers);
        let body = String::from_utf8_lossy(&response_body).to_string();
        return Ok(HttpResponseOutput {
            status: status.as_u16(),
            headers,
            body,
            final_url,
        });
    }
}

fn build_request_headers(
    transport_profile: &dyn TransportProfile,
    headers: Option<&std::collections::BTreeMap<String, String>>,
    cookie_store: CookieStore,
) -> Result<HeaderMap, LuaError> {
    let mut request_headers = HeaderMap::new();
    for (name, value) in transport_profile.default_headers() {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            LuaError::runtime(format!(
                "http.request 默认 Header 名非法（{name}）：{error}"
            ))
        })?;
        let header_value = HeaderValue::from_str(value).map_err(|error| {
            LuaError::runtime(format!(
                "http.request 默认 Header 值非法（{name}）：{error}"
            ))
        })?;
        request_headers.insert(header_name, header_value);
    }

    if let Some(headers) = headers {
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                LuaError::runtime(format!("http.request Header 名非法（{name}）：{error}"))
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|error| {
                LuaError::runtime(format!("http.request Header 值非法（{name}）：{error}"))
            })?;
            request_headers.insert(header_name, header_value);
        }
    }

    let has_cookie_header = request_headers.contains_key("cookie");
    if !has_cookie_header {
        let cookie_header = compose_cookie_header(cookie_store)?;
        if !cookie_header.is_empty() {
            let header_value = HeaderValue::from_str(cookie_header.as_str()).map_err(|error| {
                LuaError::runtime(format!(
                    "http.request Cookie Header 值非法（cookie）：{error}"
                ))
            })?;
            request_headers.insert(HeaderName::from_static("cookie"), header_value);
        }
    }

    Ok(request_headers)
}

fn flatten_response_headers(headers: &HeaderMap) -> std::collections::BTreeMap<String, String> {
    let mut merged = std::collections::BTreeMap::new();
    for (name, value) in headers {
        let key = name.as_str().to_string();
        let current = merged.entry(key).or_insert_with(String::new);
        if !current.is_empty() {
            current.push_str(", ");
        }
        let value = value.to_str().unwrap_or("<non-utf8>");
        current.push_str(value);
    }
    merged
}

fn compose_cookie_header(cookie_store: CookieStore) -> Result<String, LuaError> {
    let jar = cookie_store
        .lock()
        .map_err(|_| LuaError::runtime("cookie 会话锁已损坏"))?;
    if jar.is_empty() {
        return Ok(String::new());
    }

    let mut pairs = jar
        .iter()
        .map(|(name, entry)| {
            let _attrs = entry.attrs.len();
            format!("{name}={}", entry.value)
        })
        .collect::<Vec<_>>();
    pairs.sort();
    Ok(pairs.join("; "))
}

fn apply_response_cookies(headers: &HeaderMap, cookie_store: CookieStore) -> Result<(), LuaError> {
    let mut jar = cookie_store
        .lock()
        .map_err(|_| LuaError::runtime("cookie 会话锁已损坏"))?;
    for value in &headers.get_all("set-cookie") {
        let raw = match value.to_str() {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        if let Some((name, cookie)) = parse_set_cookie_line(raw) {
            jar.insert(name, cookie);
        }
    }
    Ok(())
}

fn parse_set_cookie_line(raw: &str) -> Option<(String, CookieEntry)> {
    let mut segments = raw.split(';');
    let name_value = segments.next()?.trim();
    let (name, value) = name_value.split_once('=')?;
    if name.trim().is_empty() {
        return None;
    }

    let mut attrs = std::collections::BTreeMap::new();
    for segment in segments {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if let Some((attr_name, attr_value)) = segment.split_once('=') {
            attrs.insert(attr_name.trim().to_string(), attr_value.trim().to_string());
        } else {
            attrs.insert(segment.to_string(), "true".to_string());
        }
    }

    Some((
        name.trim().to_string(),
        CookieEntry {
            value: value.trim().to_string(),
            attrs,
        },
    ))
}

fn ensure_allowed_target(url: &Url) -> Result<(), LuaError> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(LuaError::runtime(format!(
                "http.request 仅支持 http/https，当前为：{scheme}"
            )));
        }
    }

    let host = url
        .host_str()
        .ok_or_else(|| LuaError::runtime("http.request 缺少 host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| LuaError::runtime("http.request 端口无效"))?;
    let addresses = resolve_host_ips(host, port)?;
    if addresses.is_empty() {
        return Err(LuaError::runtime("http.request 无法解析目标地址"));
    }

    for ip in addresses {
        if is_forbidden_ip(ip) {
            return Err(LuaError::runtime(format!(
                "http.request 目标地址不允许（内网/保留地址）：{}",
                ip
            )));
        }
    }

    Ok(())
}

fn resolve_host_ips(host: &str, port: u16) -> Result<Vec<IpAddr>, LuaError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }

    let socket_address = format!("{host}:{port}");
    socket_address
        .to_socket_addrs()
        .map(|iter| iter.map(|addr| addr.ip()).collect::<Vec<_>>())
        .map_err(|error| LuaError::runtime(format!("http.request DNS 解析失败：{error}")))
}

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 127
                || octets[0] == 0
                || octets[0] == 10
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 169 && octets[1] == 254)
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return true;
            }
            let first_segment = v6.segments()[0];
            (first_segment & 0xfe00) == 0xfc00 || (first_segment & 0xffc0) == 0xfe80
        }
    }
}

fn run_reqwest_blocking<T, F>(future: F) -> Result<T, String>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    let handle = thread::spawn(move || {
        let runtime = TokioRuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("初始化异步运行时失败：{error}"))?;
        runtime.block_on(future)
    });

    handle
        .join()
        .map_err(|_| "HTTP 请求线程异常退出".to_string())?
}

fn retry_backoff(base_delay: Duration, retry_attempt: usize) -> Duration {
    let base_delay = if base_delay.is_zero() {
        Duration::from_millis(100)
    } else {
        base_delay
    };
    let shift = retry_attempt.saturating_sub(1).min(8);
    base_delay.saturating_mul(1_u32 << shift)
}
