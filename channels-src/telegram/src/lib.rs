// Generated WIT bindings and Telegram API DTOs intentionally include fields
// that are deserialised for compatibility but not read by every module.
#![allow(dead_code)]

wit_bindgen::generate!({
    world: "sandboxed-channel",
    path: "../../wit/channel.wit",
});

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
export!(TelegramChannel);
