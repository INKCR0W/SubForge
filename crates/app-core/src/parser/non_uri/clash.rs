use std::collections::BTreeMap;

use app_common::{ProxyNode, ProxyProtocol, TlsConfig};
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

use crate::utils::safe_stderr_line;
use crate::{CoreError, CoreResult};

use super::helpers::{
    insert_optional_string, json_bool, json_string, json_u16, known_json_keys, known_yaml_keys,
    map_protocol, validate_required, yaml_bool, yaml_map_string, yaml_string, yaml_u16,
};
use crate::parser::{build_proxy_node, map_transport};

pub(super) fn try_parse_clash_like_yaml(
    source_id: &str,
    payload: &str,
    updated_at: &str,
) -> CoreResult<Option<Vec<ProxyNode>>> {
    let root = match serde_yaml::from_str::<YamlValue>(payload) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    let Some(mapping) = root.as_mapping() else {
        return Ok(None);
    };

    let proxies = mapping
        .get(YamlValue::String("proxies".to_string()))
        .and_then(YamlValue::as_sequence)
        .or_else(|| {
            mapping
                .get(YamlValue::String("Proxy".to_string()))
                .and_then(YamlValue::as_sequence)
        });

    let Some(proxies) = proxies else {
        return Ok(None);
    };

    let mut nodes = Vec::new();
    for (index, proxy) in proxies.iter().enumerate() {
        let Some(proxy_map) = proxy.as_mapping() else {
            safe_stderr_line(&format!(
                "WARN: 丢弃非对象节点（source_id={}, index={}）",
                source_id,
                index + 1
            ));
            continue;
        };

        match parse_clash_map_to_proxy_node(source_id, proxy_map, updated_at) {
            Ok(node) => nodes.push(node),
            Err(error) => safe_stderr_line(&format!(
                "WARN: 解析 Clash/Mihomo 节点失败（source_id={}, index={}）：{}",
                source_id,
                index + 1,
                error
            )),
        }
    }

    Ok(Some(nodes))
}

pub(super) fn try_parse_clash_like_json(
    source_id: &str,
    payload: &str,
    updated_at: &str,
) -> CoreResult<Option<Vec<ProxyNode>>> {
    let root = match serde_json::from_str::<JsonValue>(payload) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    let Some(proxies) = root
        .as_object()
        .and_then(|map| map.get("proxies"))
        .and_then(JsonValue::as_array)
    else {
        return Ok(None);
    };

    let mut nodes = Vec::new();
    for (index, proxy) in proxies.iter().enumerate() {
        let Some(proxy_map) = proxy.as_object() else {
            safe_stderr_line(&format!(
                "WARN: 丢弃非对象节点（source_id={}, index={}）",
                source_id,
                index + 1
            ));
            continue;
        };

        match parse_clash_json_to_proxy_node(source_id, proxy_map, updated_at) {
            Ok(node) => nodes.push(node),
            Err(error) => safe_stderr_line(&format!(
                "WARN: 解析 Clash/Mihomo JSON 节点失败（source_id={}, index={}）：{}",
                source_id,
                index + 1,
                error
            )),
        }
    }

    Ok(Some(nodes))
}

fn parse_clash_map_to_proxy_node(
    source_id: &str,
    map: &serde_yaml::Mapping,
    updated_at: &str,
) -> CoreResult<ProxyNode> {
    let name = yaml_string(map, "name")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少 name".to_string()))?;
    let proxy_type = yaml_string(map, "type")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少 type".to_string()))?
        .to_ascii_lowercase();
    let protocol = map_protocol(&proxy_type)?;

    let server = yaml_string(map, "server")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少 server".to_string()))?;
    let port = yaml_u16(map, "port")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少有效 port".to_string()))?;

    let transport = map_transport(yaml_string(map, "network"));
    let tls_enabled = yaml_bool(map, "tls").unwrap_or(matches!(
        protocol,
        ProxyProtocol::Vmess | ProxyProtocol::Vless | ProxyProtocol::Trojan
    ));
    let tls_server_name = yaml_string(map, "sni").or_else(|| yaml_string(map, "servername"));

    let mut extra = BTreeMap::<String, JsonValue>::new();
    match protocol {
        ProxyProtocol::Ss | ProxyProtocol::Ssr => {
            insert_optional_string(
                &mut extra,
                "cipher",
                yaml_string(map, "cipher").or_else(|| yaml_string(map, "method")),
            );
            insert_optional_string(&mut extra, "password", yaml_string(map, "password"));
        }
        ProxyProtocol::Vmess | ProxyProtocol::Vless => {
            insert_optional_string(
                &mut extra,
                "uuid",
                yaml_string(map, "uuid").or_else(|| yaml_string(map, "id")),
            );
            insert_optional_string(&mut extra, "flow", yaml_string(map, "flow"));
        }
        ProxyProtocol::Trojan => {
            insert_optional_string(&mut extra, "password", yaml_string(map, "password"));
        }
        _ => {
            return Err(CoreError::SubscriptionParse(format!(
                "非 URI 文本暂不支持该协议：{}",
                proxy_type
            )));
        }
    }

    if let Some(ws_opts) = map
        .get(YamlValue::String("ws-opts".to_string()))
        .and_then(YamlValue::as_mapping)
    {
        insert_optional_string(&mut extra, "path", yaml_map_string(ws_opts, "path"));
        let host = ws_opts
            .get(YamlValue::String("headers".to_string()))
            .and_then(YamlValue::as_mapping)
            .and_then(|headers| {
                yaml_map_string(headers, "Host").or_else(|| yaml_map_string(headers, "host"))
            });
        insert_optional_string(&mut extra, "host", host);
    }

    validate_required(&protocol, &extra, &name)?;
    warn_ignored_yaml_fields(source_id, &name, &proxy_type, map);

    Ok(build_proxy_node(
        source_id,
        name,
        protocol,
        server,
        port,
        transport,
        TlsConfig {
            enabled: tls_enabled,
            server_name: tls_server_name,
        },
        extra,
        updated_at,
    ))
}

