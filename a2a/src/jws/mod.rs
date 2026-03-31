// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JWS (RFC 7515) signature verification for A2A agent cards.
//!
//! This module provides everything needed to verify the JWS signatures attached
//! to an `AgentCard`. Agent operators sign their card's canonical JSON to let
//! callers confirm that the card has not been tampered with in transit—important
//! when cards are cached in registries or proxied through intermediaries.
//!
//! The implementation is feature-gated behind the `jws` feature flag. All
//! cryptographic operations are performed by `RustCrypto` crates (`p256`, `rsa`,
//! `sha2`) which have no native dependencies and compile to Wasm without
//! modification.
//!
//! Supported algorithms: ES256 (ECDSA P-256 SHA-256), RS256 (RSASSA-PKCS1-v1_5
//! SHA-256). Both are widely deployed for JWS in the ecosystem. `EdDSA` and other
//! algorithms return `UnsupportedAlgorithm` so callers receive a clear error.
//!
//! Public surface:
//!   [`JwsAlgorithm`]        — the set of supported algorithm identifiers
//!   [`VerificationError`]   — all failure modes during key parsing and verification
//!   [`VerificationKey`]     — a parsed public key ready for use in [`verify_detached_jws`]
//!   [`verify_detached_jws`] — verify one JWS segment against a known payload
//!   [`parse_jwks`]          — parse a JWKS document into a list of `VerificationKey`s

mod algorithm;
mod error;
mod key;
mod verify;

#[cfg(test)]
mod tests;

pub use algorithm::JwsAlgorithm;
pub use error::VerificationError;
pub use key::{VerificationKey, parse_jwks};
pub use verify::verify_detached_jws;
