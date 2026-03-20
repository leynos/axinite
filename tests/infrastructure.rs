//! Infrastructure integration tests covering heartbeat, pairing, provider
//! chaos, SIGHUP reload, and workspace functionality.

#[path = "infrastructure/heartbeat.rs"]
mod heartbeat;
#[path = "infrastructure/pairing.rs"]
mod pairing;
#[path = "infrastructure/provider_chaos.rs"]
mod provider_chaos;
#[path = "infrastructure/sighup_reload.rs"]
mod sighup_reload;
#[path = "infrastructure/workspace.rs"]
mod workspace;
