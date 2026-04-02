// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Verification key types and JWKS parsing.
//
// A VerificationKey wraps a parsed public key from either the P-256 ECDSA
// or RSA family. It is the central input to `verify_detached_jws`—callers
// obtain one by parsing a JWK object from the agent operator's JWKS endpoint
// or by supplying raw key material directly.
//
// The inner key is kept private so that callers cannot inspect or extract raw
// key bytes—key material should not leave the type boundary unnecessarily.
// The `kid` field is public because callers need it to match a signature's
// key hint to the right key in a set without attempting every key.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rsa::traits::PublicKeyParts;

use crate::VerificationError;

/// A public key used to verify a JWS signature.
///
/// Constructed via [`VerificationKey::from_jwk`], or directly via
/// [`VerificationKey::es256_from_encoded_point`] and
/// [`VerificationKey::rs256_from_components`].
///
/// The `kid` field mirrors the `kid` (key ID) parameter from the source JWK.
/// `AgentCard::verify_signatures` uses it to match a signature's key hint to
/// the correct key in a set before falling back to trying every key.
pub struct VerificationKey {
	/// The key ID from the source JWK, if present.
	///
	/// When a JWS signature includes a `kid` in its protected or unprotected
	/// header, the key selection code can use this to avoid trying every key.
	pub kid: Option<String>,

	/// The wrapped cryptographic key material.
	///
	/// Private—callers interact with the key only through the verification
	/// functions exported by this module.
	pub(crate) inner: VerificationKeyInner,
}

// The actual key material, discriminated by algorithm family.
// Each variant owns the parsed public key object from the corresponding
// RustCrypto crate.
pub(crate) enum VerificationKeyInner {
	Es256(p256::ecdsa::VerifyingKey),
	Rs256(rsa::RsaPublicKey),
}

impl fmt::Debug for VerificationKey {
	// Debug output intentionally omits the key bytes.
	//
	// Printing raw key material in debug output creates a risk of accidental
	// exposure in log aggregation systems. The output shows only the algorithm
	// family and `kid` so that log lines remain useful without leaking the key.
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let algorithm_label = match &self.inner {
			VerificationKeyInner::Es256(_) => "ES256",
			VerificationKeyInner::Rs256(_) => "RS256",
		};
		formatter
			.debug_struct("VerificationKey")
			.field("alg", &algorithm_label)
			.field("kid", &self.kid)
			.finish_non_exhaustive()
	}
}

impl VerificationKey {
	/// Parse a JWK object into a [`VerificationKey`].
	///
	/// Supported JWK types:
	///
	/// - EC keys (`"kty": "EC"`) with `"crv": "P-256"` produce an ES256 key.
	/// - RSA keys (`"kty": "RSA"`) produce an RS256 key.
	///
	/// For EC keys the `x` and `y` fields must be Base64url-encoded 32-byte
	/// P-256 field elements. For RSA keys `n` and `e` must be Base64url-encoded
	/// big-endian byte sequences.
	///
	/// # Errors
	///
	/// Returns [`VerificationError::UnsupportedAlgorithm`] for key types this
	/// module cannot use, and [`VerificationError::InvalidJwk`] for any
	/// structural or encoding problem within a supported key type.
	pub fn from_jwk(jwk: &serde_json::Value) -> Result<Self, VerificationError> {
		let key_type = jwk
			.get("kty")
			.and_then(|value| value.as_str())
			.ok_or_else(|| VerificationError::InvalidJwk("missing 'kty' field".into()))?;

		let kid = jwk
			.get("kid")
			.and_then(|value| value.as_str())
			.map(String::from);

		match key_type {
			"EC" => parse_ec_jwk(jwk, kid),
			"RSA" => parse_rsa_jwk(jwk, kid),
			other => Err(VerificationError::UnsupportedAlgorithm(format!(
				"JWK key type '{other}' is not supported"
			))),
		}
	}

