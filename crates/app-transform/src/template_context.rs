use app_common::{ClashRoutingTemplate, ProxyNode};

#[derive(Debug, Clone, PartialEq)]
pub struct RoutingTemplateExportContext {
    pub template: ClashRoutingTemplate,
    pub appended_nodes: Vec<ProxyNode>,
}

impl RoutingTemplateExportContext {
    pub fn new(template: ClashRoutingTemplate, appended_nodes: Vec<ProxyNode>) -> Self {
        Self {
            template,
            appended_nodes,
        }
    }
}
