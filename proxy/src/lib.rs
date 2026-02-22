//! QueryProxy - PostgreSQL wire protocol proxy with query governance
//!
//! This library provides the core components for the QueryProxy server.

pub mod admin;
pub mod arrow_conversion;
pub mod auth;
pub mod crypto;
pub mod discovery;
pub mod engine;
pub mod entity;
pub mod handler;
pub mod hooks;
pub mod sql_rewrite;
