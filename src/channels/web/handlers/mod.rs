//! Handler modules for the web gateway API.
//!
//! Each module groups related endpoint handlers by domain.

pub mod chat;
pub mod chat_auth;
pub mod chat_history;
pub mod chat_threads;
pub(crate) mod common;
pub mod extensions;
pub(crate) mod install_helpers;
pub mod job_control;
pub mod job_files;
pub mod jobs;
pub mod memory;
pub mod oauth;
pub mod oauth_slack;
pub mod pairing;
pub mod routines;
pub mod settings;
pub mod skills;
pub mod static_files;
