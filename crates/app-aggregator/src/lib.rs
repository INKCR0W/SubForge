//! app-aggregator：多来源节点聚合、去重与分组。

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use app_common::ProxyNode;
use serde_json::Value;

/// 单个来源的节点输入。
#[derive(Debug, Clone, PartialEq)]
pub struct SourceNodes {
    pub source_id: String,
    pub source_alias: Option<String>,
    pub nodes: Vec<ProxyNode>,
}

impl SourceNodes {
    pub fn new(source_id: impl Into<String>, nodes: Vec<ProxyNode>) -> Self {
        Self {
            source_id: source_id.into(),
            source_alias: None,
            nodes,
        }
    }

    pub fn with_alias(
        source_id: impl Into<String>,
        source_alias: impl Into<String>,
        nodes: Vec<ProxyNode>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            source_alias: Some(source_alias.into()),
            nodes,
        }
    }
}

/// 聚合结果：去重后的节点与按 region 的节点名称分组。
#[derive(Debug, Clone, PartialEq)]
pub struct AggregationResult {
    pub nodes: Vec<ProxyNode>,
    pub region_groups: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Aggregator;

impl Aggregator {
    pub fn aggregate(&self, sources: &[SourceNodes]) -> AggregationResult {
        let source_aliases = build_source_aliases(sources);
        let mut deduped = dedupe_nodes(sources);
        resolve_name_conflicts(&mut deduped, &source_aliases);
        let region_groups = build_region_groups(&deduped);

        AggregationResult {
            nodes: deduped,
            region_groups,
        }
    }
}

fn build_source_aliases(sources: &[SourceNodes]) -> HashMap<String, String> {
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

fn dedupe_nodes(sources: &[SourceNodes]) -> Vec<ProxyNode> {
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

fn resolve_name_conflicts(nodes: &mut [ProxyNode], source_aliases: &HashMap<String, String>) {
    let mut name_to_indices = BTreeMap::<String, Vec<usize>>::new();
    for (index, node) in nodes.iter().enumerate() {
        name_to_indices
            .entry(node.name.clone())
            .or_default()
            .push(index);
    }

    for (name, indices) in name_to_indices {
        if indices.len() <= 1 {
            continue;
        }

        for index in indices {
            let source_label = source_aliases
                .get(&nodes[index].source_id)
                .cloned()
                .unwrap_or_else(|| nodes[index].source_id.clone());
            nodes[index].name = format!("{name} ({source_label})");
        }
    }

    // 后缀后仍可能重名（例如同来源内原名相同），统一加编号保证唯一。
    let mut name_counter = HashMap::<String, usize>::new();
    for node in nodes.iter_mut() {
        let entry = name_counter.entry(node.name.clone()).or_default();
        *entry += 1;
        if *entry > 1 {
            node.name = format!("{} #{}", node.name, entry);
        }
    }
}

fn build_region_groups(nodes: &[ProxyNode]) -> BTreeMap<String, Vec<String>> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for node in nodes {
        let Some(region) = normalize_region(node.region.as_deref()) else {
            continue;
        };
        groups.entry(region).or_default().push(node.name.clone());
    }

    for names in groups.values_mut() {
        names.sort_unstable();
    }

    groups
}

fn normalize_region(region: Option<&str>) -> Option<String> {
    let region = region?.trim();
    if region.is_empty() {
        None
    } else {
        Some(region.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use app_common::{ProxyProtocol, ProxyTransport, TlsConfig};
    use serde_json::{Value, json};

    use super::{Aggregator, SourceNodes};

    #[test]
    fn keeps_latest_node_for_duplicate_identity_and_outputs_17_nodes() {
        let mut source_a_nodes = Vec::new();
        let mut source_b_nodes = Vec::new();

        for index in 0..10 {
            source_a_nodes.push(build_node(
                "source-a",
                &format!("A-{index}"),
                &format!("shared-{index}.example.com"),
                3000 + index as u16,
                ProxyTransport::Ws,
                Some(&format!("uuid-{index}")),
                None,
                Some(&format!("/a/{index}")),
                Some("hk"),
                "2026-04-03T01:00:00Z",
            ));
        }

        // 制造 3 组重复：source-b 时间更新，应该覆盖 source-a。
        for index in 0..3 {
            source_b_nodes.push(build_node(
                "source-b",
                &format!("B-dup-{index}"),
                &format!("shared-{index}.example.com"),
                3000 + index as u16,
                ProxyTransport::Ws,
                Some(&format!("uuid-{index}")),
                None,
                Some(&format!("/a/{index}")),
                Some("sg"),
                "2026-04-03T02:00:00Z",
            ));
        }
        for index in 3..10 {
            source_b_nodes.push(build_node(
                "source-b",
                &format!("B-{index}"),
                &format!("source-b-{index}.example.com"),
                5000 + index as u16,
                ProxyTransport::Tcp,
                Some(&format!("uuid-b-{index}")),
                None,
                None,
                Some("sg"),
                "2026-04-03T02:00:00Z",
            ));
        }

        let result = Aggregator.aggregate(&[
            SourceNodes::new("source-a", source_a_nodes),
            SourceNodes::new("source-b", source_b_nodes),
        ]);

        assert_eq!(result.nodes.len(), 17);

        let replaced = result
            .nodes
            .iter()
            .find(|node| node.server == "shared-0.example.com" && node.port == 3000)
            .expect("应存在重复节点的聚合结果");
        assert_eq!(replaced.source_id, "source-b");
        assert_eq!(replaced.name, "B-dup-0");
    }

    #[test]
    fn appends_source_alias_when_names_conflict() {
        let source_a = build_node(
            "source-a",
            "HK-01",
            "a.example.com",
            443,
            ProxyTransport::Tcp,
            Some("uuid-a"),
            None,
            None,
            Some("hk"),
            "2026-04-03T00:00:00Z",
        );
        let source_b = build_node(
            "source-b",
            "HK-01",
            "b.example.com",
            443,
            ProxyTransport::Tcp,
            Some("uuid-b"),
            None,
            None,
            Some("hk"),
            "2026-04-03T00:00:00Z",
        );

        let result = Aggregator.aggregate(&[
            SourceNodes::with_alias("source-a", "alpha", vec![source_a]),
            SourceNodes::with_alias("source-b", "beta", vec![source_b]),
        ]);
        let names = result
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"HK-01 (alpha)"));
        assert!(names.contains(&"HK-01 (beta)"));
    }

    #[test]
    fn transport_options_participate_in_dedupe_key() {
        let node_a = build_node(
            "source-a",
            "same-endpoint-a",
            "same.example.com",
            8443,
            ProxyTransport::Ws,
            Some("uuid-same"),
            None,
            Some("/path-a"),
            Some("hk"),
            "2026-04-03T00:00:00Z",
        );
        let node_b = build_node(
            "source-b",
            "same-endpoint-b",
            "same.example.com",
            8443,
            ProxyTransport::Ws,
            Some("uuid-same"),
            None,
            Some("/path-b"),
            Some("hk"),
            "2026-04-03T00:00:01Z",
        );

        let result = Aggregator.aggregate(&[
            SourceNodes::new("source-a", vec![node_a]),
            SourceNodes::new("source-b", vec![node_b]),
        ]);
        assert_eq!(result.nodes.len(), 2);
    }

    #[test]
    fn empty_source_does_not_affect_aggregation_and_groups_by_region() {
        let hk = build_node(
            "source-a",
            "HK-Node",
            "hk.example.com",
            443,
            ProxyTransport::Tcp,
            None,
            Some("pwd-hk"),
            None,
            Some("hk"),
            "2026-04-03T00:00:00Z",
        );
        let sg = build_node(
            "source-a",
            "SG-Node",
            "sg.example.com",
            443,
            ProxyTransport::Tcp,
            None,
            Some("pwd-sg"),
            None,
            Some("sg"),
            "2026-04-03T00:00:00Z",
        );
        let unknown = build_node(
            "source-a",
            "Unknown-Node",
            "no-region.example.com",
            443,
            ProxyTransport::Tcp,
            None,
            Some("pwd-none"),
            None,
            None,
            "2026-04-03T00:00:00Z",
        );

        let result = Aggregator.aggregate(&[
            SourceNodes::new("empty-source", Vec::new()),
            SourceNodes::new("source-a", vec![hk, sg, unknown]),
        ]);

        assert_eq!(result.nodes.len(), 3);
        assert_eq!(
            result.region_groups.get("hk"),
            Some(&vec!["HK-Node".to_string()])
        );
        assert_eq!(
            result.region_groups.get("sg"),
            Some(&vec!["SG-Node".to_string()])
        );
        assert!(!result.region_groups.contains_key("unknown"));
    }

    fn build_node(
        source_id: &str,
        name: &str,
        server: &str,
        port: u16,
        transport: ProxyTransport,
        uuid: Option<&str>,
        password: Option<&str>,
        path: Option<&str>,
        region: Option<&str>,
        updated_at: &str,
    ) -> app_common::ProxyNode {
        let mut extra = BTreeMap::<String, Value>::new();
        if let Some(uuid) = uuid {
            extra.insert("uuid".to_string(), Value::String(uuid.to_string()));
        }
        if let Some(password) = password {
            extra.insert("password".to_string(), Value::String(password.to_string()));
        }
        if let Some(path) = path {
            extra.insert("path".to_string(), json!(path));
        }

        app_common::ProxyNode {
            id: format!("node-{source_id}-{server}-{port}-{name}"),
            name: name.to_string(),
            protocol: ProxyProtocol::Vmess,
            server: server.to_string(),
            port,
            transport,
            tls: TlsConfig {
                enabled: true,
                server_name: Some(server.to_string()),
            },
            extra,
            source_id: source_id.to_string(),
            tags: Vec::new(),
            region: region.map(ToString::to_string),
            updated_at: updated_at.to_string(),
        }
    }
}
