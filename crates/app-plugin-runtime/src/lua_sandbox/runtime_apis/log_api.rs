use std::sync::Arc;

use mlua::Lua;

use super::LOG_PREFIX;
use super::map_lua_error;
use crate::PluginRuntimeResult;
use crate::lua_sandbox::{RuntimeLogLevel, RuntimeLogSink};

pub(super) fn register_log_api(
    lua: &Lua,
    log_sink: Option<Arc<dyn RuntimeLogSink>>,
) -> PluginRuntimeResult<()> {
    let log_table = lua.create_table().map_err(map_lua_error)?;
    let info_sink = log_sink.clone();
    let info_fn = lua
        .create_function(move |_, message: String| {
            if let Some(sink) = info_sink.as_ref() {
                sink.emit(RuntimeLogLevel::Info, &message);
            }
            eprintln!("INFO: {} {}", LOG_PREFIX, message);
            Ok(())
        })
        .map_err(map_lua_error)?;
    let warn_sink = log_sink.clone();
    let warn_fn = lua
        .create_function(move |_, message: String| {
            if let Some(sink) = warn_sink.as_ref() {
                sink.emit(RuntimeLogLevel::Warn, &message);
            }
            eprintln!("WARN: {} {}", LOG_PREFIX, message);
            Ok(())
        })
        .map_err(map_lua_error)?;
    let error_sink = log_sink;
    let error_fn = lua
        .create_function(move |_, message: String| {
            if let Some(sink) = error_sink.as_ref() {
                sink.emit(RuntimeLogLevel::Error, &message);
            }
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
