//! Dominion native Rust engine and embeddable SDK.
//! GUI HTTP server and Rust clients use the same runtime implementation.
mod api;
pub mod bootstrap;
mod config;
mod db;
mod engine;
mod events;
mod migrations;
mod platform;
mod state;
mod vault;
pub mod sdk;
