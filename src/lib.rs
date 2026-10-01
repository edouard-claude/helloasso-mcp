//! A Model Context Protocol server for [HelloAsso](https://www.helloasso.com), the
//! payment platform French associations use for memberships, events, donations
//! and shops.
//!
//! The crate is split in two halves:
//!
//! * [`client`] is a plain async client for the HelloAsso v5 HTTP API, with no
//!   MCP in sight: the OAuth2 token lifecycle, retries, and the API's error
//!   envelopes are handled there.
//! * [`server`] and [`tools`] turn that client into an MCP server: tools,
//!   resources and prompts, built on the official [`rmcp`] SDK.
//!
//! Everything is single tenant: one API client, read from the environment, for
//! the whole process.

pub mod client;
pub mod config;
pub mod dates;
pub mod error;
pub mod prompts;
pub mod resolve;
pub mod resources;
pub mod server;
pub mod tools;

pub use client::HelloAsso;
pub use config::Config;
pub use error::{Error, Result};
pub use server::HelloAssoServer;

/// The server name advertised during MCP initialization.
pub const SERVER_NAME: &str = "helloasso";

/// The crate version, advertised during MCP initialization.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
