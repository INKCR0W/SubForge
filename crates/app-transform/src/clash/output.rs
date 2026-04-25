use serde::Serialize;
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};

use crate::TransformResult;

use super::ClashProxy;

#[derive(Debug, Serialize)]
pub(super) struct ClashConfig {
    pub(super) proxies: Vec<ClashProxy>,
    #[serde(rename = "proxy-groups")]
    pub(super) proxy_groups: Vec<ClashProxyGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) rules: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(super) struct ClashProxyGroup {
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) group_type: String,
    pub(super) proxies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) interval: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tolerance: Option<u16>,
    #[serde(rename = "use", skip_serializing_if = "Vec::is_empty")]
    pub(super) providers: Vec<String>,
}

pub(super) fn serialize_with_base_config(
    base_config_yaml: &str,
    proxies: Vec<ClashProxy>,
    proxy_groups: Vec<ClashProxyGroup>,
    rules: Option<Vec<String>>,
) -> TransformResult<String> {
    let mut root = match serde_yaml::from_str::<YamlValue>(base_config_yaml)? {
        YamlValue::Mapping(mapping) => mapping,
        _ => YamlMapping::new(),
    };
    root.insert(
        YamlValue::String("proxies".to_string()),
        serde_yaml::to_value(proxies)?,
    );
    root.insert(
        YamlValue::String("proxy-groups".to_string()),
        serde_yaml::to_value(proxy_groups)?,
    );
    if let Some(rules) = rules {
        root.insert(
            YamlValue::String("rules".to_string()),
            serde_yaml::to_value(rules)?,
        );
    }
    Ok(serde_yaml::to_string(&YamlValue::Mapping(root))?)
}
