use app_common::{ClashRoutingTemplate, ClashRoutingTemplateGroup};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};

pub(super) fn source_routing_template_key(source_instance_id: &str) -> String {
    format!("source.{source_instance_id}.clash_routing_template")
}

pub(super) fn extract_clash_routing_template(payload: &str) -> Option<ClashRoutingTemplate> {
    let root = serde_yaml::from_str::<YamlValue>(payload).ok()?;
    let root = root.as_mapping()?;
    let groups_value = yaml_map_get(root, "proxy-groups")?;
    let groups = groups_value.as_sequence()?;

    let mut parsed_groups = Vec::new();
    for group in groups {
        let Some(group_map) = group.as_mapping() else {
            continue;
        };
        let Some(name) = yaml_map_get(group_map, "name")
            .and_then(YamlValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(group_type) = yaml_map_get(group_map, "type")
            .and_then(YamlValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let proxies = yaml_map_get(group_map, "proxies")
            .and_then(YamlValue::as_sequence)
            .map(|items| {
                items
                    .iter()
                    .filter_map(YamlValue::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let url = yaml_map_get(group_map, "url")
            .and_then(YamlValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let interval = yaml_map_get(group_map, "interval")
            .and_then(YamlValue::as_i64)
            .and_then(|value| u32::try_from(value).ok());
        let tolerance = yaml_map_get(group_map, "tolerance")
            .and_then(YamlValue::as_i64)
            .and_then(|value| u16::try_from(value).ok());

        parsed_groups.push(ClashRoutingTemplateGroup {
            name: name.to_string(),
            group_type: group_type.to_string(),
            proxies,
            url,
            interval,
            tolerance,
        });
    }

    if parsed_groups.is_empty() {
        None
    } else {
        Some(ClashRoutingTemplate {
            groups: parsed_groups,
        })
    }
}

fn yaml_map_get<'a>(mapping: &'a YamlMapping, key: &str) -> Option<&'a YamlValue> {
    mapping.get(&YamlValue::String(key.to_string()))
}
