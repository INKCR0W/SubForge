use std::collections::{BTreeMap, HashSet};

use app_common::ProxyProtocol;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

use crate::{CoreError, CoreResult};

pub(super) fn map_protocol(raw: &str) -> CoreResult<ProxyProtocol> {
    match raw {
        "ss" | "shadowsocks" => Ok(ProxyProtocol::Ss),
        "ssr" | "shadowsocksr" => Ok(ProxyProtocol::Ssr),
        "vmess" => Ok(ProxyProtocol::Vmess),
        "vless" => Ok(ProxyProtocol::Vless),
        "trojan" => Ok(ProxyProtocol::Trojan),
        _ => Err(CoreError::SubscriptionParse(format!("不支持的协议：{raw}"))),
    }
}

pub(super) fn validate_required(
    protocol: &ProxyProtocol,
    extra: &BTreeMap<String, JsonValue>,
    node_name: &str,
) -> CoreResult<()> {
    let required = match protocol {
        ProxyProtocol::Ss | ProxyProtocol::Ssr => &["cipher", "password"][..],
        ProxyProtocol::Vmess | ProxyProtocol::Vless => &["uuid"][..],
        ProxyProtocol::Trojan => &["password"][..],
        _ => &[][..],
    };

    for key in required {
        if !extra.contains_key(*key) {
            return Err(CoreError::SubscriptionParse(format!(
                "节点缺少必要字段：{} ({})",
                key, node_name
            )));
        }
    }

    Ok(())
}

pub(super) fn yaml_string(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(YamlValue::String(key.to_string()))
        .and_then(YamlValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn yaml_u16(map: &serde_yaml::Mapping, key: &str) -> Option<u16> {
    map.get(YamlValue::String(key.to_string()))
        .and_then(|value| {
            value
                .as_u64()
                .and_then(|raw| u16::try_from(raw).ok())
                .or_else(|| {
                    value
                        .as_str()
                        .and_then(|raw| raw.trim().parse::<u16>().ok())
                })
        })
}

pub(super) fn yaml_bool(map: &serde_yaml::Mapping, key: &str) -> Option<bool> {
    map.get(YamlValue::String(key.to_string()))
        .and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_str().map(parse_bool_like))
        })
}

pub(super) fn yaml_map_string(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(YamlValue::String(key.to_string()))
        .and_then(YamlValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(super) fn json_string(map: &serde_json::Map<String, JsonValue>, key: &str) -> Option<String> {
    map.get(key).and_then(|value| match value {
        JsonValue::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        JsonValue::Number(number) => Some(number.to_string()),
        JsonValue::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        _ => None,
    })
}

pub(super) fn json_u16(map: &serde_json::Map<String, JsonValue>, key: &str) -> Option<u16> {
    map.get(key).and_then(|value| {
        value
            .as_u64()
            .and_then(|raw| u16::try_from(raw).ok())
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|raw| raw.trim().parse::<u16>().ok())
            })
    })
}

pub(super) fn json_bool(map: &serde_json::Map<String, JsonValue>, key: &str) -> Option<bool> {
    map.get(key).and_then(|value| {
        value
            .as_bool()
            .or_else(|| value.as_str().map(parse_bool_like))
    })
}

pub(super) fn parse_bool_like(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "tls"
    )
}

pub(super) fn insert_optional_string(
    extra: &mut BTreeMap<String, JsonValue>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        extra.insert(key.to_string(), JsonValue::String(value));
    }
}

pub(super) fn take_any(kv: &mut BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = kv.remove(*key) {
            return Some(value);
        }
    }
    None
}

pub(super) fn known_yaml_keys() -> HashSet<&'static str> {
    [
        "name",
        "type",
        "server",
        "port",
        "cipher",
        "method",
        "password",
        "uuid",
        "id",
        "flow",
        "network",
        "tls",
        "sni",
        "servername",
        "ws-opts",
    ]
    .into_iter()
    .collect()
}

pub(super) fn known_json_keys() -> HashSet<&'static str> {
    [
        "name",
        "type",
        "server",
        "port",
        "cipher",
        "method",
        "password",
        "uuid",
        "id",
        "flow",
        "network",
        "tls",
        "sni",
        "servername",
    ]
    .into_iter()
    .collect()
}
