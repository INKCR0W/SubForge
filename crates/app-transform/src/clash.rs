use std::collections::{BTreeMap, BTreeSet};

use app_common::{Profile, ProxyNode, ProxyProtocol, ProxyTransport};
use serde::Serialize;

use crate::shared::{
    clash_network, optional_bool, optional_string, optional_string_list, optional_u32,
    push_unique_proxy_name, required_string,
};
use crate::{TransformError, TransformResult, Transformer};

/// Clash/Mihomo YAML 转换器。
#[derive(Debug, Clone)]
pub struct ClashTransformer {
    auto_test_url: String,
    auto_test_interval_seconds: u32,
    auto_test_tolerance: u16,
}

impl Default for ClashTransformer {
    fn default() -> Self {
        Self {
            auto_test_url: "http://www.gstatic.com/generate_204".to_string(),
            auto_test_interval_seconds: 300,
            auto_test_tolerance: 50,
        }
    }
}

impl Transformer for ClashTransformer {
    fn transform(&self, nodes: &[ProxyNode], _profile: &Profile) -> TransformResult<String> {
        let mut proxies = Vec::with_capacity(nodes.len());
        for node in nodes {
            proxies.push(build_clash_proxy(node)?);
        }

        let config = ClashConfig {
            proxies,
            proxy_groups: self.build_proxy_groups(nodes),
        };
        Ok(serde_yaml::to_string(&config)?)
    }
}

impl ClashTransformer {
    fn build_proxy_groups(&self, nodes: &[ProxyNode]) -> Vec<ClashProxyGroup> {
        let node_names = nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>();
        let region_groups = collect_region_groups(nodes);

        let mut select_proxies = Vec::new();
        push_unique_proxy_name(&mut select_proxies, "Auto");
        for region_name in region_groups.keys() {
            push_unique_proxy_name(&mut select_proxies, region_name);
        }
        for node_name in &node_names {
            push_unique_proxy_name(&mut select_proxies, node_name);
        }

        let mut groups = vec![
            ClashProxyGroup {
                name: "Select".to_string(),
                group_type: "select".to_string(),
                proxies: select_proxies,
                url: None,
                interval: None,
                tolerance: None,
            },
            ClashProxyGroup {
                name: "Auto".to_string(),
                group_type: "url-test".to_string(),
                proxies: node_names,
                url: Some(self.auto_test_url.clone()),
                interval: Some(self.auto_test_interval_seconds),
                tolerance: Some(self.auto_test_tolerance),
            },
        ];

        for (region_name, region_node_names) in region_groups {
            groups.push(ClashProxyGroup {
                name: region_name,
                group_type: "select".to_string(),
                proxies: region_node_names,
                url: None,
                interval: None,
                tolerance: None,
            });
        }

        groups
    }
}

/// sing-box JSON 转换器。

