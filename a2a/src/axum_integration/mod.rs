// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Axum router integration for serving A2A agents.
//!
//! This module provides a thin adapter that wires an `A2AHandler`
//! implementation into an axum Router. It handles the HTTP concerns
//! that sit between the raw network and the JSON-RPC dispatch layer:
//!
//!   - POST endpoint for JSON-RPC requests (request-response and SSE)
//!   - GET endpoint for the well-known agent card (/.well-known/agent.json)
//!   - Content-Type negotiation (application/json vs text/event-stream)
//!   - SSE framing for streaming responses
//!
//! The router is constructed via `a2a_router()` which takes a shared
//! handler and an `AgentCard`. The handler is wrapped in an Arc and
//! stored as axum state—all requests share the same handler instance.
//!
//! Callers using `#[tokio::main]` must enable the `macros` and
//! `rt-multi-thread` features on their own `tokio` dependency.
//! The `axum` feature provides only `rt` and `sync` to keep
//! the mandatory feature set minimal.
//!
//! Gated behind the "axum" feature flag, which implies "server".
//!
//! **Security note:** The library does not enforce CSRF protection
//! or Origin header validation. Deployments accessible from browsers
//! should add appropriate middleware (e.g. `tower-http` `CorsLayer`)
//! to prevent cross-site request forgery.

mod router;

pub use router::a2a_router;