	/// Construct an ES256 verification key from a SEC1 uncompressed point.
	///
	/// The `bytes` slice must be the 65-byte SEC1 uncompressed point encoding:
	/// `0x04 || x || y`, where `x` and `y` are the 32-byte big-endian P-256
	/// field elements. This is the format exported by most ECDSA implementations
	/// when they serialise a public key without compression.
	///
	/// # Errors
	///
	/// Returns [`VerificationError::InvalidJwk`] if the bytes do not
	/// represent a valid point on the P-256 curve.
	pub fn es256_from_encoded_point(bytes: &[u8]) -> Result<Self, VerificationError> {
		let encoded_point = p256::EncodedPoint::from_bytes(bytes).map_err(|error| {
			VerificationError::InvalidJwk(format!("invalid SEC1 point encoding: {error}"))
		})?;

		let verifying_key =
			p256::ecdsa::VerifyingKey::from_encoded_point(&encoded_point).map_err(|error| {
				VerificationError::InvalidJwk(format!("invalid P-256 point: {error}"))
			})?;

		Ok(Self {
			kid: None,
			inner: VerificationKeyInner::Es256(verifying_key),
		})
	}

	/// Construct an RS256 verification key from raw modulus and exponent bytes.
	///
	/// Both `n` (modulus) and `e` (public exponent) are big-endian unsigned
	/// byte sequences. This matches the JWK `n` and `e` fields after
	/// Base64url-decoding, so callers who already have decoded bytes can
	/// skip the JWK parsing layer.
	///
	/// # Errors
	///
	/// Returns [`VerificationError::InvalidJwk`] if the modulus and exponent
	/// do not form a valid RSA public key, or if the resulting key is shorter
	/// than 2048 bits.
	pub fn rs256_from_components(
		modulus: &[u8],
		exponent: &[u8],
	) -> Result<Self, VerificationError> {
		let modulus_uint = rsa::BigUint::from_bytes_be(modulus);
		let exponent_uint = rsa::BigUint::from_bytes_be(exponent);

		let public_key = rsa::RsaPublicKey::new(modulus_uint, exponent_uint).map_err(|error| {
			VerificationError::InvalidJwk(format!("invalid RSA key components: {error}"))
		})?;

		// Reject keys shorter than 2048 bits. Keys smaller than this are
		// considered cryptographically weak—any signature produced by a
		// sub-2048-bit RSA key should be treated as untrustworthy.
		if public_key.size() < 256 {
			return Err(VerificationError::InvalidJwk(
				"RSA key must be at least 2048 bits".into(),
			));
		}

		Ok(Self {
			kid: None,
			inner: VerificationKeyInner::Rs256(public_key),
		})
	}
}

// Parse the EC variant of a JWK object into a VerificationKey.
//
// Only P-256 is supported; any other curve is rejected immediately so the
// caller sees a clear UnsupportedAlgorithm error rather than a confusing
// downstream failure during point construction.
fn parse_ec_jwk(
	jwk: &serde_json::Value,
	kid: Option<String>,
) -> Result<VerificationKey, VerificationError> {
	let curve = jwk
		.get("crv")
		.and_then(|value| value.as_str())
		.ok_or_else(|| VerificationError::InvalidJwk("missing 'crv' field".into()))?;

	if curve != "P-256" {
		return Err(VerificationError::UnsupportedAlgorithm(format!(
			"EC curve '{curve}' is not supported; only P-256 is"
		)));
	}

	let x_b64 = jwk
		.get("x")
		.and_then(|value| value.as_str())
		.ok_or_else(|| VerificationError::InvalidJwk("missing 'x' field".into()))?;

	let y_b64 = jwk
		.get("y")
		.and_then(|value| value.as_str())
		.ok_or_else(|| VerificationError::InvalidJwk("missing 'y' field".into()))?;

	let x_bytes = URL_SAFE_NO_PAD
		.decode(x_b64)
		.map_err(|error| VerificationError::InvalidJwk(format!("bad base64url in 'x': {error}")))?;

	let y_bytes = URL_SAFE_NO_PAD
		.decode(y_b64)
		.map_err(|error| VerificationError::InvalidJwk(format!("bad base64url in 'y': {error}")))?;

	// P-256 field elements are exactly 32 bytes. Reject anything shorter or
	// longer immediately—FieldBytes::from([u8; 32]) requires the exact size.
	if x_bytes.len() != 32 {
		return Err(VerificationError::InvalidJwk(format!(
			"'x' coordinate must be 32 bytes, got {}",
			x_bytes.len()
		)));
	}
	if y_bytes.len() != 32 {
		return Err(VerificationError::InvalidJwk(format!(
			"'y' coordinate must be 32 bytes, got {}",
			y_bytes.len()
		)));
	}

	// Copy into fixed-size arrays so FieldBytes::from can accept them without
	// the deprecated GenericArray::from_slice path.
	let mut x_array = [0u8; 32];
	let mut y_array = [0u8; 32];
	x_array.copy_from_slice(&x_bytes);
	y_array.copy_from_slice(&y_bytes);

	let x_field = p256::FieldBytes::from(x_array);
	let y_field = p256::FieldBytes::from(y_array);

	let encoded_point = p256::EncodedPoint::from_affine_coordinates(&x_field, &y_field, false);

	let verifying_key = p256::ecdsa::VerifyingKey::from_encoded_point(&encoded_point)
		.map_err(|error| VerificationError::InvalidJwk(format!("invalid P-256 point: {error}")))?;

	Ok(VerificationKey {
		kid,
		inner: VerificationKeyInner::Es256(verifying_key),
	})
}

