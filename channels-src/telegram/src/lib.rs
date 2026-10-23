// Generated WIT bindings and Telegram API DTOs intentionally include fields
// that are deserialized for compatibility but not read by every module.
#![allow(dead_code)]

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

