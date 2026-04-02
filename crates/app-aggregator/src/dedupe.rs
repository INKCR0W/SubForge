use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use app_common::ProxyNode;
use serde_json::Value;

use crate::SourceNodes;

pub(crate) fn build_source_aliases(sources: &[SourceNodes]) -> HashMap<String, String> {
    let mut aliases = HashMap::with_capacity(sources.len());
    for source in sources {
        let alias = source
            .source_alias
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(source.source_id.as_str())
            .to_string();
        aliases.insert(source.source_id.clone(), alias);
    }
    aliases
}

pub(crate) fn dedupe_nodes(sources: &[SourceNodes]) -> Vec<ProxyNode> {
    let mut deduped = Vec::new();
    let mut key_to_index = HashMap::<String, usize>::new();

    for source in sources {
        for node in &source.nodes {
            let key = build_dedupe_key(node);
            if let Some(existing_index) = key_to_index.get(&key).copied() {
                if should_replace(node, &deduped[existing_index]) {
                    deduped[existing_index] = node.clone();
                }
                continue;
            }

            key_to_index.insert(key, deduped.len());
            deduped.push(node.clone());
        }
    }

    deduped
}

fn build_dedupe_key(node: &ProxyNode) -> String {
    let credential = extract_credential(node);
    let transport_hash = build_transport_hash(node);
    format!(
        "{:?}|{}|{}|{}|{:016x}",
        node.protocol,
        node.server.to_ascii_lowercase(),
        node.port,
        credential,
        transport_hash
    )
}

fn extract_credential(node: &ProxyNode) -> String {
    node.extra
        .get("uuid")
        .or_else(|| node.extra.get("password"))
        .map(value_to_identity)
        .unwrap_or_default()
}

fn value_to_identity(value: &Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn build_transport_hash(node: &ProxyNode) -> u64 {
    let mut filtered_extra = node.extra.clone();
    filtered_extra.remove("uuid");
    filtered_extra.remove("password");

    let signature = serde_json::json!({
        "transport": node.transport,
        "tls": node.tls,
        "extra": filtered_extra
    });
    let serialized = serde_json::to_string(&signature).unwrap_or_default();

    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    hasher.finish()
}

fn should_replace(candidate: &ProxyNode, current: &ProxyNode) -> bool {
    // Runtime 时间戳统一使用 RFC3339（UTC）字符串，按字典序可比较新旧。
    candidate.updated_at >= current.updated_at
}