// Parse the RSA variant of a JWK object into a VerificationKey.
//
// Extracts the `n` and `e` fields, which RFC 7518 §6.3 requires to be
// Base64url-encoded big-endian unsigned integers with no leading zero bytes
// (other than the representation of zero itself). The rsa crate accepts
// any big-endian byte sequence, so we rely on it to catch degenerate inputs
// such as a zero modulus.
fn parse_rsa_jwk(
	jwk: &serde_json::Value,
	kid: Option<String>,
) -> Result<VerificationKey, VerificationError> {
	let n_b64 = jwk
		.get("n")
		.and_then(|value| value.as_str())
		.ok_or_else(|| VerificationError::InvalidJwk("missing 'n' field".into()))?;

	let e_b64 = jwk
		.get("e")
		.and_then(|value| value.as_str())
		.ok_or_else(|| VerificationError::InvalidJwk("missing 'e' field".into()))?;

	let n_bytes = URL_SAFE_NO_PAD
		.decode(n_b64)
		.map_err(|error| VerificationError::InvalidJwk(format!("bad base64url in 'n': {error}")))?;

	let e_bytes = URL_SAFE_NO_PAD
		.decode(e_b64)
		.map_err(|error| VerificationError::InvalidJwk(format!("bad base64url in 'e': {error}")))?;

	// Delegate to rs256_from_components so the 2048-bit minimum check
	// and key construction happen in a single place.
	let mut key = VerificationKey::rs256_from_components(&n_bytes, &e_bytes)?;
	key.kid = kid;
	Ok(key)
}

/// Parse a JWKS (JSON Web Key Set) document into a list of verification keys.
///
/// Expects the standard JWKS format: `{ "keys": [ { "kty": "EC", ... }, ... ] }`.
///
/// Keys that use algorithms or key types not supported by this module are
/// silently skipped rather than failing the entire parse—this matches the
/// tolerant behaviour expected by RFC 7517 §5, which allows a JWKS to contain
/// keys intended for multiple purposes or implementations. A JWKS published
/// by an agent operator might include `EdDSA` keys for other consumers alongside
/// the P-256 or RSA keys used for A2A card signing.
///
/// # Errors
///
/// Returns [`VerificationError::InvalidJwk`] if the top-level document has no
/// `"keys"` array, since that structural requirement cannot be tolerated.
/// Also propagates errors from malformed supported-algorithm keys within the set.
pub fn parse_jwks(jwks: &serde_json::Value) -> Result<Vec<VerificationKey>, VerificationError> {
	let keys_array = jwks
		.get("keys")
		.and_then(|value| value.as_array())
		.ok_or_else(|| {
			VerificationError::InvalidJwk("JWKS document missing 'keys' array".into())
		})?;

	let mut result = Vec::with_capacity(keys_array.len());

	for entry in keys_array {
		match VerificationKey::from_jwk(entry) {
			Ok(key) => result.push(key),
			// Skip unsupported keys—they may be for other clients.
			Err(VerificationError::UnsupportedAlgorithm(_)) => {}
			// Propagate all other errors: a malformed key in the set is worth
			// surfacing because it likely indicates operator error.
			Err(other) => return Err(other),
		}
	}

	Ok(result)
}
