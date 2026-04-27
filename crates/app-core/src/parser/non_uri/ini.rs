use std::collections::BTreeMap;

use app_common::{ProxyNode, ProxyProtocol, TlsConfig};
use serde_json::Value as JsonValue;

use crate::utils::safe_stderr_line;
use crate::{CoreError, CoreResult};

use super::helpers::{
    insert_optional_string, map_protocol, parse_bool_like, take_any, validate_required,
};
use crate::parser::{build_proxy_node, map_transport};

pub(super) fn parse_ini_like_lines(
    source_id: &str,
    payload: &str,
    updated_at: &str,
) -> Vec<ProxyNode> {
    let mut nodes = Vec::new();

    for (line_number, raw_line) in payload.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with("//")
            || (line.starts_with('[') && line.ends_with(']'))
        {
            continue;
        }

        let Some((name_raw, value_raw)) = line.split_once('=') else {
            continue;
        };
        let name = name_raw.trim();
        if name.is_empty() {
            continue;
        }

        match parse_ini_proxy_line(source_id, name, value_raw.trim(), updated_at) {
            Ok(Some(node)) => nodes.push(node),
            Ok(None) => {}
            Err(error) => safe_stderr_line(&format!(
                "WARN: 解析文本代理行失败（source_id={}, line={}）：{}",
                source_id,
                line_number + 1,
                error
            )),
        }
    }

    nodes
}

fn parse_ini_proxy_line(
    source_id: &str,
    name: &str,
    value_raw: &str,
    updated_at: &str,
) -> CoreResult<Option<ProxyNode>> {
    let parts = value_raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();

    if parts.len() < 3 {
        return Ok(None);
    }

    let proxy_type = parts[0].to_ascii_lowercase();
    let protocol = match map_protocol(&proxy_type) {
        Ok(protocol) => protocol,
        Err(_) => return Ok(None),
    };

    let server = parts[1].to_string();
    let port = parts[2]
        .parse::<u16>()
        .map_err(|error| CoreError::SubscriptionParse(format!("端口非法：{error}")))?;

    let mut kv = BTreeMap::<String, String>::new();
    for part in parts.iter().skip(3) {
        if let Some((key, value)) = part.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if !key.is_empty() && !value.is_empty() {
                kv.insert(key, value);
            }
        }
    }

    let mut extra = BTreeMap::<String, JsonValue>::new();
    match protocol {
        ProxyProtocol::Ss | ProxyProtocol::Ssr => {
            insert_optional_string(
                &mut extra,
                "cipher",
                take_any(&mut kv, &["encrypt-method", "method", "cipher"]),
            );
            insert_optional_string(
                &mut extra,
                "password",
                take_any(&mut kv, &["password", "passwd"]),
            );
        }
        ProxyProtocol::Vmess | ProxyProtocol::Vless => {
            insert_optional_string(&mut extra, "uuid", take_any(&mut kv, &["uuid", "username"]));
            insert_optional_string(&mut extra, "flow", take_any(&mut kv, &["flow"]));
        }
        ProxyProtocol::Trojan => {
            insert_optional_string(
                &mut extra,
                "password",
                take_any(&mut kv, &["password", "passwd"]),
            );
        }
        _ => return Ok(None),
    }

    let transport = map_transport(take_any(
        &mut kv,
        &["network", "obfs", "transport", "plugin", "obfs-mode"],
    ));

    let tls_server_name = take_any(&mut kv, &["sni", "peer", "servername"]);
    let tls_enabled = take_any(&mut kv, &["tls", "over-tls", "obfs", "secure"])
        .map(|value| parse_bool_like(&value))
        .unwrap_or(matches!(
            protocol,
            ProxyProtocol::Vmess | ProxyProtocol::Vless | ProxyProtocol::Trojan
        ));

    let path = take_any(&mut kv, &["ws-path", "path", "obfs-uri"]);
    insert_optional_string(&mut extra, "path", path);
    let host = take_any(&mut kv, &["ws-headers", "host", "obfs-host"]).and_then(|raw| {
        if let Some((_, value)) = raw.split_once(':') {
            Some(value.trim().to_string())
        } else {
            Some(raw)
        }
    });
    insert_optional_string(&mut extra, "host", host);

    validate_required(&protocol, &extra, name)?;

    if !kv.is_empty() {
        safe_stderr_line(&format!(
            "WARN: 丢弃不支持字段（source_id={}, name={}, type={}）：{}",
            source_id,
            name,
            proxy_type,
            kv.keys().cloned().collect::<Vec<_>>().join(",")
        ));
    }

    Ok(Some(build_proxy_node(
        source_id,
        name.to_string(),
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
    )))
}
