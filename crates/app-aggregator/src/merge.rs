use crate::dedupe::{build_source_aliases, dedupe_nodes};
use crate::region::{build_region_groups, resolve_name_conflicts};
use crate::{AggregationResult, SourceNodes};

#[derive(Debug, Default, Clone, Copy)]
pub struct Aggregator;

impl Aggregator {
    pub fn aggregate(&self, sources: &[SourceNodes]) -> AggregationResult {
        let source_aliases = build_source_aliases(sources);
        let mut deduped = dedupe_nodes(sources);
        resolve_name_conflicts(&mut deduped, &source_aliases);
        let region_groups = build_region_groups(&deduped);

        AggregationResult {
            nodes: deduped,
            region_groups,
        }
    }
}
