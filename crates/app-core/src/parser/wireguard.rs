use std::collections::BTreeMap;

use app_common::{ProxyNode, ProxyProtocol, ProxyTransport, TlsConfig};
use reqwest::Url;
use serde_json::Value;

use crate::CoreError;
use crate::CoreResult;

use super::decode_percent_encoded;

pub(crate) fn parse_wireguard_uri(
    line: &str,
    source_id: &str,
    updated_at: &str,
) -> CoreResult<ProxyNode> {
    let url = Url::parse(line)
        .map_err(|error| CoreError::SubscriptionParse(format!("wireguard URI 非法：{error}")))?;
    let server = url
        .host_str()
        .ok_or_else(|| CoreError::SubscriptionParse("wireguard URI 缺少 host".to_string()))?
        .to_string();
    let port = url.port_or_known_default().unwrap_or(51820);

    let mut extra = BTreeMap::new();
    if !url.username().is_empty() {
        extra.insert(
            "private_key".to_string(),
            Value::String(url.username().to_string()),
        );
    }

    let query_pairs = url.query_pairs().collect::<Vec<_>>();
    insert_optional_string(
        &mut extra,
        "public_key",
        query_value(&query_pairs, &["publickey", "public_key"]),
    );
    insert_optional_string(
        &mut extra,
        "preshared_key",
        query_value(&query_pairs, &["presharedkey", "preshared_key"]),
    );
    insert_optional_string(
        &mut extra,
        "reserved",
        query_value(&query_pairs, &["reserved"]),
    );
    insert_optional_u16(&mut extra, "mtu", query_u16(&query_pairs, &["mtu"]));

    let mut local_address_values = query_multi_values(&query_pairs, &["address", "local_address"]);
    if local_address_values.is_empty() {
        local_address_values.push("172.16.0.2/32".to_string());
    }
    extra.insert(
        "local_address".to_string(),
        Value::Array(
            local_address_values
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );

    let mut peer_values = query_multi_values(&query_pairs, &["peer"]);
    if peer_values.is_empty() {
        peer_values.push(format!("{server}:{port}"));
    }
    extra.insert(
        "peers".to_string(),
        Value::Array(peer_values.into_iter().map(Value::String).collect()),
    );

    let name = decode_percent_encoded(
        url.fragment()
            .filter(|value| !value.is_empty())
            .unwrap_or("wireguard"),
    );

    Ok(ProxyNode {
        id: super::build_proxy_node_id(
            source_id,
            &ProxyProtocol::WireGuard,
            &server,
            port,
            &name,
            extra.get("private_key").or_else(|| extra.get("public_key")),
        ),
        name,
        protocol: ProxyProtocol::WireGuard,
        server,
        port,
        transport: ProxyTransport::Quic,
        tls: TlsConfig {
            enabled: false,
            server_name: None,
        },
        extra,
        source_id: source_id.to_string(),
        tags: Vec::new(),
        region: None,
        updated_at: updated_at.to_string(),
    })
}

fn query_value(
    query_pairs: &[(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)],
    keys: &[&str],
) -> Option<String> {
    query_pairs
        .iter()
        .find_map(|(key, value)| {
            if keys
                .iter()
                .any(|candidate| key.eq_ignore_ascii_case(candidate))
            {
                Some(decode_percent_encoded(value.trim()))
            } else {
                None
            }
        })
        .filter(|value| !value.is_empty())
}

fn query_multi_values(
    query_pairs: &[(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)],
    keys: &[&str],
) -> Vec<String> {
    let mut values = Vec::new();
    for (key, value) in query_pairs {
        if !keys
            .iter()
            .any(|candidate| key.eq_ignore_ascii_case(candidate))
        {
            continue;
        }
        let decoded = decode_percent_encoded(value.trim());
        for part in decoded
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            values.push(part.to_string());
        }
    }
    values
}

fn query_u16(
    query_pairs: &[(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)],
    keys: &[&str],
) -> Option<u16> {
    query_value(query_pairs, keys).and_then(|value| value.parse::<u16>().ok())
}

fn insert_optional_string(extra: &mut BTreeMap<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        extra.insert(key.to_string(), Value::String(value));
    }
}

fn insert_optional_u16(extra: &mut BTreeMap<String, Value>, key: &str, value: Option<u16>) {
    if let Some(value) = value {
        extra.insert(key.to_string(), Value::Number(value.into()));
    }
}
