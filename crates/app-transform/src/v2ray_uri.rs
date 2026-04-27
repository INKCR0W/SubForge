use app_common::{Profile, ProxyNode};

use crate::{TransformError, TransformResult, Transformer, build_share_uri};

#[derive(Debug, Clone, Default)]
pub struct V2RayUriTransformer;

impl Transformer for V2RayUriTransformer {
    fn transform(&self, nodes: &[ProxyNode], _profile: &Profile) -> TransformResult<String> {
        let mut uri_lines = Vec::with_capacity(nodes.len());
        for node in nodes {
            let uri = match build_share_uri(node) {
                Ok(uri) => uri,
                Err(TransformError::UnsupportedProtocol {
                    node_name,
                    protocol,
                    ..
                }) => {
                    return Err(TransformError::UnsupportedProtocol {
                        node_name,
                        protocol,
                        target: "v2ray-uri",
                    });
                }
                Err(error) => return Err(error),
            };
            uri_lines.push(uri);
        }

        Ok(uri_lines.join("\n"))
    }
}
