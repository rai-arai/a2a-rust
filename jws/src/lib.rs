// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JWS (RFC 7515) signature verification for A2A agent cards.
//!
//! Provides everything needed to verify the JWS signatures attached to
//! an `AgentCard`. Supported algorithms: ES256 (ECDSA P-256 SHA-256)
//! and RS256 (RSASSA-PKCS1-v1_5 SHA-256). Pure Rust via `RustCrypto`
//! crates—no native dependencies, Wasm-compatible.
//!
//! Import `AgentCardVerify` to call `card.verify_signatures(keys)`.

mod agent_card_ext;
mod algorithm;
mod error;
mod key;
mod verify;

#[cfg(test)]
mod tests;

pub use agent_card_ext::AgentCardVerify;
pub use algorithm::JwsAlgorithm;
pub use error::VerificationError;
pub use key::{VerificationKey, parse_jwks};
pub use verify::verify_detached_jws;
