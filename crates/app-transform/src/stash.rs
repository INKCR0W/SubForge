use app_common::{Profile, ProxyNode};

use crate::{ClashTransformer, TransformResult, Transformer};

#[derive(Debug, Clone, Default)]
pub struct StashTransformer;

impl StashTransformer {
    pub fn transform(&self, nodes: &[ProxyNode], profile: &Profile) -> TransformResult<String> {
        ClashTransformer::default().transform(nodes, profile)
    }
}
