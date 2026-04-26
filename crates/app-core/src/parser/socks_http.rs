use std::collections::BTreeMap;

use app_common::{ProxyNode, ProxyProtocol, ProxyTransport, TlsConfig};
use reqwest::Url;
use serde_json::Value;

use crate::{CoreError, CoreResult};

use super::{build_proxy_node, decode_percent_encoded};

pub(crate) fn parse_socks5_uri(
    line: &str,
    source_id: &str,
    updated_at: &str,
) -> CoreResult<ProxyNode> {
    let url = Url::parse(line)
        .map_err(|error| CoreError::SubscriptionParse(format!("socks5 URI 非法：{error}")))?;
    let server = url
        .host_str()
        .ok_or_else(|| CoreError::SubscriptionParse("socks5 URI 缺少 host".to_string()))?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| CoreError::SubscriptionParse("socks5 URI 缺少端口".to_string()))?;
    let name = decode_percent_encoded(
        url.fragment()
            .filter(|value| !value.is_empty())
            .unwrap_or("socks5"),
    );

    let mut extra = BTreeMap::new();
    if !url.username().is_empty() {
        extra.insert(
            "username".to_string(),
            Value::String(url.username().to_string()),
        );
    }
    if let Some(password) = url.password().filter(|value| !value.is_empty()) {
        extra.insert("password".to_string(), Value::String(password.to_string()));
    }

    Ok(build_proxy_node(
        source_id,
        name,
        ProxyProtocol::Socks5,
        server,
        port,
        ProxyTransport::Tcp,
        TlsConfig {
            enabled: line.starts_with("socks5+tls://"),
            server_name: None,
        },
        extra,
        updated_at,
    ))
}

pub(crate) fn parse_http_proxy_uri(
    line: &str,
    source_id: &str,
    updated_at: &str,
) -> CoreResult<ProxyNode> {
    let url = Url::parse(line)
        .map_err(|error| CoreError::SubscriptionParse(format!("http(s) URI 非法：{error}")))?;
    let server = url
        .host_str()
        .ok_or_else(|| CoreError::SubscriptionParse("http(s) URI 缺少 host".to_string()))?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| CoreError::SubscriptionParse("http(s) URI 缺少端口".to_string()))?;
    let name = decode_percent_encoded(
        url.fragment()
            .filter(|value| !value.is_empty())
            .unwrap_or("http"),
    );

    let mut extra = BTreeMap::new();
    if !url.username().is_empty() {
        extra.insert(
            "username".to_string(),
            Value::String(url.username().to_string()),
        );
    }
    if let Some(password) = url.password().filter(|value| !value.is_empty()) {
        extra.insert("password".to_string(), Value::String(password.to_string()));
    }

    Ok(build_proxy_node(
        source_id,
        name,
        ProxyProtocol::Http,
        server,
        port,
        ProxyTransport::Tcp,
        TlsConfig {
            enabled: url.scheme().eq_ignore_ascii_case("https"),
            server_name: None,
        },
        extra,
        updated_at,
    ))
}
