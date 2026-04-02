use mlua::Lua;

use super::LOG_PREFIX;
use super::map_lua_error;
use crate::PluginRuntimeResult;

pub(super) fn register_log_api(lua: &Lua) -> PluginRuntimeResult<()> {
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
