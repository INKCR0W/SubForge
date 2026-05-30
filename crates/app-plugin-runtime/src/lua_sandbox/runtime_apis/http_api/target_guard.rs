use std::net::{IpAddr, ToSocketAddrs};

use mlua::Error as LuaError;
use reqwest::Url;
use reqwest::redirect::Policy;

use crate::{PluginRuntimeError, PluginRuntimeResult};

pub(super) fn redirect_policy(max_redirects: usize) -> Policy {
    let default_policy = Policy::limited(max_redirects);
    Policy::custom(move |attempt| {
        match ensure_allowed_redirect_chain(attempt.url(), attempt.previous()) {
            Ok(()) => default_policy.redirect(attempt),
            Err(error) => attempt.error(error.to_string()),
        }
    })
}

pub(super) fn ensure_allowed_target(url: &Url) -> Result<(), LuaError> {
    ensure_http_target_allowed_for_plugin(url).map_err(plugin_runtime_error_to_lua_error)
}

pub fn ensure_http_target_allowed_for_plugin(url: &Url) -> PluginRuntimeResult<()> {
    ensure_allowed_redirect_chain(url, &[])
}

pub fn http_target_redirect_policy_for_plugin(max_redirects: usize) -> Policy {
    redirect_policy(max_redirects)
}

fn plugin_runtime_error_to_lua_error(error: PluginRuntimeError) -> LuaError {
    match error {
        PluginRuntimeError::ScriptRuntime(message) => LuaError::runtime(message),
        other => LuaError::runtime(other.to_string()),
    }
}

fn ensure_allowed_redirect_chain(url: &Url, previous: &[Url]) -> PluginRuntimeResult<()> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(PluginRuntimeError::ScriptRuntime(format!(
                "http.request 仅支持 http/https，当前为：{scheme}"
            )));
        }
    }

    let host = url
        .host_str()
        .ok_or_else(|| PluginRuntimeError::ScriptRuntime("http.request 缺少 host".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| PluginRuntimeError::ScriptRuntime("http.request 端口无效".to_string()))?;
    let addresses = resolve_host_ips(host, port)?;
    if addresses.is_empty() {
        return Err(PluginRuntimeError::ScriptRuntime(
            "http.request 无法解析目标地址".to_string(),
        ));
    }

    for ip in addresses {
        if is_forbidden_ip(ip) {
            if previous.is_empty() {
                return Err(PluginRuntimeError::ScriptRuntime(format!(
                    "http.request 目标地址不允许（内网/保留地址）：{}",
                    ip
                )));
            }
            return Err(PluginRuntimeError::ScriptRuntime(format!(
                "http.request 重定向目标地址不允许（内网/保留地址）：{}，url={}，hop={}",
                ip,
                url,
                previous.len()
            )));
        }
    }

    Ok(())
}

fn resolve_host_ips(host: &str, port: u16) -> PluginRuntimeResult<Vec<IpAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }

    let socket_address = format!("{host}:{port}");
    socket_address
        .to_socket_addrs()
        .map(|iter| iter.map(|addr| addr.ip()).collect::<Vec<_>>())
        .map_err(|error| {
            PluginRuntimeError::ScriptRuntime(format!("http.request DNS 解析失败：{error}"))
        })
}

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 127
                || octets[0] == 0
                || octets[0] == 10
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 169 && octets[1] == 254)
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return true;
            }
            let first_segment = v6.segments()[0];
            (first_segment & 0xfe00) == 0xfc00 || (first_segment & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_public_initial_target() {
        let url = Url::parse("https://example.com/path").expect("url 解析应成功");
        let result = ensure_allowed_target(&url);
        assert!(result.is_ok(), "公网地址应允许访问");
    }

    #[test]
    fn blocks_forbidden_redirect_hop_target() {
        let redirect_target = Url::parse("http://127.0.0.1:18118/health").expect("url 解析应成功");
        let previous = vec![Url::parse("https://example.com/start").expect("url 解析应成功")];

        let error = ensure_allowed_redirect_chain(&redirect_target, &previous)
            .expect_err("重定向 hop 指向内网地址应被拦截");
        let message = error.to_string();
        assert!(
            message.contains("重定向目标地址不允许") && message.contains("hop=1"),
            "错误信息应标识重定向 hop 被拦截"
        );
    }
}
