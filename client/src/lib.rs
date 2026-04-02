// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTTP client for calling remote A2A agents.
//!
//! Provides `A2AClient` for typed JSON-RPC calls, `discover_agent()`
//! for fetching agent cards from the well-known endpoint, and SSE
//! stream parsing for streaming operations.

pub mod discovery;
pub mod rpc;
pub mod sse;

pub use discovery::discover_agent;
pub use rpc::A2AClient;
pub use sse::into_event_stream;
