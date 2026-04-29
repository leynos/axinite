//! Integration tests for channel implementations covering OpenAI
//! compatibility, relay, Telegram auth, WASM channels, and WebSocket gateway.

mod support;

#[path = "channels/openai_compat.rs"]
mod openai_compat;
#[path = "channels/relay.rs"]
mod relay;
#[path = "channels/skills_upload.rs"]
mod skills_upload;
#[path = "channels/telegram_auth.rs"]
mod telegram_auth;
#[path = "channels/wasm_channel.rs"]
mod wasm_channel;
#[path = "channels/ws_gateway.rs"]
mod ws_gateway;
