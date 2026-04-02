use std::collections::{BTreeMap, HashMap};

use app_common::ProxyNode;

pub(crate) fn build_region_groups(nodes: &[ProxyNode]) -> BTreeMap<String, Vec<String>> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for node in nodes {
        let Some(region) = normalize_region(node.region.as_deref()) else {
            continue;
        };
        groups.entry(region).or_default().push(node.name.clone());
    }

    for names in groups.values_mut() {
        names.sort_unstable();
    }

    groups
}

pub(crate) fn resolve_name_conflicts(
    nodes: &mut [ProxyNode],
    source_aliases: &HashMap<String, String>,
) {
    let mut name_to_indices = BTreeMap::<String, Vec<usize>>::new();
    for (index, node) in nodes.iter().enumerate() {
        name_to_indices
            .entry(node.name.clone())
            .or_default()
            .push(index);
    }

    for (name, indices) in name_to_indices {
        if indices.len() <= 1 {
            continue;
        }

        for index in indices {
            let source_label = source_aliases
                .get(&nodes[index].source_id)
                .cloned()
                .unwrap_or_else(|| nodes[index].source_id.clone());
            nodes[index].name = format!("{name} ({source_label})");
        }
    }

    // 后缀后仍可能重名（例如同来源内原名相同），统一加编号保证唯一。
    let mut name_counter = HashMap::<String, usize>::new();
    for node in nodes.iter_mut() {
        let entry = name_counter.entry(node.name.clone()).or_default();
        *entry += 1;
        if *entry > 1 {
            node.name = format!("{} #{}", node.name, entry);
        }
    }
}

fn normalize_region(region: Option<&str>) -> Option<String> {
    let region = region?.trim();
    if region.is_empty() {
        None
    } else {
        Some(region.to_ascii_lowercase())
    }
}
