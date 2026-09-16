//! `second_brain_rs`: a framework-aware Second Brain vault MCP server.
//!
//! The crate is split into pure modules ([`config`], [`auth`], [`tools`]) and
//! an integration layer ([`mcp`], [`http`], [`observability`]) that wires them
//! onto an axum + rmcp Streamable-HTTP surface.

pub mod auth;
pub mod config;
mod framework;
pub mod http;
pub mod mcp;
pub mod observability;
pub mod runtime;
pub mod tools;
pub mod vault;

/// Vault-backed skill prompts.
pub mod skills;

/// In-memory OCR job contract.
pub mod ocr;
