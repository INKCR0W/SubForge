use std::collections::BTreeMap;

use app_common::{ProxyNode, ProxyProtocol, ProxyTransport, TlsConfig};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde_json::{Value, json};

use super::{Base64Transformer, ClashTransformer, SingboxTransformer, Transformer};

#[test]
fn snapshot_ss_proxy_yaml() {
    assert_snapshot(
        build_node(
            "SS-HK",
            ProxyProtocol::Ss,
            ProxyTransport::Tcp,
            Some("hk"),
            vec![
                ("cipher", Value::String("aes-128-gcm".to_string())),
                ("password", Value::String("p@ss".to_string())),
            ],
        ),
        include_str!("../fixtures/clash_ss.yaml"),
    );
}

#[test]
fn snapshot_vmess_proxy_yaml() {
    assert_snapshot(
        build_node(
            "VMESS-SG",
            ProxyProtocol::Vmess,
            ProxyTransport::Ws,
            Some("sg"),
            vec![
                (
                    "uuid",
                    Value::String("11111111-1111-1111-1111-111111111111".to_string()),
                ),
                ("path", Value::String("/ws".to_string())),
                ("host", Value::String("edge.example.com".to_string())),
            ],
        ),
        include_str!("../fixtures/clash_vmess.yaml"),
    );
}

#[test]
fn snapshot_vless_proxy_yaml() {
    assert_snapshot(
        build_node(
            "VLESS-US",
            ProxyProtocol::Vless,
            ProxyTransport::Grpc,
            Some("us"),
            vec![
                (
                    "uuid",
                    Value::String("22222222-2222-2222-2222-222222222222".to_string()),
                ),
                ("service_name", Value::String("vless-grpc".to_string())),
                ("flow", Value::String("xtls-rprx-vision".to_string())),
            ],
        ),
        include_str!("../fixtures/clash_vless.yaml"),
    );
}

#[test]
fn snapshot_trojan_proxy_yaml() {
    assert_snapshot(
        build_node(
            "TROJAN-JP",
            ProxyProtocol::Trojan,
            ProxyTransport::Tcp,
            Some("jp"),
            vec![("password", Value::String("trojan-pass".to_string()))],
        ),
        include_str!("../fixtures/clash_trojan.yaml"),
    );
}

#[test]
fn snapshot_hysteria2_proxy_yaml() {
    assert_snapshot(
        build_node(
            "HY2-HK",
            ProxyProtocol::Hysteria2,
            ProxyTransport::Quic,
            Some("hk"),
            vec![
                ("password", Value::String("hy2-pass".to_string())),
                ("obfs", Value::String("salamander".to_string())),
                ("obfs_password", Value::String("hy2-obfs".to_string())),
                ("alpn", json!(["h3"])),
            ],
        ),
        include_str!("../fixtures/clash_hysteria2.yaml"),
    );
}

#[test]
fn snapshot_tuic_proxy_yaml() {
    assert_snapshot(
        build_node(
            "TUIC-SG",
            ProxyProtocol::Tuic,
            ProxyTransport::Quic,
            Some("sg"),
            vec![
                (
                    "uuid",
                    Value::String("33333333-3333-3333-3333-333333333333".to_string()),
                ),
                ("password", Value::String("tuic-pass".to_string())),
                ("congestion_control", Value::String("bbr".to_string())),
                ("udp_relay_mode", Value::String("native".to_string())),
                ("alpn", json!(["h3", "h3-29"])),
            ],
        ),
        include_str!("../fixtures/clash_tuic.yaml"),
    );
}

#[test]
fn snapshot_ss_outbound_json() {
    assert_json_snapshot(
        build_node(
            "SS-HK",
            ProxyProtocol::Ss,
            ProxyTransport::Tcp,
            Some("hk"),
            vec![
                ("cipher", Value::String("aes-128-gcm".to_string())),
                ("password", Value::String("p@ss".to_string())),
            ],
        ),
        include_str!("../fixtures/singbox_ss.json"),
    );
}

#[test]
fn snapshot_vmess_outbound_json() {
    assert_json_snapshot(
        build_node(
            "VMESS-SG",
            ProxyProtocol::Vmess,
            ProxyTransport::Ws,
            Some("sg"),
            vec![
                (
                    "uuid",
                    Value::String("11111111-1111-1111-1111-111111111111".to_string()),
                ),
                ("path", Value::String("/ws".to_string())),
                ("host", Value::String("edge.example.com".to_string())),
            ],
        ),
        include_str!("../fixtures/singbox_vmess.json"),
    );
}

#[test]
fn snapshot_vless_outbound_json() {
    assert_json_snapshot(
        build_node(
            "VLESS-US",
            ProxyProtocol::Vless,
            ProxyTransport::Grpc,
            Some("us"),
            vec![
                (
                    "uuid",
                    Value::String("22222222-2222-2222-2222-222222222222".to_string()),
                ),
                ("service_name", Value::String("vless-grpc".to_string())),
                ("flow", Value::String("xtls-rprx-vision".to_string())),
            ],
        ),
        include_str!("../fixtures/singbox_vless.json"),
    );
}

