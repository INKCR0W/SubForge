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
