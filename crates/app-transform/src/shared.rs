use app_common::{ProxyNode, ProxyTransport};
use serde_json::Value;

use crate::{TransformError, TransformResult};

pub(crate) fn push_unique_proxy_name(target: &mut Vec<String>, name: &str) {
    if !target.iter().any(|item| item == name) {
        target.push(name.to_string());
    }
}

pub(crate) fn clash_network(transport: &ProxyTransport) -> &'static str {
    match transport {
        ProxyTransport::Tcp => "tcp",
        ProxyTransport::Ws => "ws",
        ProxyTransport::Grpc => "grpc",
        ProxyTransport::H2 => "h2",
        ProxyTransport::Quic => "quic",
    }
}

pub(crate) fn required_string(node: &ProxyNode, field: &'static str) -> TransformResult<String> {
    optional_string(node, field).ok_or_else(|| TransformError::MissingField {
        node_name: node.name.clone(),
        field,
    })
}

pub(crate) fn optional_string(node: &ProxyNode, field: &str) -> Option<String> {
    node.extra.get(field).and_then(|value| match value {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    })
}

pub(crate) fn optional_bool(node: &ProxyNode, field: &str) -> Option<bool> {
    node.extra.get(field).and_then(|value| match value {
        Value::Bool(raw) => Some(*raw),
        Value::String(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    })
}

pub(crate) fn optional_u32(node: &ProxyNode, field: &str) -> Option<u32> {
    node.extra.get(field).and_then(|value| match value {
        Value::Number(raw) => raw.as_u64().and_then(|raw| u32::try_from(raw).ok()),
        Value::String(raw) => raw.trim().parse::<u32>().ok(),
        _ => None,
    })
}

pub(crate) fn optional_string_list(node: &ProxyNode, field: &str) -> Option<Vec<String>> {
    node.extra.get(field).and_then(|value| match value {
        Value::Array(items) => {
            let values = items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            if values.is_empty() {
                None
            } else {
                Some(values)
            }
        }
        Value::String(raw) => {
            let value = raw.trim();
            if value.is_empty() {
                None
            } else {
                Some(vec![value.to_string()])
            }
        }
        _ => None,
    })
}
