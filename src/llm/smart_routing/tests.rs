//! Unit tests for smart routing configuration and model selection.

mod overrides;
mod routing;
mod scoring_edge;
mod scoring_tiers;

use super::SmartRoutingConfig;

fn default_config() -> SmartRoutingConfig {
    SmartRoutingConfig::default()
}
