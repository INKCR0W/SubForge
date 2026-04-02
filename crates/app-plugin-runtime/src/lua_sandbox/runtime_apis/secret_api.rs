use std::sync::Arc;

use app_secrets::SecretStore;
use mlua::Lua;

use super::map_lua_error;
use super::map_secret_error;
use crate::PluginRuntimeResult;

pub(super) fn register_secret_api(
    lua: &Lua,
    secret_store: Arc<dyn SecretStore>,
    secret_scope: String,
) -> PluginRuntimeResult<()> {
    let secret_table = lua.create_table().map_err(map_lua_error)?;

    let get_store = Arc::clone(&secret_store);
    let get_scope = secret_scope.clone();
    let get_fn = lua
        .create_function(move |_, key: String| {
            let secret = get_store
                .get(&get_scope, key.trim())
                .map_err(|error| map_secret_error("secret.get", error))?;
            Ok(secret.as_str().to_string())
        })
        .map_err(map_lua_error)?;

    let set_store = Arc::clone(&secret_store);
    let set_scope = secret_scope;
    let set_fn = lua
        .create_function(move |_, (key, value): (String, String)| {
            set_store
                .set(&set_scope, key.trim(), value.as_str())
                .map_err(|error| map_secret_error("secret.set", error))?;
            Ok(())
        })
        .map_err(map_lua_error)?;

    secret_table.set("get", get_fn).map_err(map_lua_error)?;
    secret_table.set("set", set_fn).map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("secret", secret_table).map_err(map_lua_error)?;
    Ok(())
}
