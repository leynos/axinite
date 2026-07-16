//! Request and response DTOs for the web gateway API.
//!
//! Split by API area; every DTO is re-exported here so existing
//! `types::*` paths keep working.

mod chat;
mod events;
mod extensions;
mod jobs;
mod memory;
mod routines;
mod settings;

pub use chat::*;
pub use events::*;
pub use extensions::*;
pub use jobs::*;
pub use memory::*;
pub use routines::*;
pub use settings::*;

#[cfg(test)]
mod tests;
