use mlua::{Lua, LuaSerdeExt};
use scraper::{Html, Selector};

use super::map_lua_error;
use crate::PluginRuntimeResult;

pub(super) fn register_html_api(lua: &Lua) -> PluginRuntimeResult<()> {
    let html_table = lua.create_table().map_err(map_lua_error)?;
    let query_fn = lua
        .create_function(|lua, (raw_html, selector): (String, String)| {
            let selector = Selector::parse(selector.trim()).map_err(|error| {
                mlua::Error::runtime(format!("html.query selector 非法：{error}"))
            })?;
            let document = Html::parse_document(&raw_html);
            let mut matches = Vec::new();
            for node in document.select(&selector) {
                let text = normalize_html_text(node.text().collect::<Vec<_>>().join(" "));
                matches.push(text);
            }
            lua.to_value(&matches)
        })
        .map_err(map_lua_error)?;
    html_table.set("query", query_fn).map_err(map_lua_error)?;

    let globals = lua.globals();
    globals.set("html", html_table).map_err(map_lua_error)?;
    Ok(())
}

fn normalize_html_text(input: String) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}
