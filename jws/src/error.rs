// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Verification error types for the JWS module.
//
// Every failure mode during JWS signature verification is represented as a
// distinct variant here. Granular variants allow callers to distinguish
// between a malformed card (operator error), an unsupported algorithm
// (library limitation), and a genuine signature mismatch (potential
// tampering)—three situations that warrant very different responses.

use std::fmt;

use crate::JwsAlgorithm;

/// Errors that can occur during JWS signature verification.
///
/// Every variant carries enough context for the caller to emit actionable
/// diagnostics. Variants that wrap strings carry the raw error text from
/// the underlying crypto or parsing layer so operators can read it in logs
/// without decoding stack traces.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum VerificationError {
	/// The protected header could not be decoded or parsed as JSON.
	///
	/// The wrapped string describes the failure—either a Base64url
	/// decode error or a JSON parse error—so the operator knows
	/// exactly which step failed and what the input looked like.
	InvalidProtectedHeader(String),

	/// The protected header JSON contained no `alg` field.
	///
	/// Per RFC 7515 §4.1.1 the `alg` header parameter is mandatory.
	/// Without it there is no way to select the right verification path.
	MissingAlgorithm,

	/// The `alg` field names an algorithm this module does not support.
	///
	/// The wrapped string is the raw algorithm name from the header, so
	/// operators see the exact value rather than a generic message.
	UnsupportedAlgorithm(String),

	/// The signature bytes are syntactically invalid for the declared
	/// algorithm—for example, the wrong byte length for P-256 compact
	/// encoding, or a malformed PKCS#1 structure.
	InvalidSignature(String),

	/// The signature was syntactically valid but did not verify against
	/// the supplied key and payload.
	///
	/// This is the expected result when the card has been tampered with
	/// or when the wrong key is supplied. It is deliberately distinct from
	/// [`InvalidSignature`] so callers can distinguish parse failures from
	/// cryptographic rejections.
	SignatureInvalid,

	/// The algorithm declared in the protected header does not match the
	/// algorithm family of the key supplied for verification.
	///
	/// Both fields are included so the operator can see precisely which
	/// algorithm the header declared and which type the supplied key is,
	/// without having to cross-reference the original card.
	AlgorithmKeyMismatch {
		/// The algorithm named in the protected header.
		header_alg: JwsAlgorithm,
		/// A short label for the key's algorithm family (e.g. `"ES256"` or `"RS256"`).
		key_alg: &'static str,
	},

	/// The JWK object could not be parsed into a usable verification key.
	///
	/// The wrapped string names the missing or malformed field, for example
	/// `"missing 'x' field"` or `"invalid P-256 point"`.
	InvalidJwk(String),

	/// The agent card has no signatures to verify.
	///
	/// Callers that call `AgentCard::verify_signatures` on an unsigned card
	/// receive this error rather than `Ok(())`, making it explicit that
	/// integrity was not checked rather than silently succeeding.
	NoSignatures,

	/// The agent card could not be serialised to its canonical form
	/// for payload reconstruction. This is an internal error—it
	/// should not occur under normal conditions.
	Serialization(String),
}

impl fmt::Display for VerificationError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::InvalidProtectedHeader(message) => {
				write!(formatter, "invalid JWS protected header: {message}")
			}
			Self::MissingAlgorithm => {
				formatter.write_str("JWS protected header is missing the 'alg' field")
			}
			Self::UnsupportedAlgorithm(algorithm) => {
				write!(formatter, "unsupported JWS algorithm: {algorithm}")
			}
			Self::InvalidSignature(message) => {
				write!(formatter, "invalid signature encoding: {message}")
			}
			Self::SignatureInvalid => formatter.write_str("signature verification failed"),
			Self::AlgorithmKeyMismatch {
				header_alg,
				key_alg,
			} => {
				write!(
					formatter,
					"algorithm mismatch: header declares {header_alg} but key is {key_alg}"
				)
			}
			Self::InvalidJwk(message) => write!(formatter, "invalid JWK: {message}"),
			Self::NoSignatures => formatter.write_str("agent card has no signatures to verify"),
			Self::Serialization(message) => {
				write!(
					formatter,
					"failed to serialise agent card to canonical form: {message}"
				)
			}
		}
	}
}

impl std::error::Error for VerificationError {}
