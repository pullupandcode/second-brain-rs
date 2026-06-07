//! `second_brain_rs`: a framework-aware Second Brain vault MCP server.
//!
//! The crate is split into pure modules ([`config`], [`auth`], [`tools`]) and
//! an integration layer ([`mcp`], [`http`], [`observability`]) that wires them
//! onto an axum + rmcp Streamable-HTTP surface.

pub mod auth;
pub mod config;
pub mod http;
pub mod mcp;
pub mod observability;
pub mod tools;