fn parse_clash_json_to_proxy_node(
    source_id: &str,
    map: &serde_json::Map<String, JsonValue>,
    updated_at: &str,
) -> CoreResult<ProxyNode> {
    let name = json_string(map, "name")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少 name".to_string()))?;
    let proxy_type = json_string(map, "type")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少 type".to_string()))?
        .to_ascii_lowercase();
    let protocol = map_protocol(&proxy_type)?;

    let server = json_string(map, "server")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少 server".to_string()))?;
    let port = json_u16(map, "port")
        .ok_or_else(|| CoreError::SubscriptionParse("节点缺少有效 port".to_string()))?;

    let transport = map_transport(json_string(map, "network"));
    let tls_enabled = json_bool(map, "tls").unwrap_or(matches!(
        protocol,
        ProxyProtocol::Vmess | ProxyProtocol::Vless | ProxyProtocol::Trojan
    ));
    let tls_server_name = json_string(map, "sni").or_else(|| json_string(map, "servername"));

    let mut extra = BTreeMap::<String, JsonValue>::new();
    match protocol {
        ProxyProtocol::Ss | ProxyProtocol::Ssr => {
            insert_optional_string(
                &mut extra,
                "cipher",
                json_string(map, "cipher").or_else(|| json_string(map, "method")),
            );
            insert_optional_string(&mut extra, "password", json_string(map, "password"));
        }
        ProxyProtocol::Vmess | ProxyProtocol::Vless => {
            insert_optional_string(
                &mut extra,
                "uuid",
                json_string(map, "uuid").or_else(|| json_string(map, "id")),
            );
            insert_optional_string(&mut extra, "flow", json_string(map, "flow"));
        }
        ProxyProtocol::Trojan => {
            insert_optional_string(&mut extra, "password", json_string(map, "password"));
        }
        _ => {
            return Err(CoreError::SubscriptionParse(format!(
                "非 URI 文本暂不支持该协议：{}",
                proxy_type
            )));
        }
    }

    validate_required(&protocol, &extra, &name)?;
    warn_ignored_json_fields(source_id, &name, &proxy_type, map);

    Ok(build_proxy_node(
        source_id,
        name,
        protocol,
        server,
        port,
        transport,
        TlsConfig {
            enabled: tls_enabled,
            server_name: tls_server_name,
        },
        extra,
        updated_at,
    ))
}

fn warn_ignored_yaml_fields(
    source_id: &str,
    node_name: &str,
    proxy_type: &str,
    map: &serde_yaml::Mapping,
) {
    let allow = known_yaml_keys();
    let mut ignored = map
        .keys()
        .filter_map(YamlValue::as_str)
        .map(|key| key.to_ascii_lowercase())
        .filter(|key| !allow.contains(key.as_str()))
        .collect::<Vec<_>>();
    ignored.sort();
    ignored.dedup();
    if ignored.is_empty() {
        return;
    }

    safe_stderr_line(&format!(
        "WARN: 丢弃不支持字段（source_id={}, name={}, type={}）：{}",
        source_id,
        node_name,
        proxy_type,
        ignored.join(",")
    ));
}

fn warn_ignored_json_fields(
    source_id: &str,
    node_name: &str,
    proxy_type: &str,
    map: &serde_json::Map<String, JsonValue>,
) {
    let allow = known_json_keys();
    let mut ignored = map
        .keys()
        .map(|key| key.to_ascii_lowercase())
        .filter(|key| !allow.contains(key.as_str()))
        .collect::<Vec<_>>();
    ignored.sort();
    ignored.dedup();
    if ignored.is_empty() {
        return;
    }

    safe_stderr_line(&format!(
        "WARN: 丢弃不支持字段（source_id={}, name={}, type={}）：{}",
        source_id,
        node_name,
        proxy_type,
        ignored.join(",")
    ));
}
