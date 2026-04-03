use std::collections::BTreeMap;
use std::time::{Duration, Instant};

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
            NodeMeta {
                uuid: Some(&format!("uuid-{index}")),
                password: None,
                path: Some(&format!("/a/{index}")),
                region: Some("hk"),
            },
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
            NodeMeta {
                uuid: Some(&format!("uuid-{index}")),
                password: None,
                path: Some(&format!("/a/{index}")),
                region: Some("sg"),
            },
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
            NodeMeta {
                uuid: Some(&format!("uuid-b-{index}")),
                password: None,
                path: None,
                region: Some("sg"),
            },
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
        NodeMeta {
            uuid: Some("uuid-a"),
            password: None,
            path: None,
            region: Some("hk"),
        },
        "2026-04-03T00:00:00Z",
    );
    let source_b = build_node(
        "source-b",
        "HK-01",
        "b.example.com",
        443,
        ProxyTransport::Tcp,
        NodeMeta {
            uuid: Some("uuid-b"),
            password: None,
            path: None,
            region: Some("hk"),
        },
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
        NodeMeta {
            uuid: Some("uuid-same"),
            password: None,
            path: Some("/path-a"),
            region: Some("hk"),
        },
        "2026-04-03T00:00:00Z",
    );
    let node_b = build_node(
        "source-b",
        "same-endpoint-b",
        "same.example.com",
        8443,
        ProxyTransport::Ws,
        NodeMeta {
            uuid: Some("uuid-same"),
            password: None,
            path: Some("/path-b"),
            region: Some("hk"),
        },
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
        NodeMeta {
            uuid: None,
            password: Some("pwd-hk"),
            path: None,
            region: Some("hk"),
        },
        "2026-04-03T00:00:00Z",
    );
    let sg = build_node(
        "source-a",
        "SG-Node",
        "sg.example.com",
        443,
        ProxyTransport::Tcp,
        NodeMeta {
            uuid: None,
            password: Some("pwd-sg"),
            path: None,
            region: Some("sg"),
        },
        "2026-04-03T00:00:00Z",
    );
    let unknown = build_node(
        "source-a",
        "Unknown-Node",
        "no-region.example.com",
        443,
        ProxyTransport::Tcp,
        NodeMeta {
            uuid: None,
            password: Some("pwd-none"),
            path: None,
            region: None,
        },
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

#[test]
fn aggregates_1000_nodes_within_500ms() {
    let source_a = (0..500)
        .map(|index| {
            build_node(
                "source-a",
                &format!("A-{index}"),
                &format!("a-{index}.example.com"),
                3000 + index as u16,
                ProxyTransport::Ws,
                NodeMeta {
                    uuid: Some(&format!("uuid-a-{index}")),
                    password: None,
                    path: Some("/ws"),
                    region: Some("hk"),
                },
                "2026-04-04T00:00:00Z",
            )
        })
        .collect::<Vec<_>>();
    let source_b = (0..500)
        .map(|index| {
            build_node(
                "source-b",
                &format!("B-{index}"),
                &format!("b-{index}.example.com"),
                4000 + index as u16,
                ProxyTransport::Tcp,
                NodeMeta {
                    uuid: Some(&format!("uuid-b-{index}")),
                    password: None,
                    path: None,
                    region: Some("sg"),
                },
                "2026-04-04T00:00:00Z",
            )
        })
        .collect::<Vec<_>>();

    let started_at = Instant::now();
    let result = Aggregator.aggregate(&[
        SourceNodes::new("source-a", source_a),
        SourceNodes::new("source-b", source_b),
    ]);
    let elapsed = started_at.elapsed();

    assert_eq!(result.nodes.len(), 1000);
    assert!(
        elapsed < Duration::from_millis(500),
        "1000 节点聚合耗时应小于 500ms，当前: {:?}",
        elapsed
    );
}

#[derive(Default)]
struct NodeMeta<'a> {
    uuid: Option<&'a str>,
    password: Option<&'a str>,
    path: Option<&'a str>,
    region: Option<&'a str>,
}

fn build_node(
    source_id: &str,
    name: &str,
    server: &str,
    port: u16,
    transport: ProxyTransport,
    meta: NodeMeta<'_>,
    updated_at: &str,
) -> app_common::ProxyNode {
    let mut extra = BTreeMap::<String, Value>::new();
    if let Some(uuid) = meta.uuid {
        extra.insert("uuid".to_string(), Value::String(uuid.to_string()));
    }
    if let Some(password) = meta.password {
        extra.insert("password".to_string(), Value::String(password.to_string()));
    }
    if let Some(path) = meta.path {
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
        region: meta.region.map(ToString::to_string),
        updated_at: updated_at.to_string(),
    }
}