#[test]
fn snapshot_trojan_outbound_json() {
    assert_json_snapshot(
        build_node(
            "TROJAN-JP",
            ProxyProtocol::Trojan,
            ProxyTransport::Tcp,
            Some("jp"),
            vec![("password", Value::String("trojan-pass".to_string()))],
        ),
        include_str!("../fixtures/singbox_trojan.json"),
    );
}

#[test]
fn snapshot_hysteria2_outbound_json() {
    assert_json_snapshot(
        build_node(
            "HY2-HK",
            ProxyProtocol::Hysteria2,
            ProxyTransport::Quic,
            Some("hk"),
            vec![
                ("password", Value::String("hy2-pass".to_string())),
                ("obfs", Value::String("salamander".to_string())),
                ("obfs_password", Value::String("hy2-obfs".to_string())),
                ("alpn", json!(["h3"])),
            ],
        ),
        include_str!("../fixtures/singbox_hysteria2.json"),
    );
}

#[test]
fn snapshot_tuic_outbound_json() {
    assert_json_snapshot(
        build_node(
            "TUIC-SG",
            ProxyProtocol::Tuic,
            ProxyTransport::Quic,
            Some("sg"),
            vec![
                (
                    "uuid",
                    Value::String("33333333-3333-3333-3333-333333333333".to_string()),
                ),
                ("password", Value::String("tuic-pass".to_string())),
                ("congestion_control", Value::String("bbr".to_string())),
                ("udp_relay_mode", Value::String("native".to_string())),
                ("alpn", json!(["h3", "h3-29"])),
            ],
        ),
        include_str!("../fixtures/singbox_tuic.json"),
    );
}

#[test]
fn snapshot_ss_share_link_base64() {
    assert_base64_snapshot(
        build_node(
            "SS-HK",
            ProxyProtocol::Ss,
            ProxyTransport::Tcp,
            Some("hk"),
            vec![
                ("cipher", Value::String("aes-128-gcm".to_string())),
                ("password", Value::String("p@ss".to_string())),
            ],
        ),
        include_str!("../fixtures/base64_ss.txt"),
        "ss://YWVzLTEyOC1nY206cEBzcw@ss-hk.example.com:443#SS-HK",
    );
}

#[test]
fn snapshot_vmess_share_link_base64() {
    assert_base64_snapshot(
        build_node(
            "VMESS-SG",
            ProxyProtocol::Vmess,
            ProxyTransport::Ws,
            Some("sg"),
            vec![
                (
                    "uuid",
                    Value::String("11111111-1111-1111-1111-111111111111".to_string()),
                ),
                ("path", Value::String("/ws".to_string())),
                ("host", Value::String("edge.example.com".to_string())),
            ],
        ),
        include_str!("../fixtures/base64_vmess.txt"),
        "vmess://eyJhZGQiOiJ2bWVzcy1zZy5leGFtcGxlLmNvbSIsImFpZCI6IjAiLCJob3N0IjoiZWRnZS5leGFtcGxlLmNvbSIsImlkIjoiMTExMTExMTEtMTExMS0xMTExLTExMTEtMTExMTExMTExMTExIiwibmV0Ijoid3MiLCJwYXRoIjoiL3dzIiwicG9ydCI6IjQ0MyIsInBzIjoiVk1FU1MtU0ciLCJzY3kiOiJhdXRvIiwic25pIjoidGxzLmV4YW1wbGUuY29tIiwidGxzIjoidGxzIiwidHlwZSI6Im5vbmUiLCJ2IjoiMiJ9",
    );
}

#[test]
fn snapshot_vless_share_link_base64() {
    assert_base64_snapshot(
        build_node(
            "VLESS-US",
            ProxyProtocol::Vless,
            ProxyTransport::Grpc,
            Some("us"),
            vec![
                (
                    "uuid",
                    Value::String("22222222-2222-2222-2222-222222222222".to_string()),
                ),
                ("service_name", Value::String("vless-grpc".to_string())),
                ("flow", Value::String("xtls-rprx-vision".to_string())),
            ],
        ),
        include_str!("../fixtures/base64_vless.txt"),
        "vless://22222222-2222-2222-2222-222222222222@vless-us.example.com:443?encryption=none&type=grpc&serviceName=vless-grpc&security=tls&sni=tls.example.com&flow=xtls-rprx-vision#VLESS-US",
    );
}

#[test]
fn snapshot_trojan_share_link_base64() {
    assert_base64_snapshot(
        build_node(
            "TROJAN-JP",
            ProxyProtocol::Trojan,
            ProxyTransport::Tcp,
            Some("jp"),
            vec![("password", Value::String("trojan-pass".to_string()))],
        ),
        include_str!("../fixtures/base64_trojan.txt"),
        "trojan://trojan-pass@trojan-jp.example.com:443?security=tls&sni=tls.example.com#TROJAN-JP",
    );
}

