//! Test hook implementations and event fixtures for registry tests.

use std::time::Duration;

use crate::hooks::hook::{
    HookContext, HookError, HookEvent, HookFailureMode, HookOutcome, HookPoint, NativeHook,
};

use super::super::extract_content;

/// A test hook that always returns ok.
pub(super) struct PassthroughHook {
    pub(super) name: String,
    pub(super) points: Vec<HookPoint>,
}

impl NativeHook for PassthroughHook {
    fn name(&self) -> &str {
        &self.name
    }
    fn hook_points(&self) -> &[HookPoint] {
        &self.points
    }
    async fn execute<'a>(
        &'a self,
        _event: &'a HookEvent,
        _ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        Ok(HookOutcome::ok())
    }
}

/// A hook that modifies content by appending a suffix.
pub(super) struct ModifyHook {
    pub(super) name: String,
    pub(super) suffix: String,
    pub(super) points: Vec<HookPoint>,
}

impl NativeHook for ModifyHook {
    fn name(&self) -> &str {
        &self.name
    }
    fn hook_points(&self) -> &[HookPoint] {
        &self.points
    }
    async fn execute<'a>(
        &'a self,
        event: &'a HookEvent,
        _ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        let content = extract_content(event);
        Ok(HookOutcome::modify(format!("{}{}", content, self.suffix)))
    }
}

/// A hook that always rejects.
pub(super) struct RejectHook {
    pub(super) name: String,
    pub(super) reason: String,
    pub(super) points: Vec<HookPoint>,
}

impl NativeHook for RejectHook {
    fn name(&self) -> &str {
        &self.name
    }
    fn hook_points(&self) -> &[HookPoint] {
        &self.points
    }
    async fn execute<'a>(
        &'a self,
        _event: &'a HookEvent,
        _ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        Ok(HookOutcome::reject(&self.reason))
    }
}

/// A hook that always errors.
pub(super) struct ErrorHook {
    pub(super) name: String,
    pub(super) points: Vec<HookPoint>,
    pub(super) failure_mode: HookFailureMode,
}

impl NativeHook for ErrorHook {
    fn name(&self) -> &str {
        &self.name
    }
    fn hook_points(&self) -> &[HookPoint] {
        &self.points
    }
    fn failure_mode(&self) -> HookFailureMode {
        self.failure_mode
    }
    async fn execute<'a>(
        &'a self,
        _event: &'a HookEvent,
        _ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        Err(HookError::ExecutionFailed {
            reason: "test error".into(),
        })
    }
}

/// A hook that sleeps longer than its timeout.
pub(super) struct SlowHook {
    pub(super) name: String,
    pub(super) points: Vec<HookPoint>,
    pub(super) failure_mode: HookFailureMode,
}

impl NativeHook for SlowHook {
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
        Duration::from_millis(50)
    }
    async fn execute<'a>(
        &'a self,
        _event: &'a HookEvent,
        _ctx: &'a HookContext,
    ) -> Result<HookOutcome, HookError> {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(HookOutcome::ok())
    }
}

pub(super) fn test_event() -> HookEvent {
    HookEvent::Inbound {
        user_id: "user-1".into(),
        channel: "test".into(),
        content: "hello".into(),
        thread_id: None,
    }
}
