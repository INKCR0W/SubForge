use std::collections::BTreeMap;

use app_common::ProxyNode;

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
