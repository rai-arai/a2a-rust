// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTTP client for calling remote A2A agents.
//!
//! The client module provides a high-level, typed interface for
//! interacting with A2A agents over HTTP. It handles agent discovery
//! (fetching the agent card from /.well-known/agent.json), JSON-RPC
//! envelope construction, and SSE stream parsing for streaming
//! operations.
//!
//! The client is built on reqwest and is fully async. It does not
//! spawn any background tasks or manage connections beyond what
//! reqwest's connection pool provides. Callers bring their own
//! async runtime (tokio, async-std, etc.).
//!
//! Gated behind the "client" feature flag.

pub mod discovery;
pub mod rpc;
pub(crate) mod sse;

pub use discovery::discover_agent;
pub use rpc::{A2AClient, ClientEventStream};
