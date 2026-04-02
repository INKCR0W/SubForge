use mlua::{Lua, LuaSerdeExt, Value as LuaValue};
use serde_json::Value;

use super::map_lua_error;
use crate::PluginRuntimeResult;

pub(super) fn register_json_api(lua: &Lua) -> PluginRuntimeResult<()> {
    let json_table = lua.create_table().map_err(map_lua_error)?;
    let parse_fn = lua
        .create_function(|lua, payload: String| {
            let value: Value = serde_json::from_str(&payload)
                .map_err(|error| mlua::Error::runtime(format!("json.parse 失败：{error}")))?;
            lua.to_value(&value)
        })
        .map_err(map_lua_error)?;
    let stringify_fn = lua
        .create_function(|lua, payload: LuaValue| {
            let value: Value = lua.from_value(payload)?;
            serde_json::to_string(&value)
                .map_err(|error| mlua::Error::runtime(format!("json.stringify 失败：{error}")))
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
