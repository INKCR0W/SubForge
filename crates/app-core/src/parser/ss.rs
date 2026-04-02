use std::collections::BTreeMap;

use app_common::{ProxyNode, ProxyProtocol, ProxyTransport, TlsConfig};
use serde_json::Value;

use crate::CoreError;
use crate::CoreResult;

use super::{build_proxy_node, parse_host_port, split_fragment, try_decode_base64_text};

pub(crate) fn parse_ss_uri(line: &str, source_id: &str, updated_at: &str) -> CoreResult<ProxyNode> {
    let raw = &line["ss://".len()..];
    let (without_fragment, name) = split_fragment(raw);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);

    let (credential_part, host_part) = if let Some((cred, host)) = without_query.rsplit_once('@') {
        (cred.to_string(), host.to_string())
    } else {
        let decoded = try_decode_base64_text(without_query)
            .ok_or_else(|| CoreError::SubscriptionParse("ss URI 缺少 @server:port".to_string()))?;
        let (cred, host) = decoded
            .rsplit_once('@')
            .ok_or_else(|| CoreError::SubscriptionParse("ss URI 凭证无法解析".to_string()))?;
        (cred.to_string(), host.to_string())
    };

    let credential_decoded =
        try_decode_base64_text(&credential_part).unwrap_or_else(|| credential_part.clone());
    let (cipher, password) = credential_decoded.split_once(':').ok_or_else(|| {
        CoreError::SubscriptionParse("ss URI 凭证必须为 method:password".to_string())
    })?;
    let (server, port) = parse_host_port(&host_part)?;

    let mut extra = BTreeMap::new();
    extra.insert("cipher".to_string(), Value::String(cipher.to_string()));
    extra.insert("password".to_string(), Value::String(password.to_string()));

    Ok(build_proxy_node(
        source_id,
        name.unwrap_or_else(|| format!("ss-{server}:{port}")),
        ProxyProtocol::Ss,
        server,
        port,
        ProxyTransport::Tcp,
        TlsConfig {
            enabled: false,
            server_name: None,
        },
        extra,
        updated_at,
    ))
}
