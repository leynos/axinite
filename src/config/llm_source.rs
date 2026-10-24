//! Parameter object for re-resolving the LLM configuration.

/// Where an LLM re-resolution reads its persisted settings from.
///
/// Bundles the optional settings store, the owning user, and the optional TOML
/// overlay so [`super::Config::re_resolve_llm_with_context`] stays within the
/// project's argument-count ceiling.
#[derive(Clone, Copy)]
pub struct LlmSettingsSource<'a> {
    /// Settings store consulted for persisted values, when one is available.
    pub store: Option<&'a (dyn crate::db::SettingsStore + Sync)>,
    /// User whose settings are loaded from `store`.
    pub user_id: &'a str,
    /// Optional TOML file layered over the stored settings.
    pub toml_path: Option<&'a std::path::Path>,
}
