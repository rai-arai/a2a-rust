// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// JWS algorithm identifier.
//
// A JWS protected header carries an `alg` field that names the signature
// algorithm used to produce the signature over that header and its payload.
// This type represents the subset of algorithms that this crate can verify.
// Anything outside that subset is rejected at parse time so that callers
// receive a clear, early error rather than a confusing failure during the
// cryptographic step.

use std::fmt;
use std::str::FromStr;

use crate::jws::VerificationError;

/// The signature algorithm declared in a JWS protected header.
///
/// Only algorithms that this module can verify are represented here;
/// any other `alg` value is rejected with
/// [`VerificationError::UnsupportedAlgorithm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JwsAlgorithm {
	/// ECDSA using P-256 and SHA-256 (RFC 7518 §3.4).
	Es256,

	/// RSASSA-PKCS1-v1_5 using SHA-256 (RFC 7518 §3.3).
	Rs256,
}

impl FromStr for JwsAlgorithm {
	type Err = VerificationError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value {
			"ES256" => Ok(Self::Es256),
			"RS256" => Ok(Self::Rs256),
			other => Err(VerificationError::UnsupportedAlgorithm(other.to_string())),
		}
	}
}

impl fmt::Display for JwsAlgorithm {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Es256 => formatter.write_str("ES256"),
			Self::Rs256 => formatter.write_str("RS256"),
		}
	}
}