fn build_clash_proxy(node: &ProxyNode) -> TransformResult<ClashProxy> {
    let network = Some(clash_network(&node.transport).to_string());
    let ws_opts = build_ws_options(node);
    let grpc_opts = build_grpc_options(node);
    let h2_opts = build_h2_options(node);
    let tls_enabled = Some(node.tls.enabled);
    let servername = node.tls.server_name.clone();
    let sni = node
        .tls
        .server_name
        .clone()
        .or_else(|| optional_string(node, "sni"));
    let skip_cert_verify = optional_bool(node, "skip_cert_verify");

    let mut proxy = ClashProxy {
        name: node.name.clone(),
        proxy_type: String::new(),
        server: node.server.clone(),
        port: node.port,
        cipher: None,
        password: None,
        uuid: None,
        alter_id: None,
        udp: Some(true),
        tls: tls_enabled,
        sni,
        servername,
        network: None,
        flow: None,
        skip_cert_verify,
        client_fingerprint: optional_string(node, "client_fingerprint"),
        ws_opts,
        grpc_opts,
        h2_opts,
        alpn: optional_string_list(node, "alpn"),
        obfs: optional_string(node, "obfs"),
        obfs_password: optional_string(node, "obfs_password"),
        congestion_control: optional_string(node, "congestion_control"),
        udp_relay_mode: optional_string(node, "udp_relay_mode"),
    };

    match node.protocol {
        ProxyProtocol::Ss => {
            proxy.proxy_type = "ss".to_string();
            proxy.cipher = Some(required_string(node, "cipher")?);
            proxy.password = Some(required_string(node, "password")?);
            proxy.network = Some("tcp".to_string());
            proxy.tls = None;
            proxy.sni = None;
            proxy.servername = None;
            proxy.ws_opts = None;
            proxy.grpc_opts = None;
            proxy.h2_opts = None;
            proxy.skip_cert_verify = None;
            proxy.client_fingerprint = None;
            proxy.alpn = None;
            proxy.obfs = None;
            proxy.obfs_password = None;
            proxy.congestion_control = None;
            proxy.udp_relay_mode = None;
        }
        ProxyProtocol::Vmess => {
            proxy.proxy_type = "vmess".to_string();
            proxy.uuid = Some(required_string(node, "uuid")?);
            proxy.alter_id = optional_u32(node, "alter_id").or(Some(0));
            proxy.cipher = optional_string(node, "cipher").or(Some("auto".to_string()));
            proxy.network = network;
            proxy.flow = None;
            proxy.sni = None;
        }
        ProxyProtocol::Vless => {
            proxy.proxy_type = "vless".to_string();
            proxy.uuid = Some(required_string(node, "uuid")?);
            proxy.network = network;
            proxy.flow = optional_string(node, "flow");
            proxy.sni = None;
            proxy.alter_id = None;
            proxy.cipher = None;
        }
        ProxyProtocol::Trojan => {
            proxy.proxy_type = "trojan".to_string();
            proxy.password = Some(required_string(node, "password")?);
            proxy.network = network;
            proxy.sni = proxy.servername.clone();
            proxy.alter_id = None;
            proxy.cipher = None;
            proxy.uuid = None;
            proxy.flow = None;
        }
        ProxyProtocol::Hysteria2 => {
            proxy.proxy_type = "hysteria2".to_string();
            proxy.password = Some(
                optional_string(node, "password")
                    .or_else(|| optional_string(node, "auth"))
                    .ok_or_else(|| TransformError::MissingField {
                        node_name: node.name.clone(),
                        field: "password/auth",
                    })?,
            );
            proxy.network = None;
            proxy.uuid = None;
            proxy.flow = None;
            proxy.alter_id = None;
            proxy.cipher = None;
            proxy.grpc_opts = None;
            proxy.h2_opts = None;
            proxy.ws_opts = None;
        }
        ProxyProtocol::Tuic => {
            proxy.proxy_type = "tuic".to_string();
            proxy.uuid = Some(required_string(node, "uuid")?);
            proxy.password = Some(required_string(node, "password")?);
            proxy.network = None;
            proxy.flow = None;
            proxy.alter_id = None;
            proxy.cipher = None;
            proxy.grpc_opts = None;
            proxy.h2_opts = None;
            proxy.ws_opts = None;
        }
    }

    Ok(proxy)
}

fn build_ws_options(node: &ProxyNode) -> Option<ClashWsOptions> {
    if !matches!(node.transport, ProxyTransport::Ws) {
        return None;
    }

    let mut headers = BTreeMap::new();
    if let Some(host) = optional_string(node, "host") {
        headers.insert("Host".to_string(), host);
    }

    Some(ClashWsOptions {
        path: optional_string(node, "path").unwrap_or_else(|| "/".to_string()),
        headers: (!headers.is_empty()).then_some(headers),
        max_early_data: optional_u32(node, "max_early_data"),
        early_data_header_name: optional_string(node, "early_data_header_name"),
    })
}

