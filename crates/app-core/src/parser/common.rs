use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use app_common::{ProxyNode, ProxyProtocol, ProxyTransport, TlsConfig};
use serde_json::Value;

use crate::{CoreError, CoreResult};

pub(crate) fn split_fragment(raw: &str) -> (&str, Option<String>) {
    if let Some((value, fragment)) = raw.split_once('#') {
        (value, Some(fragment.to_string()))
    } else {
        (raw, None)
    }
}

pub(crate) fn parse_host_port(raw: &str) -> CoreResult<(String, u16)> {
    if let Some(stripped) = raw.strip_prefix('[') {
        let (host, remainder) = stripped
            .split_once(']')
            .ok_or_else(|| CoreError::SubscriptionParse(format!("host 非法：{raw}")))?;
        let port = remainder
            .strip_prefix(':')
            .ok_or_else(|| CoreError::SubscriptionParse(format!("端口缺失：{raw}")))?
            .parse::<u16>()
            .map_err(|error| CoreError::SubscriptionParse(format!("端口非法：{error}")))?;
        return Ok((host.to_string(), port));
    }

    let (host, port) = raw
        .rsplit_once(':')
        .ok_or_else(|| CoreError::SubscriptionParse(format!("host:port 解析失败：{raw}")))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| CoreError::SubscriptionParse(format!("端口非法：{error}")))?;
    Ok((host.to_string(), port))
}

pub(crate) fn map_transport(raw: Option<String>) -> ProxyTransport {
    match raw.as_deref() {
        Some("ws") => ProxyTransport::Ws,
        Some("grpc") => ProxyTransport::Grpc,
        Some("h2") => ProxyTransport::H2,
        Some("quic") => ProxyTransport::Quic,
        _ => ProxyTransport::Tcp,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_proxy_node(
    source_id: &str,
    name: String,
    protocol: ProxyProtocol,
    server: String,
    port: u16,
    transport: ProxyTransport,
    tls: TlsConfig,
    extra: BTreeMap<String, Value>,
    updated_at: &str,
) -> ProxyNode {
    ProxyNode {
        id: build_proxy_node_id(
            source_id,
            &protocol,
            &server,
            port,
            &name,
            extra.get("uuid").or_else(|| extra.get("password")),
        ),
        name,
        protocol,
        server,
        port,
        transport,
        tls,
        extra,
        source_id: source_id.to_string(),
        tags: Vec::new(),
        region: None,
        updated_at: updated_at.to_string(),
    }
}

pub(crate) fn build_proxy_node_id(
    source_id: &str,
    protocol: &ProxyProtocol,
    server: &str,
    port: u16,
    name: &str,
    credential: Option<&Value>,
) -> String {
    let mut hasher = DefaultHasher::new();
    source_id.hash(&mut hasher);
    protocol.hash(&mut hasher);
    server.hash(&mut hasher);
    port.hash(&mut hasher);
    name.hash(&mut hasher);
    if let Some(value) = credential {
        value.to_string().hash(&mut hasher);
    }
    format!("node-{:016x}", hasher.finish())
}
