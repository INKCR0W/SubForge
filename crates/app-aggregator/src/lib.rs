//! app-aggregator：多来源节点聚合、去重与分组。

mod dedupe;
mod merge;
mod region;
#[cfg(test)]
mod tests;
mod types;

pub use dedupe::build_dedupe_key as build_node_dedupe_key;
pub use merge::Aggregator;
pub use types::{AggregationResult, SourceNodes};
