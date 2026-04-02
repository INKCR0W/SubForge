use std::fs;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::thread;
use std::time::{Duration, Instant};

use app_transport::{NetworkProfileFactory, TransportProfile};
use base64::Engine;
use mlua::{
    Error as LuaError, Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, MultiValue, Table,
    Value as LuaValue, VmState,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::runtime::Builder as TokioRuntimeBuilder;

use crate::{PluginRuntimeError, PluginRuntimeResult};

const DEFAULT_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
const DEFAULT_HOOK_STEP: u32 = 1000;
const DEFAULT_MAX_INSTRUCTIONS: u64 = 100_000_000;
const DEFAULT_NETWORK_PROFILE: &str = "standard";
const SCRIPT_HTTP_TIMEOUT_MS: u64 = 15_000;
const SCRIPT_HTTP_MAX_REQUESTS: usize = 20;
const SCRIPT_HTTP_MAX_REDIRECTS: usize = 5;
const SCRIPT_HTTP_MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
const HOOK_TIMEOUT_SENTINEL: &str = "__subforge_script_timeout__";
const HOOK_LIMIT_SENTINEL: &str = "__subforge_script_limit__";
const HTTP_REQUEST_LIMIT_SENTINEL: &str = "__subforge_http_request_limit__";
const HTTP_RESPONSE_LIMIT_SENTINEL: &str = "__subforge_http_response_limit__";
const LOG_PREFIX: &str = "subforge.lua";

const DISABLED_GLOBALS: &[&str] = &[
    "os",
    "io",
    "debug",
    "loadfile",
    "dofile",
    "require",
    "rawget",
    "rawset",
    "collectgarbage",
    "package",
];

#[derive(Debug, Clone)]
pub struct LuaSandboxConfig {
    pub memory_limit_bytes: usize,
    pub timeout: Duration,
    pub max_instructions: u64,
    pub instruction_hook_step: u32,
    pub network_profile: String,
}

impl Default for LuaSandboxConfig {
    fn default() -> Self {
        Self {
            memory_limit_bytes: DEFAULT_MEMORY_LIMIT_BYTES,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
            max_instructions: DEFAULT_MAX_INSTRUCTIONS,
            instruction_hook_step: DEFAULT_HOOK_STEP,
            network_profile: DEFAULT_NETWORK_PROFILE.to_string(),
        }
    }
}

impl LuaSandboxConfig {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_memory_limit_bytes(mut self, memory_limit_bytes: usize) -> Self {
        self.memory_limit_bytes = memory_limit_bytes;
        self
    }

    pub fn with_instruction_limit(
        mut self,
        max_instructions: u64,
        instruction_hook_step: u32,
    ) -> Self {
        self.max_instructions = max_instructions;
        self.instruction_hook_step = instruction_hook_step.max(1);
        self
    }

    pub fn with_network_profile(mut self, profile: impl Into<String>) -> Self {
        self.network_profile = profile.into();
        self
    }
}

pub struct LuaSandbox {
    lua: Lua,
    config: LuaSandboxConfig,
}

impl LuaSandbox {
    pub fn new() -> PluginRuntimeResult<Self> {
        Self::new_with_config(LuaSandboxConfig::default())
    }

    pub fn new_with_config(config: LuaSandboxConfig) -> PluginRuntimeResult<Self> {
        let lua =
            Lua::new_with(mlua::StdLib::ALL_SAFE, LuaOptions::default()).map_err(map_lua_error)?;
        lua.set_memory_limit(config.memory_limit_bytes)
            .map_err(map_lua_error)?;
        disable_globals(&lua)?;
        register_runtime_apis(&lua, &config)?;

        Ok(Self { lua, config })
    }

    pub fn exec_file(
        &self,
        path: impl AsRef<Path>,
        entry_fn: &str,
        args: &[Value],
    ) -> PluginRuntimeResult<Value> {
        let script_path = path.as_ref();
        let script_content = fs::read_to_string(script_path)?;
        self.install_limits_hook()?;

        let execution_result = (|| -> PluginRuntimeResult<Value> {
            let chunk_name = script_path.display().to_string();
            self.lua
                .load(&script_content)
                .set_name(chunk_name)
                .exec()
                .map_err(map_lua_error)?;

            let globals = self.lua.globals();
            let entrypoint: Function = globals.get(entry_fn).map_err(map_lua_error)?;
            let lua_args = pack_args(&self.lua, args)?;
            let lua_result: LuaValue = entrypoint.call(lua_args).map_err(map_lua_error)?;
            self.lua.from_value(lua_result).map_err(map_lua_error)
        })();

        self.lua.remove_hook();
        execution_result
    }

    fn install_limits_hook(&self) -> PluginRuntimeResult<()> {
        let started = Instant::now();
        let timeout = self.config.timeout;
        let max_instructions = self.config.max_instructions;
        let instruction_step = self.config.instruction_hook_step as u64;
        let executed_instructions = Arc::new(AtomicU64::new(0));
        let instruction_counter = Arc::clone(&executed_instructions);

        self.lua
            .set_hook(
                HookTriggers::new().every_nth_instruction(self.config.instruction_hook_step),
                move |_lua, _debug| {
                    if started.elapsed() >= timeout {
                        return Err(LuaError::runtime(HOOK_TIMEOUT_SENTINEL));
                    }

                    let next = instruction_counter
                        .fetch_add(instruction_step, Ordering::Relaxed)
                        .saturating_add(instruction_step);
                    if next > max_instructions {
                        return Err(LuaError::runtime(HOOK_LIMIT_SENTINEL));
                    }

                    Ok(VmState::Continue)
                },
            )
            .map_err(map_lua_error)
    }
}

#[derive(Debug, Deserialize)]
struct HttpRequestInput {
    url: String,
    method: Option<String>,
    headers: Option<std::collections::BTreeMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct HttpResponseOutput {
    status: u16,
    headers: std::collections::BTreeMap<String, String>,
    body: String,
    final_url: String,
}

fn register_runtime_apis(lua: &Lua, config: &LuaSandboxConfig) -> PluginRuntimeResult<()> {
    register_json_api(lua)?;
    register_base64_api(lua)?;
    register_time_api(lua)?;
    register_log_api(lua)?;
    register_http_api(lua, &config.network_profile)?;
    Ok(())
}

fn register_json_api(lua: &Lua) -> PluginRuntimeResult<()> {
    let json_table = lua.create_table().map_err(map_lua_error)?;
    let parse_fn = lua
        .create_function(|lua, payload: String| {
            let value: Value = serde_json::from_str(&payload)
                .map_err(|error| LuaError::runtime(format!("json.parse 失败：{error}")))?;
            lua.to_value(&value)
        })
        .map_err(map_lua_error)?;
    let stringify_fn = lua
        .create_function(|lua, payload: LuaValue| {
            let value: Value = lua.from_value(payload)?;
            serde_json::to_string(&value)
                .map_err(|error| LuaError::runtime(format!("json.stringify 失败：{error}")))
        })
        .map_err(map_lua_error)?;

    json_table.set("parse", parse_fn).map_err(map_lua_error)?;
    json_table
        .set("stringify", stringify_fn)
        .map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("json", json_table).map_err(map_lua_error)?;
    Ok(())
}

fn register_base64_api(lua: &Lua) -> PluginRuntimeResult<()> {
    let base64_table = lua.create_table().map_err(map_lua_error)?;
    let encode_fn = lua
        .create_function(|_, payload: String| {
            Ok(base64::engine::general_purpose::STANDARD.encode(payload.as_bytes()))
        })
        .map_err(map_lua_error)?;
    let decode_fn = lua
        .create_function(|_, payload: String| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(payload)
                .map_err(|error| LuaError::runtime(format!("base64.decode 失败：{error}")))?;
            String::from_utf8(bytes)
                .map_err(|error| LuaError::runtime(format!("base64.decode 非 UTF-8 文本：{error}")))
        })
        .map_err(map_lua_error)?;

    base64_table
        .set("encode", encode_fn)
        .map_err(map_lua_error)?;
    base64_table
        .set("decode", decode_fn)
        .map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("base64", base64_table).map_err(map_lua_error)?;
    Ok(())
}

fn register_time_api(lua: &Lua) -> PluginRuntimeResult<()> {
    let time_table = lua.create_table().map_err(map_lua_error)?;
    let now_fn = lua
        .create_function(|_, ()| {
            let now = OffsetDateTime::now_utc();
            now.format(&Rfc3339)
                .map_err(|error| LuaError::runtime(format!("time.now 格式化失败：{error}")))
        })
        .map_err(map_lua_error)?;
    time_table.set("now", now_fn).map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("time", time_table).map_err(map_lua_error)?;
    Ok(())
}

fn register_log_api(lua: &Lua) -> PluginRuntimeResult<()> {
    let log_table = lua.create_table().map_err(map_lua_error)?;
    let info_fn = lua
        .create_function(|_, message: String| {
            eprintln!("INFO: {} {}", LOG_PREFIX, message);
            Ok(())
        })
        .map_err(map_lua_error)?;
    let warn_fn = lua
        .create_function(|_, message: String| {
            eprintln!("WARN: {} {}", LOG_PREFIX, message);
            Ok(())
        })
        .map_err(map_lua_error)?;
    let error_fn = lua
        .create_function(|_, message: String| {
            eprintln!("ERROR: {} {}", LOG_PREFIX, message);
            Ok(())
        })
        .map_err(map_lua_error)?;

    log_table.set("info", info_fn).map_err(map_lua_error)?;
    log_table.set("warn", warn_fn).map_err(map_lua_error)?;
    log_table.set("error", error_fn).map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("log", log_table).map_err(map_lua_error)?;
    Ok(())
}

fn register_http_api(lua: &Lua, network_profile: &str) -> PluginRuntimeResult<()> {
    let transport_profile = NetworkProfileFactory::create(network_profile)
        .map_err(|error| PluginRuntimeError::ScriptRuntime(error.to_string()))?;
    let request_counter = Arc::new(AtomicUsize::new(0));
    let http_table = lua.create_table().map_err(map_lua_error)?;

    let request_fn = lua
        .create_function(move |lua, request_table: Table| {
            let next = request_counter
                .fetch_add(1, AtomicOrdering::Relaxed)
                .saturating_add(1);
            if next > SCRIPT_HTTP_MAX_REQUESTS {
                return Err(LuaError::runtime(HTTP_REQUEST_LIMIT_SENTINEL));
            }

            let request: HttpRequestInput = lua.from_value(LuaValue::Table(request_table))?;
            let response = execute_http_request(transport_profile.as_ref(), request)?;
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
    let headers = build_request_headers(transport_profile, request.headers.as_ref())?;

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

fn disable_globals(lua: &Lua) -> PluginRuntimeResult<()> {
    let globals = lua.globals();
    for name in DISABLED_GLOBALS {
        globals.raw_remove(*name).map_err(map_lua_error)?;
    }
    Ok(())
}

fn pack_args(lua: &Lua, args: &[Value]) -> PluginRuntimeResult<MultiValue> {
    let mut lua_values = Vec::with_capacity(args.len());
    for arg in args {
        let value = lua.to_value(arg).map_err(map_lua_error)?;
        lua_values.push(value);
    }
    Ok(MultiValue::from_vec(lua_values))
}

fn map_lua_error(error: LuaError) -> PluginRuntimeError {
    if runtime_message_contains(&error, HOOK_TIMEOUT_SENTINEL) {
        return PluginRuntimeError::ScriptTimeout("脚本执行超过超时上限".to_string());
    }

    if runtime_message_contains(&error, HOOK_LIMIT_SENTINEL) {
        return PluginRuntimeError::ScriptLimit("脚本指令数超过上限".to_string());
    }

    if runtime_message_contains(&error, HTTP_REQUEST_LIMIT_SENTINEL) {
        return PluginRuntimeError::ScriptLimit(format!(
            "http.request 次数超过上限：{}",
            SCRIPT_HTTP_MAX_REQUESTS
        ));
    }

    if runtime_message_contains(&error, HTTP_RESPONSE_LIMIT_SENTINEL) {
        return PluginRuntimeError::ScriptLimit(format!(
            "http.request 响应体超过上限：{} bytes",
            SCRIPT_HTTP_MAX_RESPONSE_BYTES
        ));
    }

    if let Some(message) = memory_error_message(&error) {
        return PluginRuntimeError::ScriptLimit(format!("脚本内存超过上限：{message}"));
    }

    PluginRuntimeError::ScriptRuntime(error.to_string())
}

fn runtime_message_contains(error: &LuaError, marker: &str) -> bool {
    match error {
        LuaError::RuntimeError(message) => message.contains(marker),
        LuaError::CallbackError { cause, .. }
        | LuaError::WithContext { cause, .. }
        | LuaError::BadArgument { cause, .. } => runtime_message_contains(cause.as_ref(), marker),
        _ => false,
    }
}

fn memory_error_message(error: &LuaError) -> Option<&str> {
    match error {
        LuaError::MemoryError(message) => Some(message.as_str()),
        LuaError::CallbackError { cause, .. }
        | LuaError::WithContext { cause, .. }
        | LuaError::BadArgument { cause, .. } => memory_error_message(cause.as_ref()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::{LuaSandbox, LuaSandboxConfig};
    use crate::PluginRuntimeError;

    #[test]
    fn executes_basic_arithmetic_script() {
        let script_path = write_temp_script(
            "basic-exec",
            r#"
                function run(a, b)
                    return { sum = a + b, product = a * b }
                end
            "#,
        );
        let sandbox = LuaSandbox::new().expect("沙箱初始化应成功");
        let result = sandbox
            .exec_file(&script_path, "run", &[json!(2), json!(3)])
            .expect("脚本应执行成功");

        assert_eq!(result["sum"], json!(5));
        assert_eq!(result["product"], json!(6));
        cleanup_script(&script_path);
    }

    #[test]
    fn disallows_dangerous_lua_capabilities() {
        let script_path = write_temp_script(
            "disabled-capabilities",
            r#"
                function run()
                    return {
                        os_execute = pcall(function() return os.execute("echo 1") end),
                        io_open = pcall(function() return io.open("test.txt", "r") end),
                        require_mod = pcall(function() return require("x") end),
                        debug_info = pcall(function() return debug.getinfo(1) end),
                        rawget_call = pcall(function() return rawget({},"k") end)
                    }
                end
            "#,
        );
        let sandbox = LuaSandbox::new().expect("沙箱初始化应成功");
        let result = sandbox
            .exec_file(&script_path, "run", &[])
            .expect("脚本应可执行并返回结果");

        assert_eq!(result["os_execute"], json!(false));
        assert_eq!(result["io_open"], json!(false));
        assert_eq!(result["require_mod"], json!(false));
        assert_eq!(result["debug_info"], json!(false));
        assert_eq!(result["rawget_call"], json!(false));
        cleanup_script(&script_path);
    }

    #[test]
    fn returns_script_limit_when_memory_limit_exceeded() {
        let script_path = write_temp_script(
            "memory-limit",
            r#"
                function run()
                    local t = {}
                    for i = 1, 200000 do
                        t[i] = i
                    end
                    return #t
                end
            "#,
        );
        let config = LuaSandboxConfig::default()
            .with_memory_limit_bytes(128 * 1024)
            .with_timeout(Duration::from_secs(2))
            .with_instruction_limit(1_000_000_000, 1000);
        let sandbox = LuaSandbox::new_with_config(config).expect("沙箱初始化应成功");
        let error = sandbox
            .exec_file(&script_path, "run", &[])
            .expect_err("应触发内存限制");

        assert!(matches!(error, PluginRuntimeError::ScriptLimit(_)));
        assert_eq!(error.code(), "E_SCRIPT_LIMIT");
        cleanup_script(&script_path);
    }

    #[test]
    fn returns_script_timeout_on_infinite_loop() {
        let script_path = write_temp_script(
            "timeout-limit",
            r#"
                function run()
                    while true do
                    end
                end
            "#,
        );
        let config = LuaSandboxConfig::default()
            .with_timeout(Duration::from_millis(80))
            .with_instruction_limit(u64::MAX / 2, 1000);
        let sandbox = LuaSandbox::new_with_config(config).expect("沙箱初始化应成功");
        let error = sandbox
            .exec_file(&script_path, "run", &[])
            .expect_err("应触发超时限制");

        assert!(matches!(error, PluginRuntimeError::ScriptTimeout(_)));
        assert_eq!(error.code(), "E_SCRIPT_TIMEOUT");
        cleanup_script(&script_path);
    }

    #[test]
    fn returns_script_limit_on_instruction_budget_exceeded() {
        let script_path = write_temp_script(
            "instruction-limit",
            r#"
                function run()
                    local sum = 0
                    for i = 1, 10000000 do
                        sum = sum + i
                    end
                    return sum
                end
            "#,
        );
        let config = LuaSandboxConfig::default()
            .with_timeout(Duration::from_secs(3))
            .with_instruction_limit(10_000, 1000);
        let sandbox = LuaSandbox::new_with_config(config).expect("沙箱初始化应成功");
        let error = sandbox
            .exec_file(&script_path, "run", &[])
            .expect_err("应触发指令预算限制");

        assert!(matches!(error, PluginRuntimeError::ScriptLimit(_)));
        assert_eq!(error.code(), "E_SCRIPT_LIMIT");
        cleanup_script(&script_path);
    }

    #[test]
    fn exposes_json_base64_time_and_log_apis() {
        let script_path = write_temp_script(
            "runtime-apis",
            r#"
                function run()
                    local parsed = json.parse("{\"name\":\"subforge\",\"count\":2}")
                    local encoded = base64.encode("hello")
                    local decoded = base64.decode(encoded)
                    local now = time.now()
                    log.info("runtime api smoke test")
                    return {
                        parsed_name = parsed.name,
                        parsed_count = parsed.count,
                        encoded = encoded,
                        decoded = decoded,
                        now = now
                    }
                end
            "#,
        );
        let sandbox = LuaSandbox::new().expect("沙箱初始化应成功");
        let result = sandbox
            .exec_file(&script_path, "run", &[])
            .expect("运行时 API 应可调用");

        assert_eq!(result["parsed_name"], json!("subforge"));
        assert_eq!(result["parsed_count"], json!(2));
        assert_eq!(result["encoded"], json!("aGVsbG8="));
        assert_eq!(result["decoded"], json!("hello"));
        let now = result["now"].as_str().expect("time.now 应返回字符串");
        assert!(
            now.contains('T') && now.ends_with('Z'),
            "time.now 应返回 UTC RFC3339 时间字符串"
        );
        cleanup_script(&script_path);
    }

    #[test]
    fn enforces_http_request_count_limit() {
        let script_path = write_temp_script(
            "http-limit",
            r#"
                function run()
                    for i = 1, 20 do
                        pcall(function()
                            http.request({ url = "http://127.0.0.1:18118/health" })
                        end)
                    end
                    return http.request({ url = "http://127.0.0.1:18118/health" })
                end
            "#,
        );
        let sandbox = LuaSandbox::new().expect("沙箱初始化应成功");
        let error = sandbox
            .exec_file(&script_path, "run", &[])
            .expect_err("第 21 次请求应触发上限");

        assert!(matches!(error, PluginRuntimeError::ScriptLimit(_)));
        assert_eq!(error.code(), "E_SCRIPT_LIMIT");
        cleanup_script(&script_path);
    }

    #[test]
    fn blocks_loopback_ssrf_target() {
        let script_path = write_temp_script(
            "ssrf-loopback",
            r#"
                function run()
                    return http.request({ url = "http://127.0.0.1:18118/health" })
                end
            "#,
        );
        let sandbox = LuaSandbox::new().expect("沙箱初始化应成功");
        let error = sandbox
            .exec_file(&script_path, "run", &[])
            .expect_err("访问 loopback 应被拦截");

        assert!(matches!(error, PluginRuntimeError::ScriptRuntime(_)));
        assert_eq!(error.code(), "E_SCRIPT_RUNTIME");
        cleanup_script(&script_path);
    }

    #[test]
    fn blocks_cloud_metadata_ssrf_target() {
        let script_path = write_temp_script(
            "ssrf-metadata",
            r#"
                function run()
                    return http.request({ url = "http://169.254.169.254/latest/meta-data" })
                end
            "#,
        );
        let sandbox = LuaSandbox::new().expect("沙箱初始化应成功");
        let error = sandbox
            .exec_file(&script_path, "run", &[])
            .expect_err("访问云元数据地址应被拦截");

        assert!(matches!(error, PluginRuntimeError::ScriptRuntime(_)));
        assert_eq!(error.code(), "E_SCRIPT_RUNTIME");
        cleanup_script(&script_path);
    }

    #[test]
    fn blocks_dns_rebinding_to_private_ip() {
        let script_path = write_temp_script(
            "dns-rebinding",
            r#"
                function run()
                    return http.request({ url = "http://localhost:18118/health" })
                end
            "#,
        );
        let sandbox = LuaSandbox::new().expect("沙箱初始化应成功");
        let error = sandbox
            .exec_file(&script_path, "run", &[])
            .expect_err("域名解析到内网地址时应被拦截");

        assert!(matches!(error, PluginRuntimeError::ScriptRuntime(_)));
        assert_eq!(error.code(), "E_SCRIPT_RUNTIME");
        cleanup_script(&script_path);
    }

    fn write_temp_script(prefix: &str, content: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间异常")
            .as_nanos();
        let script_path =
            std::env::temp_dir().join(format!("subforge-lua-sandbox-{prefix}-{nanos}.lua"));
        fs::write(&script_path, content).expect("写入脚本文件失败");
        script_path
    }

    fn cleanup_script(path: &Path) {
        let _ = fs::remove_file(path);
    }
}
