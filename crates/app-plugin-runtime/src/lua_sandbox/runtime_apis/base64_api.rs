use base64::Engine;
use mlua::{Error as LuaError, Lua};

use super::map_lua_error;
use crate::PluginRuntimeResult;

pub(super) fn register_base64_api(lua: &Lua) -> PluginRuntimeResult<()> {
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
