use std::collections::{HashMap, HashSet};

use app_aggregator::{Aggregator, SourceNodes, build_node_dedupe_key};
use app_common::{ClashRoutingTemplate, ProxyNode};
use app_transform::RoutingTemplateExportContext;

pub(super) fn build_template_export_nodes(
    source_nodes: &[SourceNodes],
    default_nodes: Vec<ProxyNode>,
    template_source_id: Option<&str>,
    routing_template: Option<ClashRoutingTemplate>,
) -> (Vec<ProxyNode>, Option<RoutingTemplateExportContext>) {
    let Some(template) = routing_template else {
        return (default_nodes, None);
    };
    let Some(template_source_id) = template_source_id else {
        return (default_nodes, None);
    };

    let template_nodes = source_nodes
        .iter()
        .find(|source| source.source_id == template_source_id)
        .map(|source| source.nodes.clone())
        .unwrap_or_default();
    if template_nodes.is_empty() {
        let context = RoutingTemplateExportContext::new(template, default_nodes.clone());
        return (default_nodes, Some(context));
    }

    let non_template_sources = source_nodes
        .iter()
        .filter(|source| source.source_id != template_source_id)
        .cloned()
        .collect::<Vec<_>>();
    let mut appended_nodes = Aggregator.aggregate(&non_template_sources).nodes;
    let template_dedupe_keys = template_nodes
        .iter()
        .map(build_node_dedupe_key)
        .collect::<HashSet<_>>();
    appended_nodes.retain(|node| !template_dedupe_keys.contains(&build_node_dedupe_key(node)));
    rename_appended_conflicts(&template_nodes, &mut appended_nodes, source_nodes);

    let mut final_nodes = template_nodes;
    final_nodes.extend(appended_nodes.clone());

    (
        final_nodes,
        Some(RoutingTemplateExportContext::new(template, appended_nodes)),
    )
}

fn rename_appended_conflicts(
    template_nodes: &[ProxyNode],
    appended_nodes: &mut [ProxyNode],
    source_nodes: &[SourceNodes],
) {
    let source_aliases = source_nodes
        .iter()
        .map(|source| {
            (
                source.source_id.clone(),
                source
                    .source_alias
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(source.source_id.as_str())
                    .to_string(),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut used_names = template_nodes
        .iter()
        .map(|node| node.name.clone())
        .collect::<HashSet<_>>();

    for node in appended_nodes {
        if used_names.insert(node.name.clone()) {
            continue;
        }

        let base_name = node.name.clone();
        let source_label = source_aliases
            .get(&node.source_id)
            .cloned()
            .unwrap_or_else(|| node.source_id.clone());
        let candidate_base = format!("{base_name} ({source_label})");
        let mut candidate = candidate_base.clone();
        let mut suffix = 2usize;
        while used_names.contains(&candidate) {
            candidate = format!("{candidate_base} #{suffix}");
            suffix += 1;
        }

        node.name = candidate.clone();
        used_names.insert(candidate);
    }
}
