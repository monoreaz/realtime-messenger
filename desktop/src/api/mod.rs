//! Backend access: REST client, wire models and the realtime WebSocket.
pub mod client;
pub mod models;
pub mod ws;

pub use client::{ApiError, Client};