fn build_grpc_options(node: &ProxyNode) -> Option<ClashGrpcOptions> {
    if !matches!(node.transport, ProxyTransport::Grpc) {
        return None;
    }

    Some(ClashGrpcOptions {
        grpc_service_name: optional_string(node, "grpc_service_name")
            .or_else(|| optional_string(node, "service_name"))
            .unwrap_or_else(|| "grpc".to_string()),
    })
}

fn build_h2_options(node: &ProxyNode) -> Option<ClashH2Options> {
    if !matches!(node.transport, ProxyTransport::H2) {
        return None;
    }

    let host = optional_string_list(node, "host");
    Some(ClashH2Options {
        host,
        path: optional_string(node, "path"),
    })
}

fn collect_region_groups(nodes: &[ProxyNode]) -> BTreeMap<String, Vec<String>> {
    let mut groups = BTreeMap::<String, BTreeSet<String>>::new();
    for node in nodes {
        let Some(region_name) = normalize_region_name(node.region.as_deref()) else {
            continue;
        };
        groups
            .entry(region_name)
            .or_default()
            .insert(node.name.clone());
    }

    groups
        .into_iter()
        .map(|(name, values)| (name, values.into_iter().collect::<Vec<_>>()))
        .collect()
}

fn normalize_region_name(region: Option<&str>) -> Option<String> {
    let value = region?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_ascii_uppercase())
    }
}

#[derive(Debug, Serialize)]
struct ClashConfig {
    proxies: Vec<ClashProxy>,
    #[serde(rename = "proxy-groups")]
    proxy_groups: Vec<ClashProxyGroup>,
}

#[derive(Debug, Serialize)]
struct ClashProxyGroup {
    name: String,
    #[serde(rename = "type")]
    group_type: String,
    proxies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interval: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tolerance: Option<u16>,
}

#[derive(Debug, Serialize)]
struct ClashProxy {
    name: String,
    #[serde(rename = "type")]
    proxy_type: String,
    server: String,
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    cipher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uuid: Option<String>,
    #[serde(rename = "alterId", skip_serializing_if = "Option::is_none")]
    alter_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    udp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sni: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    servername: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    flow: Option<String>,
    #[serde(rename = "skip-cert-verify", skip_serializing_if = "Option::is_none")]
    skip_cert_verify: Option<bool>,
    #[serde(rename = "client-fingerprint", skip_serializing_if = "Option::is_none")]
    client_fingerprint: Option<String>,
    #[serde(rename = "ws-opts", skip_serializing_if = "Option::is_none")]
    ws_opts: Option<ClashWsOptions>,
    #[serde(rename = "grpc-opts", skip_serializing_if = "Option::is_none")]
    grpc_opts: Option<ClashGrpcOptions>,
    #[serde(rename = "h2-opts", skip_serializing_if = "Option::is_none")]
    h2_opts: Option<ClashH2Options>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alpn: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    obfs: Option<String>,
    #[serde(rename = "obfs-password", skip_serializing_if = "Option::is_none")]
    obfs_password: Option<String>,
    #[serde(
        rename = "congestion-controller",
        skip_serializing_if = "Option::is_none"
    )]
    congestion_control: Option<String>,
    #[serde(rename = "udp-relay-mode", skip_serializing_if = "Option::is_none")]
    udp_relay_mode: Option<String>,
}

#[derive(Debug, Serialize)]
struct ClashWsOptions {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<BTreeMap<String, String>>,
    #[serde(rename = "max-early-data", skip_serializing_if = "Option::is_none")]
    max_early_data: Option<u32>,
    #[serde(
        rename = "early-data-header-name",
        skip_serializing_if = "Option::is_none"
    )]
    early_data_header_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct ClashGrpcOptions {
    #[serde(rename = "grpc-service-name")]
    grpc_service_name: String,
}

#[derive(Debug, Serialize)]
struct ClashH2Options {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}
