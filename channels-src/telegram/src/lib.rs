#![allow(dead_code)]

wit_bindgen::generate!({
    world: "sandboxed-channel",
    path: "../../wit/channel.wit",
});

use exports::near::agent::channel::{
    AgentResponse, Attachment, ChannelConfig, Guest, HttpEndpointConfig, IncomingHttpRequest,
    OutgoingHttpResponse, PollConfig, StatusType, StatusUpdate,
};
use near::agent::channel_host::{self, EmittedMessage, InboundAttachment};

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
