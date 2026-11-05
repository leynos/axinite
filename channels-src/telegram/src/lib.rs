//! Telegram channel component for Axinite.
//!
//! The guest logic handles Telegram webhook updates and host-mediated API
//! operations. Its ordinary modules contain safe channel behaviour; the
//! `bindings` module contains generated component ABI glue and is the crate's
//! only unsafe boundary.

#[expect(unsafe_code, reason = "wit-bindgen component export ABI")]
mod bindings;

use bindings::{exports, near};

mod attachments;
mod downloads;
mod guest;
mod inbound;
mod polling;
mod send;
mod state;
mod status;
mod types;
mod webhook;

#[cfg(test)]
mod tests;

struct TelegramChannel;
