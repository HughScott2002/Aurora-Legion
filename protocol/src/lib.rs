//! Shared types for the Legion keyboard daemon, GUI and CLI.
//!
//! This crate is UI-free on purpose: it holds the domain types (effects,
//! profiles, custom effects), file storage helpers and the IPC schema that
//! the daemon and its clients speak over the unix socket.

pub mod custom_effect;
pub mod effects;
pub mod ipc;
pub mod profile;
pub mod storage;
