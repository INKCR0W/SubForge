use app_common::{Profile, ProxyNode, ProxyProtocol};

use crate::shared::{optional_string, required_string};
use crate::{TransformError, TransformResult, Transformer};

#[derive(Debug, Clone, Default)]
pub struct NonUriTextTransformer;

impl NonUriTextTransformer {
    pub fn transform_surge(
        &self,
        nodes: &[ProxyNode],
        profile: &Profile,
    ) -> TransformResult<String> {
        self.transform_as_ini(nodes, profile)
    }

    pub fn transform_loon(
        &self,
        nodes: &[ProxyNode],
        profile: &Profile,
    ) -> TransformResult<String> {
        self.transform_as_ini(nodes, profile)
    }

    pub fn transform_qx(&self, nodes: &[ProxyNode], profile: &Profile) -> TransformResult<String> {
        self.transform_as_ini(nodes, profile)
    }

    fn transform_as_ini(&self, nodes: &[ProxyNode], _profile: &Profile) -> TransformResult<String> {
        let mut lines = Vec::with_capacity(nodes.len());
        for node in nodes {
            lines.push(node_to_ini_line(node)?);
        }
        Ok(lines.join("\n"))
    }
}

impl Transformer for NonUriTextTransformer {
    fn transform(&self, nodes: &[ProxyNode], profile: &Profile) -> TransformResult<String> {
        self.transform_as_ini(nodes, profile)
    }
}

fn node_to_ini_line(node: &ProxyNode) -> TransformResult<String> {
    let node_type = match node.protocol {
        ProxyProtocol::Ss => "ss",
        ProxyProtocol::Ssr => "ssr",
        ProxyProtocol::Vmess => "vmess",
        ProxyProtocol::Vless => "vless",
        ProxyProtocol::Trojan => "trojan",
        _ => {
            return Err(TransformError::UnsupportedProtocol {
                node_name: node.name.clone(),
                protocol: protocol_name(&node.protocol),
                target: "non-uri-text",
            });
        }
    };

    let mut parts = vec![
        node.name.clone(),
        " = ".to_string(),
        node_type.to_string(),
        ", ".to_string(),
        node.server.clone(),
        ", ".to_string(),
        node.port.to_string(),
    ];

    match node.protocol {
        ProxyProtocol::Ss | ProxyProtocol::Ssr => {
            let cipher = required_string(node, "cipher")?;
            let password = required_string(node, "password")?;
            parts.push(", encrypt-method=".to_string());
            parts.push(cipher);
            parts.push(", password=".to_string());
            parts.push(password);
        }
        ProxyProtocol::Vmess | ProxyProtocol::Vless => {
            let uuid = required_string(node, "uuid")?;
            parts.push(", uuid=".to_string());
            parts.push(uuid);
            if let Some(flow) = optional_string(node, "flow") {
                parts.push(", flow=".to_string());
                parts.push(flow);
            }
        }
        ProxyProtocol::Trojan => {
            let password = required_string(node, "password")?;
            parts.push(", password=".to_string());
            parts.push(password);
        }
        _ => {}
    }

    if node.tls.enabled {
        parts.push(", tls=true".to_string());
    }
    if let Some(sni) = node
        .tls
        .server_name
        .clone()
        .or_else(|| optional_string(node, "sni"))
    {
        parts.push(", sni=".to_string());
        parts.push(sni);
    }

    Ok(parts.concat())
}

fn protocol_name(protocol: &ProxyProtocol) -> &'static str {
    match protocol {
        ProxyProtocol::Ss => "ss",
        ProxyProtocol::Ssr => "ssr",
        ProxyProtocol::Vmess => "vmess",
        ProxyProtocol::Vless => "vless",
        ProxyProtocol::Trojan => "trojan",
        ProxyProtocol::Hysteria2 => "hysteria2",
        ProxyProtocol::Tuic => "tuic",
        ProxyProtocol::AnyTls => "anytls",
        ProxyProtocol::WireGuard => "wireguard",
        ProxyProtocol::Socks5 => "socks5",
        ProxyProtocol::Http => "http",
    }
}
