use std::sync::Arc;

use mlua::{Error as LuaError, Lua, Table};

use super::{CookieEntry, CookieStore, map_lua_error, parse_cookie_attrs};
use crate::PluginRuntimeResult;

pub(super) fn register_cookie_api(lua: &Lua, cookie_store: CookieStore) -> PluginRuntimeResult<()> {
    let cookie_table = lua.create_table().map_err(map_lua_error)?;

    let get_store = Arc::clone(&cookie_store);
    let get_fn = lua
        .create_function(move |_, name: String| {
            let jar = get_store
                .lock()
                .map_err(|_| LuaError::runtime("cookie.get 无法获取会话锁"))?;
            Ok(jar.get(name.trim()).map(|entry| entry.value.clone()))
        })
        .map_err(map_lua_error)?;

    let set_store = Arc::clone(&cookie_store);
    let set_fn = lua
        .create_function(
            move |_, (name, value, attrs): (String, String, Option<Table>)| {
                let mut jar = set_store
                    .lock()
                    .map_err(|_| LuaError::runtime("cookie.set 无法获取会话锁"))?;
                jar.insert(
                    name.trim().to_string(),
                    CookieEntry {
                        value,
                        attrs: parse_cookie_attrs(attrs)?,
                    },
                );
                Ok(())
            },
        )
        .map_err(map_lua_error)?;

    cookie_table.set("get", get_fn).map_err(map_lua_error)?;
    cookie_table.set("set", set_fn).map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("cookie", cookie_table).map_err(map_lua_error)?;
    Ok(())
}
