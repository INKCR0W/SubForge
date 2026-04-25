use std::collections::{BTreeMap, BTreeSet};

use app_common::{ClashRoutingTemplate, Profile, ProxyNode};
use serde::Serialize;

use crate::shared::push_unique_proxy_name;
use crate::{RoutingTemplateExportContext, TransformResult, Transformer};

mod group_utils;
mod output;
mod proxy;
use group_utils::{collect_region_groups, filter_group_candidate_nodes, is_builtin_policy};
use output::{ClashConfig, ClashProxyGroup, serialize_with_base_config};

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
        self.transform_with_template(nodes, None)
    }
}

impl ClashTransformer {
    pub fn transform_with_template(
        &self,
        nodes: &[ProxyNode],
        routing_template: Option<&ClashRoutingTemplate>,
    ) -> TransformResult<String> {
        let template_context =
            routing_template
                .cloned()
                .map(|template| RoutingTemplateExportContext {
                    template,
                    appended_nodes: nodes.to_vec(),
                });
        self.transform_with_template_context(nodes, template_context.as_ref())
    }

    pub fn transform_with_template_context(
        &self,
        nodes: &[ProxyNode],
        template_context: Option<&RoutingTemplateExportContext>,
    ) -> TransformResult<String> {
        let mut proxies = Vec::with_capacity(nodes.len());
        for node in nodes {
            proxies.push(proxy::build_clash_proxy(node)?);
        }

        let (proxy_groups, template_applied) = match template_context {
            Some(context) => self.build_proxy_groups_from_template(nodes, context),
            None => (self.build_proxy_groups(nodes), false),
        };
        let rules = if template_applied {
            template_context.and_then(|context| {
                let template = &context.template;
                if template.rules.is_empty() {
                    None
                } else {
                    Some(template.rules.clone())
                }
            })
        } else {
            None
        };

        if let Some(template) = template_context.map(|context| &context.template)
            && let Some(base_config_yaml) = template.base_config_yaml.as_deref()
            && !base_config_yaml.trim().is_empty()
        {
            return serialize_with_base_config(base_config_yaml, proxies, proxy_groups, rules);
        }

        let config = ClashConfig {
            proxies,
            proxy_groups,
            rules,
        };
        Ok(serde_yaml::to_string(&config)?)
    }

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
                providers: Vec::new(),
            },
            ClashProxyGroup {
                name: "Auto".to_string(),
                group_type: "url-test".to_string(),
                proxies: node_names,
                url: Some(self.auto_test_url.clone()),
                interval: Some(self.auto_test_interval_seconds),
                tolerance: Some(self.auto_test_tolerance),
                providers: Vec::new(),
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
                providers: Vec::new(),
            });
        }

        groups
    }

    fn build_proxy_groups_from_template(
        &self,
        nodes: &[ProxyNode],
        template_context: &RoutingTemplateExportContext,
    ) -> (Vec<ClashProxyGroup>, bool) {
        let routing_template = &template_context.template;
        let final_node_names = nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>();
        let appended_node_names = template_context
            .appended_nodes
            .iter()
            .map(|node| node.name.clone())
            .collect::<Vec<_>>();
        let group_name_set = routing_template
            .groups
            .iter()
            .map(|group| group.name.as_str())
            .collect::<BTreeSet<_>>();

        let mut groups = Vec::with_capacity(routing_template.groups.len());
        for template_group in &routing_template.groups {
            let has_plain_node_slot = template_group
                .proxies
                .iter()
                .any(|item| !group_name_set.contains(item.as_str()) && !is_builtin_policy(item));
            let populate_all_nodes =
                template_group.proxies.is_empty() && template_group.include_all;
            let candidate_nodes = if has_plain_node_slot {
                filter_group_candidate_nodes(
                    &appended_node_names,
                    template_group.filter.as_deref(),
                    template_group.exclude_filter.as_deref(),
                )
            } else if populate_all_nodes {
                filter_group_candidate_nodes(
                    &final_node_names,
                    template_group.filter.as_deref(),
                    template_group.exclude_filter.as_deref(),
                )
            } else {
                Vec::new()
            };
            let should_append_nodes = has_plain_node_slot || populate_all_nodes;

            let mut proxies = Vec::new();
            if routing_template.preserve_original_proxy_names {
                for item in &template_group.proxies {
                    push_unique_proxy_name(&mut proxies, item);
                }
                if should_append_nodes {
                    for name in &candidate_nodes {
                        push_unique_proxy_name(&mut proxies, name);
                    }
                }
            } else {
                let mut inserted_aggregated_nodes = false;
                for item in &template_group.proxies {
                    if group_name_set.contains(item.as_str()) || is_builtin_policy(item) {
                        push_unique_proxy_name(&mut proxies, item);
                        continue;
                    }
                    if !inserted_aggregated_nodes {
                        for name in &candidate_nodes {
                            push_unique_proxy_name(&mut proxies, name);
                        }
                        inserted_aggregated_nodes = true;
                    }
                }
                if !inserted_aggregated_nodes && should_append_nodes {
                    for name in &candidate_nodes {
                        push_unique_proxy_name(&mut proxies, name);
                    }
                }
            }

            groups.push(ClashProxyGroup {
                name: template_group.name.clone(),
                group_type: template_group.group_type.clone(),
                proxies,
                url: template_group.url.clone(),
                interval: template_group.interval,
                tolerance: template_group.tolerance,
                providers: template_group.providers.clone(),
            });
        }

        if groups.is_empty() {
            (self.build_proxy_groups(nodes), false)
        } else {
            (groups, true)
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ClashProxy {
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
pub(super) struct ClashWsOptions {
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
pub(super) struct ClashGrpcOptions {
    #[serde(rename = "grpc-service-name")]
    grpc_service_name: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ClashH2Options {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}
