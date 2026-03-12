//! Tests for the orchestrator API.

#![allow(
    unused_imports,
    reason = "test submodules share this prelude via super::*"
)]

pub(super) use std::collections::HashMap;
pub(super) use std::sync::Arc;
pub(super) use std::time::Duration;

pub(super) use axum::body::Body;
pub(super) use axum::http::{Request, StatusCode};
pub(super) use tokio::sync::{Mutex, broadcast};
pub(super) use tower::ServiceExt;
pub(super) use uuid::Uuid;

pub(super) use crate::orchestrator::auth::TokenStore;
pub(super) use crate::orchestrator::job_manager::{ContainerJobConfig, ContainerJobManager};
pub(super) use crate::testing::StubLlm;
pub(super) use crate::tools::{Tool, ToolOutput, ToolRegistry};

use super::*;

mod auth;
mod credentials;
mod events;
mod extension_tool;
mod fixtures;
mod prompts;
mod status;
