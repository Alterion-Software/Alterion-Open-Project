//! AOP Collaborate: the server behind sharing a plan.
//!
//! A plan's history is an append-only log of the commands that made it, and
//! this service is the shared copy of that log. It holds no scheduling logic
//! and no opinion about what a command means: it orders commands, hands out
//! the ones a client has not seen, and refuses to lose any.
//!
//! Two decisions shape everything here.
//!
//! The first is that the server never merges. A client pushes work it made
//! against a cursor; if the server has moved on, the push is answered with
//! what the client missed and the client replays its own commands on top.
//! Merging on the server would mean the server understanding the commands,
//! which is the whole scheduling engine, and it would mean two answers to
//! "what happened" instead of one.
//!
//! The second is that identity lives elsewhere. The Alterion identity
//! provider is already written, already live, and already self-hostable, so
//! this is a resource server: it takes a bearer token and asks the issuer
//! whether it is real. Everything about the issuer beyond its base URL comes
//! from its discovery document, so somebody self-hosting changes one setting.

pub mod auth;
pub mod config;
pub mod entity;
pub mod error;
pub mod handlers;
pub mod live;
pub mod schema;
pub mod state;
pub mod sync;

#[cfg(test)]
mod tests;

pub use config::Config;
pub use error::SyncError;
pub use state::AppState;
