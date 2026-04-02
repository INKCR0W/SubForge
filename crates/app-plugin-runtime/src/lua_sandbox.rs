use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mlua::{
    Error as LuaError, Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, MultiValue, StdLib,
    Value as LuaValue, VmState,
};
use serde_json::Value;

use crate::{PluginRuntimeError, PluginRuntimeResult};

const DEFAULT_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
const DEFAULT_HOOK_STEP: u32 = 1000;
const DEFAULT_MAX_INSTRUCTIONS: u64 = 100_000_000;
const HOOK_TIMEOUT_SENTINEL: &str = "__subforge_script_timeout__";
const HOOK_LIMIT_SENTINEL: &str = "__subforge_script_limit__";

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
}

impl Default for LuaSandboxConfig {
    fn default() -> Self {
        Self {
            memory_limit_bytes: DEFAULT_MEMORY_LIMIT_BYTES,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
            max_instructions: DEFAULT_MAX_INSTRUCTIONS,
            instruction_hook_step: DEFAULT_HOOK_STEP,
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
        let lua = Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default()).map_err(map_lua_error)?;
        lua.set_memory_limit(config.memory_limit_bytes)
            .map_err(map_lua_error)?;
        disable_globals(&lua)?;

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
