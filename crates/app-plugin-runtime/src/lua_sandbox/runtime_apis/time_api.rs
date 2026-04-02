use mlua::{Error as LuaError, Lua};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::map_lua_error;
use crate::PluginRuntimeResult;

pub(super) fn register_time_api(lua: &Lua) -> PluginRuntimeResult<()> {
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
