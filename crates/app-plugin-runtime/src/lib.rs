//! app-plugin-runtime：插件加载、校验与脚本运行时。

mod error;
mod loader;
mod lua_sandbox;
#[cfg(test)]
mod tests;

pub use error::{PluginRuntimeError, PluginRuntimeResult};
pub use loader::{LoadedPlugin, PluginLoader};
pub use lua_sandbox::{
    LuaSandbox, LuaSandboxConfig, RuntimeLogLevel, RuntimeLogSink,
    ensure_http_target_allowed_for_plugin, http_target_redirect_policy_for_plugin,
};
