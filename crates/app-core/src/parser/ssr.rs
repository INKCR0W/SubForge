use std::collections::BTreeMap;

use app_common::{ProxyNode, ProxyProtocol, ProxyTransport, TlsConfig};
use serde_json::Value;

use crate::CoreError;
use crate::CoreResult;

use super::{build_proxy_node, decode_percent_encoded, try_decode_base64_text};

pub(crate) fn parse_ssr_uri(
    line: &str,
    source_id: &str,
    updated_at: &str,
) -> CoreResult<ProxyNode> {
    let raw = line["ssr://".len()..].trim();
    let decoded = try_decode_base64_text(raw)
        .ok_or_else(|| CoreError::SubscriptionParse("ssr URI Base64 解码失败".to_string()))?;

    let (head, query_part) = decoded
        .split_once("/?")
        .map_or((decoded.as_str(), ""), |(h, q)| (h, q));
    let mut segments = head.split(':');

    let server = segments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::SubscriptionParse("ssr URI 缺少 server".to_string()))?
        .to_string();
    let port = segments
        .next()
        .ok_or_else(|| CoreError::SubscriptionParse("ssr URI 缺少 port".to_string()))?
        .parse::<u16>()
        .map_err(|error| CoreError::SubscriptionParse(format!("ssr URI 端口非法：{error}")))?;
    let protocol = segments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::SubscriptionParse("ssr URI 缺少 protocol".to_string()))?
        .to_string();
    let method = segments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::SubscriptionParse("ssr URI 缺少 method".to_string()))?
        .to_string();
    let obfs = segments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::SubscriptionParse("ssr URI 缺少 obfs".to_string()))?
        .to_string();
    let password_encoded = segments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CoreError::SubscriptionParse("ssr URI 缺少 password".to_string()))?;

    let password = try_decode_base64_text(password_encoded).ok_or_else(|| {
        CoreError::SubscriptionParse("ssr URI password Base64 解码失败".to_string())
    })?;

    let mut name = format!("ssr-{server}:{port}");
    let mut extra = BTreeMap::new();
    extra.insert("cipher".to_string(), Value::String(method));
    extra.insert("password".to_string(), Value::String(password));
    extra.insert("protocol".to_string(), Value::String(protocol));
    extra.insert("obfs".to_string(), Value::String(obfs));

    if !query_part.is_empty() {
        for pair in query_part.split('&') {
            let Some((key, raw_value)) = pair.split_once('=') else {
                continue;
            };
            let decoded_value = try_decode_base64_text(raw_value)
                .map(|value| decode_percent_encoded(&value))
                .or_else(|| {
                    if raw_value.is_empty() {
                        None
                    } else {
                        Some(decode_percent_encoded(raw_value))
                    }
                })
                .filter(|value| !value.is_empty());

            match key {
                "remarks" => {
                    if let Some(value) = decoded_value {
                        name = value;
                    }
                }
                "obfsparam" => {
                    if let Some(value) = decoded_value {
                        extra.insert("obfs_param".to_string(), Value::String(value));
                    }
                }
                "protoparam" => {
                    if let Some(value) = decoded_value {
                        extra.insert("protocol_param".to_string(), Value::String(value));
                    }
                }
                _ => {}
            }
        }
    }

    Ok(build_proxy_node(
        source_id,
        name,
        ProxyProtocol::Ssr,
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