#[test]
fn snapshot_hysteria2_share_link_base64() {
    assert_base64_snapshot(
        build_node(
            "HY2-HK",
            ProxyProtocol::Hysteria2,
            ProxyTransport::Quic,
            Some("hk"),
            vec![
                ("password", Value::String("hy2-pass".to_string())),
                ("obfs", Value::String("salamander".to_string())),
                ("obfs_password", Value::String("hy2-obfs".to_string())),
                ("alpn", json!(["h3"])),
            ],
        ),
        include_str!("../fixtures/base64_hysteria2.txt"),
        "hysteria2://hy2-pass@hy2-hk.example.com:443?obfs=salamander&obfs-password=hy2-obfs&sni=tls.example.com&alpn=h3#HY2-HK",
    );
}

#[test]
fn snapshot_tuic_share_link_base64() {
    assert_base64_snapshot(
        build_node(
            "TUIC-SG",
            ProxyProtocol::Tuic,
            ProxyTransport::Quic,
            Some("sg"),
            vec![
                (
                    "uuid",
                    Value::String("33333333-3333-3333-3333-333333333333".to_string()),
                ),
                ("password", Value::String("tuic-pass".to_string())),
                ("congestion_control", Value::String("bbr".to_string())),
                ("udp_relay_mode", Value::String("native".to_string())),
                ("alpn", json!(["h3", "h3-29"])),
            ],
        ),
        include_str!("../fixtures/base64_tuic.txt"),
        "tuic://33333333-3333-3333-3333-333333333333:tuic-pass@tuic-sg.example.com:443?congestion_control=bbr&udp_relay_mode=native&sni=tls.example.com&alpn=h3%2Ch3-29#TUIC-SG",
    );
}

fn assert_snapshot(node: ProxyNode, expected_snapshot: &str) {
    let transformer = ClashTransformer::default();
    let yaml = transformer
        .transform(&[node], &test_profile())
        .expect("转换 YAML 失败");
    assert_eq!(normalize_yaml(&yaml), normalize_yaml(expected_snapshot));
}

fn normalize_yaml(yaml: &str) -> String {
    yaml.replace("\r\n", "\n").trim().to_string()
}

fn assert_json_snapshot(node: ProxyNode, expected_snapshot: &str) {
    let transformer = SingboxTransformer::default();
    let json = transformer
        .transform(&[node], &test_profile())
        .expect("转换 JSON 失败");
    assert_eq!(normalize_json(&json), normalize_json(expected_snapshot));
}

fn assert_base64_snapshot(node: ProxyNode, expected_snapshot: &str, expected_uri: &str) {
    let transformer = Base64Transformer;
    let payload = transformer
        .transform(&[node], &test_profile())
        .expect("转换 Base64 失败");
    let expected_snapshot = expected_snapshot.trim().trim_start_matches('\u{feff}');
    assert_eq!(payload.trim(), expected_snapshot);

    let decoded = BASE64_STANDARD
        .decode(payload.as_bytes())
        .expect("Base64 解码失败");
    let decoded_uri = String::from_utf8(decoded).expect("Base64 解码内容不是 UTF-8");
    assert_eq!(decoded_uri, expected_uri);
}

fn normalize_json(payload: &str) -> String {
    let value: Value = serde_json::from_str(payload).expect("解析 JSON 快照失败");
    serde_json::to_string_pretty(&value).expect("序列化 JSON 快照失败")
}

fn test_profile() -> app_common::Profile {
    app_common::Profile {
        id: "profile-1".to_string(),
        name: "Default".to_string(),
        description: Some("test profile".to_string()),
        created_at: "2026-04-03T00:00:00Z".to_string(),
        updated_at: "2026-04-03T00:00:00Z".to_string(),
    }
}

fn build_node(
    name: &str,
    protocol: ProxyProtocol,
    transport: ProxyTransport,
    region: Option<&str>,
    extra_entries: Vec<(&str, Value)>,
) -> ProxyNode {
    let mut extra = BTreeMap::<String, Value>::new();
    for (key, value) in extra_entries {
        extra.insert(key.to_string(), value);
    }

    ProxyNode {
        id: format!("node-{name}"),
        name: name.to_string(),
        protocol,
        server: format!("{}.example.com", name.to_ascii_lowercase()),
        port: 443,
        transport,
        tls: TlsConfig {
            enabled: true,
            server_name: Some("tls.example.com".to_string()),
        },
        extra,
        source_id: "source-a".to_string(),
        tags: Vec::new(),
        region: region.map(ToString::to_string),
        updated_at: "2026-04-03T00:00:00Z".to_string(),
    }
}
