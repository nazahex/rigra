//! Rigra core library.
//!
//! This crate exposes programmatic APIs for linting, formatting, and syncing
//! repository files according to TOML-based policies and an index file.
//!
//! High-level modules:
//! - `cli`: CLI argument parsing (binary uses this).
//! - `config`: Discovery and effective configuration resolution.
//! - `format`: Deterministic JSON formatting including ordering and line breaks.
//! - `lint`: Policy-driven validation, including order lint with message/level.
//! - `sync`: Template synchronization with scope gating.
//! - `models`: Data models for index, policy, and lint output structs.
//! - `output`: Human/JSON printers for lint/format/sync.
//! - `utils`: Supporting helpers.
//! - `checks`: Implementation of policy checks.
//!
//! Note: All documentation comments are written in English by convention.
pub mod checks;
pub mod cli;
pub mod config;
pub mod format;
pub mod lint;
pub mod models;
pub mod output;
pub mod sync;
pub mod utils;
