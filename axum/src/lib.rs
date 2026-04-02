// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Axum transport adapter for serving A2A agents.
//!
//! Provides `a2a_router()` which builds an axum `Router` with:
//!   - POST / — JSON-RPC endpoint (request-response and SSE streaming)
//!   - GET /.well-known/agent.json — agent card discovery

mod router;

pub use router::a2a_router;
