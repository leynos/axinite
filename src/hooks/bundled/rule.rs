//! Built-in audit hook and declarative rule hooks compiled from
//! [`HookRuleConfig`] (guards, rejections, and regex/string rewrites).

use std::time::Duration;

use regex::Regex;

use crate::hooks::{
    HookContext, HookError, HookEvent, HookFailureMode, HookOutcome, HookPoint, NativeHook,
};

use super::config::{HookBundleError, HookRuleConfig, RegexReplacementConfig, timeout_from_ms};

const DEFAULT_RULE_PRIORITY: u32 = 100;

const ALL_HOOK_POINTS: [HookPoint; 6] = [
    HookPoint::BeforeInbound,
    HookPoint::BeforeToolCall,
    HookPoint::BeforeOutbound,
    HookPoint::OnSessionStart,
    HookPoint::OnSessionEnd,
    HookPoint::TransformResponse,
];

/// Built-in audit trail hook that logs lifecycle events.
pub(super) struct AuditLogHook;

impl NativeHook for AuditLogHook {
    fn name(&self) -> &str {
        "builtin.audit_log"
    }

    fn hook_points(&self) -> &[HookPoint] {
        &ALL_HOOK_POINTS
    }

    async fn execute<'a>(
        &'a self,
        event: &'a HookEvent,
        _ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        tracing::debug!(
            target: "hooks::audit",
            hook = NativeHook::name(self),
            point = event.hook_point().as_str(),
            user_id = %event_user_id(event),
            "Lifecycle hook event"
        );

        Ok(HookOutcome::ok())
    }
}

#[derive(Debug, Clone)]
struct CompiledReplacement {
    regex: Regex,
    replacement: String,
}

/// Runtime hook compiled from [`HookRuleConfig`].
#[derive(Debug)]
pub(super) struct RuleHook {
    name: String,
    points: Vec<HookPoint>,
    failure_mode: HookFailureMode,
    timeout: Duration,
    when_regex: Option<Regex>,
    reject_reason: Option<String>,
    replacements: Vec<CompiledReplacement>,
    prepend: Option<String>,
    append: Option<String>,
}

impl RuleHook {
    pub(super) fn from_config(
        source: &str,
        config: HookRuleConfig,
    ) -> Result<(Self, u32), HookBundleError> {
        let scoped_name = format!("{}::{}", source, config.name);

        if config.points.is_empty() {
            return Err(HookBundleError::MissingHookPoints { hook: scoped_name });
        }

        let timeout = timeout_from_ms(config.timeout_ms, &scoped_name)?;
        let when_regex = compile_when_regex(config.when_regex, &scoped_name)?;
        let replacements = compile_replacements(config.replacements, &scoped_name)?;

        let hook = Self {
            name: scoped_name,
            points: config.points,
            failure_mode: config.failure_mode.unwrap_or(HookFailureMode::FailOpen),
            timeout,
            when_regex,
            reject_reason: config.reject_reason,
            replacements,
            prepend: config.prepend,
            append: config.append,
        };

        if hook.is_inert_guard() {
            tracing::warn!(
                hook = %hook.name,
                "Rule hook has a guard but no actions; it will always no-op"
            );
        }

        Ok((hook, config.priority.unwrap_or(DEFAULT_RULE_PRIORITY)))
    }

    /// Whether this hook has a `when` guard but nothing it would do on match.
    fn is_inert_guard(&self) -> bool {
        self.when_regex.is_some() && !self.has_actions()
    }

    /// Whether the hook defines a rejection or any content rewrite action.
    fn has_actions(&self) -> bool {
        self.reject_reason.is_some() || self.rewrites_content()
    }

    /// Whether the hook replaces, prepends, or appends content.
    fn rewrites_content(&self) -> bool {
        let inserts = self.prepend.is_some() || self.append.is_some();
        !self.replacements.is_empty() || inserts
    }
}

/// Compile a rule hook's optional `when_regex` guard, naming `hook` on error.
fn compile_when_regex(
    pattern: Option<String>,
    hook: &str,
) -> Result<Option<Regex>, HookBundleError> {
    match pattern {
        Some(pattern) => Ok(Some(Regex::new(&pattern).map_err(|e| {
            HookBundleError::InvalidRegex {
                hook: hook.to_string(),
                pattern,
                reason: e.to_string(),
            }
        })?)),
        None => Ok(None),
    }
}

/// Compile a rule hook's replacement patterns in order, naming `hook` on error.
fn compile_replacements(
    raw: Vec<RegexReplacementConfig>,
    hook: &str,
) -> Result<Vec<CompiledReplacement>, HookBundleError> {
    let mut replacements = Vec::with_capacity(raw.len());
    for replacement in raw {
        let compiled =
            Regex::new(&replacement.pattern).map_err(|e| HookBundleError::InvalidRegex {
                hook: hook.to_string(),
                pattern: replacement.pattern.clone(),
                reason: e.to_string(),
            })?;
        replacements.push(CompiledReplacement {
            regex: compiled,
            replacement: replacement.replacement,
        });
    }
    Ok(replacements)
}

impl NativeHook for RuleHook {
    fn name(&self) -> &str {
        &self.name
    }

    fn hook_points(&self) -> &[HookPoint] {
        &self.points
    }

    fn failure_mode(&self) -> HookFailureMode {
        self.failure_mode
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    async fn execute<'a>(
        &'a self,
        event: &'a HookEvent,
        _ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        let content = extract_primary_content(event);

        if let Some(ref guard) = self.when_regex
            && !guard.is_match(&content)
        {
            return Ok(HookOutcome::ok());
        }

        if let Some(ref reason) = self.reject_reason {
            return Ok(HookOutcome::reject(reason.clone()));
        }

        let mut modified = content.clone();

        for replacement in &self.replacements {
            modified = replacement
                .regex
                .replace_all(&modified, replacement.replacement.as_str())
                .into_owned();
        }

        if let Some(ref prefix) = self.prepend {
            modified = format!("{}{}", prefix, modified);
        }

        if let Some(ref suffix) = self.append {
            modified.push_str(suffix);
        }

        if modified != content {
            Ok(HookOutcome::modify(modified))
        } else {
            Ok(HookOutcome::ok())
        }
    }
}

fn event_user_id(event: &HookEvent) -> &str {
    match event {
        HookEvent::Inbound { user_id, .. }
        | HookEvent::ToolCall { user_id, .. }
        | HookEvent::Outbound { user_id, .. }
        | HookEvent::SessionStart { user_id, .. }
        | HookEvent::SessionEnd { user_id, .. }
        | HookEvent::ResponseTransform { user_id, .. } => user_id,
    }
}

fn extract_primary_content(event: &HookEvent) -> String {
    match event {
        HookEvent::Inbound { content, .. } | HookEvent::Outbound { content, .. } => content.clone(),
        HookEvent::ToolCall { parameters, .. } => {
            serde_json::to_string(parameters).unwrap_or_default()
        }
        HookEvent::SessionStart { session_id, .. } | HookEvent::SessionEnd { session_id, .. } => {
            session_id.clone()
        }
        HookEvent::ResponseTransform { response, .. } => response.clone(),
    }
}
