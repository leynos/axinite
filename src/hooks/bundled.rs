//! Bundled hook implementations and declarative hook registration.
//!
//! Submodules:
//! - [`config`]: bundle configuration types and parse errors
//! - [`rule`]: built-in audit hook and declarative rule hooks
//! - [`webhook`]: fire-and-forget outbound webhook hooks
//! - [`net_policy`]: webhook target URL/header/IP validation

mod config;
mod net_policy;
mod rule;
#[cfg(test)]
mod tests;
mod webhook;

use std::sync::Arc;

pub use config::{
    HookBundleConfig, HookBundleError, HookRegistrationSummary, HookRuleConfig,
    OutboundWebhookConfig, RegexReplacementConfig,
};

use crate::hooks::HookRegistry;

use rule::{AuditLogHook, RuleHook};
use webhook::OutboundWebhookHook;

/// Register bundled built-in hooks that ship with IronClaw.
pub async fn register_bundled_hooks(registry: &Arc<HookRegistry>) -> HookRegistrationSummary {
    registry
        .register_with_priority(Arc::new(AuditLogHook), 25)
        .await;

    HookRegistrationSummary {
        hooks: 1,
        outbound_webhooks: 0,
        errors: 0,
    }
}

/// Register all hooks from a declarative bundle.
pub async fn register_bundle(
    registry: &Arc<HookRegistry>,
    source: &str,
    bundle: HookBundleConfig,
) -> HookRegistrationSummary {
    let mut summary = HookRegistrationSummary::default();

    for rule in bundle.rules {
        match RuleHook::from_config(source, rule) {
            Ok((hook, priority)) => {
                registry
                    .register_with_priority(Arc::new(hook), priority)
                    .await;
                summary.hooks += 1;
            }
            Err(err) => {
                summary.errors += 1;
                tracing::warn!(source = source, error = %err, "Skipping invalid declarative hook rule");
            }
        }
    }

    for webhook in bundle.outbound_webhooks {
        match OutboundWebhookHook::from_config(source, webhook) {
            Ok((hook, priority)) => {
                registry
                    .register_with_priority(Arc::new(hook), priority)
                    .await;
                summary.outbound_webhooks += 1;
            }
            Err(err) => {
                summary.errors += 1;
                tracing::warn!(source = source, error = %err, "Skipping invalid outbound webhook hook");
            }
        }
    }

    summary
}
