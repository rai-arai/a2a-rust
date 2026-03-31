// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// JWS detached-payload verification.
//
// A detached JWS (RFC 7515 §7.2) omits the payload from the serialised form.
// The caller supplies the raw payload separately so this function can
// reconstruct the signing input—BASE64URL(header) || "." || BASE64URL(payload)—
// and verify the signature over it. This is the format used for AgentCard
// signatures: the payload is the card's canonical JSON with the `signatures`
// field removed, and the signature is stored in the card's `signatures` array.
//
// The function is intentionally narrow: one protected header, one signature,
// one payload, one key. Orchestrating multiple signatures over a single card
// is the responsibility of AgentCard::verify_signatures.

use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::jws::key::VerificationKeyInner;
use crate::jws::{JwsAlgorithm, VerificationError, VerificationKey};

/// Verify a single detached-payload JWS segment.
///
/// Reconstructs the RFC 7515 signing input from the protected header and
/// payload, then verifies the signature using the supplied public key.
/// The `protected_b64` and `signature_b64` strings are taken exactly as
/// they appear in the JWS object—no re-encoding is performed.
///
/// Returns the [`JwsAlgorithm`] declared in the protected header on success.
/// This lets callers record which algorithm was actually used without
/// re-parsing the header.
///
/// # Errors
///
/// Returns [`VerificationError`] if the protected header cannot be decoded or
/// parsed, the algorithm is unsupported, the key type does not match the
/// declared algorithm, or the signature does not verify.
pub fn verify_detached_jws(
	protected_b64: &str,
	signature_b64: &str,
	payload: &[u8],
	key: &VerificationKey,
) -> Result<JwsAlgorithm, VerificationError> {
	// Decode and parse the protected header.
	let header_bytes = URL_SAFE_NO_PAD.decode(protected_b64).map_err(|error| {
		VerificationError::InvalidProtectedHeader(format!("base64url decode failed: {error}"))
	})?;

	let header: serde_json::Value = serde_json::from_slice(&header_bytes).map_err(|error| {
		VerificationError::InvalidProtectedHeader(format!("JSON parse failed: {error}"))
	})?;

	// Extract the mandatory `alg` field.
	let algorithm_string = header
		.get("alg")
		.and_then(|value| value.as_str())
		.ok_or(VerificationError::MissingAlgorithm)?;

	let algorithm = JwsAlgorithm::from_str(algorithm_string)?;

	// Reject mismatches between the declared algorithm and the key type
	// before touching the signature bytes. This gives operators a specific
	// error message rather than a confusing cryptographic failure.
	check_algorithm_key_match(algorithm, &key.inner)?;

	// Reconstruct the JWS signing input per RFC 7515 §7.2.6.
	// For detached payloads the caller supplies the raw bytes; we
	// Base64url-encode them here to reproduce the original signing input.
	let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
	let signing_input = format!("{protected_b64}.{encoded_payload}");

	// Decode the signature bytes.
	let signature_bytes = URL_SAFE_NO_PAD.decode(signature_b64).map_err(|error| {
		VerificationError::InvalidSignature(format!("base64url decode failed: {error}"))
	})?;

	// Dispatch to the algorithm-specific verification path.
	match &key.inner {
		VerificationKeyInner::Es256(verifying_key) => {
			verify_es256(verifying_key, signing_input.as_bytes(), &signature_bytes)?;
		}
		VerificationKeyInner::Rs256(public_key) => {
			verify_rs256(public_key, signing_input.as_bytes(), &signature_bytes)?;
		}
	}

	Ok(algorithm)
}

// Verify that the algorithm declared in the header is compatible with the
// supplied key type. Catches operator configuration mistakes—such as supplying
// an EC key for an RS256 header—before the cryptographic layer, which would
// return an opaque error.
fn check_algorithm_key_match(
	algorithm: JwsAlgorithm,
	inner: &VerificationKeyInner,
) -> Result<(), VerificationError> {
	match (algorithm, inner) {
		(JwsAlgorithm::Es256, VerificationKeyInner::Es256(_))
		| (JwsAlgorithm::Rs256, VerificationKeyInner::Rs256(_)) => Ok(()),
		(JwsAlgorithm::Es256, VerificationKeyInner::Rs256(_)) => {
			Err(VerificationError::AlgorithmKeyMismatch {
				header_alg: algorithm,
				key_alg: "RS256",
			})
		}
		(JwsAlgorithm::Rs256, VerificationKeyInner::Es256(_)) => {
			Err(VerificationError::AlgorithmKeyMismatch {
				header_alg: algorithm,
				key_alg: "ES256",
			})
		}
	}
}

// Verify an ES256 (ECDSA P-256 SHA-256) signature.
//
// The signature bytes must be in compact (IEEE P1363) encoding—64 bytes for
// P-256. The p256 crate also accepts DER encoding via `from_der`, but the JWS
// spec (RFC 7518 §3.4) mandates compact encoding, so `from_slice` is the
// correct parser here.
fn verify_es256(
	verifying_key: &p256::ecdsa::VerifyingKey,
	signing_input: &[u8],
	signature_bytes: &[u8],
) -> Result<(), VerificationError> {
	use p256::ecdsa::signature::Verifier;

	let signature = p256::ecdsa::Signature::from_slice(signature_bytes).map_err(|error| {
		VerificationError::InvalidSignature(format!("invalid ECDSA signature encoding: {error}"))
	})?;

	verifying_key
		.verify(signing_input, &signature)
		.map_err(|_| VerificationError::SignatureInvalid)
}

// Verify an RS256 (RSASSA-PKCS1-v1_5 SHA-256) signature.
//
// The rsa crate's pkcs1v15::VerifyingKey is constructed each call rather than
// cached because the key is behind an immutable reference and the type does not
// implement Clone in a way that would allow pre-construction. The construction
// is cheap—it wraps the existing RsaPublicKey without copying key bytes.
fn verify_rs256(
	public_key: &rsa::RsaPublicKey,
	signing_input: &[u8],
	signature_bytes: &[u8],
) -> Result<(), VerificationError> {
	use rsa::signature::Verifier;

	let signature = rsa::pkcs1v15::Signature::try_from(signature_bytes).map_err(|error| {
		VerificationError::InvalidSignature(format!("invalid RSA signature encoding: {error}"))
	})?;

	let verifying_key = rsa::pkcs1v15::VerifyingKey::<sha2::Sha256>::new(public_key.clone());

	verifying_key
		.verify(signing_input, &signature)
		.map_err(|_| VerificationError::SignatureInvalid)
}
