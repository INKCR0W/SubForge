use app_common::{Profile, ProxyNode};

use crate::TransformResult;

/// 统一转换器接口。
pub trait Transformer {
    fn transform(&self, nodes: &[ProxyNode], profile: &Profile) -> TransformResult<String>;
}
