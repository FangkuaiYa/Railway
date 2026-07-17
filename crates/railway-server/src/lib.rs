//! # railway-server
//!
//! The Among Us private server runtime. Glues together:
//! - `railway-hazel` for UDP networking
//! - `railway-protocol` for message parsing
//! - `railway-game-logic` for game state management
//!
//! Owns the tokio runtime, UDP socket, and optional HTTP API.

pub mod client;
pub mod client_manager;
pub mod config;
pub mod game_manager;
pub mod http;
pub mod logging;
pub mod matchmaker;
pub mod message_router;
pub mod reactor;
