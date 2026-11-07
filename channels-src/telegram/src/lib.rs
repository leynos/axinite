// Generated WIT bindings and Telegram API DTOs intentionally include fields
// that are deserialized for compatibility but not read by every module.
#![allow(dead_code)]

//! Telegram channel for Axinite.
//!
//! This WASM component implements the channel interface for receiving
//! Telegram updates by webhook or polling and sending replies, status updates
//! and attachments back through the Telegram Bot API.

// The generated bindings live in a private module so the undocumented
// wit-bindgen items are not part of the crate's documented surface.
mod bindings {
    //! Guest bindings that wit-bindgen generates from the channel WIT world.

    wit_bindgen::generate!({
        world: "sandboxed-channel",
        path: "../../wit/channel.wit",
    });
}

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

// Export the component
bindings::export!(TelegramChannel with_types_in bindings);
