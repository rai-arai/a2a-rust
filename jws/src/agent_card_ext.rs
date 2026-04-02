// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Extension trait that adds JWS signature verification to `AgentCard`.
//!
//! Import `AgentCardVerify` to call `card.verify_signatures(keys)` on
//! any `AgentCard`. This keeps the core `a2a` crate free of crypto
//! dependencies while making verification ergonomic for callers that
//! depend on `a2a-jws`.

use a2a::agent_card::{AgentCard, AgentCardSignature};

use crate::error::VerificationError;
use crate::key::VerificationKey;
use crate::verify::verify_detached_jws;

/// Extension trait that adds JWS signature verification to `AgentCard`.
///
/// ```ignore
/// use a2a_jws::AgentCardVerify;
///
/// let result = card.verify_signatures(&keys);
/// ```
pub trait AgentCardVerify {
	/// Verifies all signatures on this agent card against the provided keys.
	///
	/// For each signature the method extracts the key ID (`kid`) from the
	/// protected or unprotected header, finds a matching key by `kid`, and
	/// verifies the signature over the card's canonical payload.
	///
	/// The canonical payload is this card serialised to JSON with the
	/// `signatures` field set to an empty vec—reproducing the content that
	/// was originally signed. Canonical form is defined as: all JSON object
	/// keys sorted lexicographically at every nesting level, with the
	/// `signatures` field omitted.
	///
	/// # Errors
	///
	/// Returns `VerificationError::NoSignatures` if the card has no
	/// signatures. Returns other `VerificationError` variants if
	/// serialisation fails, a key ID has no match, or a signature does
	/// not verify against any of the provided keys.
	fn verify_signatures(&self, keys: &[VerificationKey]) -> Result<(), VerificationError>;
}

impl AgentCardVerify for AgentCard {
	fn verify_signatures(&self, keys: &[VerificationKey]) -> Result<(), VerificationError> {
		if self.signatures.is_empty() {
			return Err(VerificationError::NoSignatures);
		}

		let mut canonical = self.clone();
		canonical.signatures = vec![];
		let mut canonical_value = serde_json::to_value(&canonical).map_err(|error| {
			VerificationError::Serialization(format!("failed to serialise canonical card: {error}"))
		})?;
		canonicalise_json(&mut canonical_value);
		let payload = serde_json::to_vec(&canonical_value).map_err(|error| {
			VerificationError::Serialization(format!(
				"failed to convert canonical value to bytes: {error}"
			))
		})?;

		for signature in &self.signatures {
			let kid = extract_kid_from_signature(signature);

			let matched_key = kid.as_deref().and_then(|kid_value| {
				keys.iter()
					.find(|key| key.kid.as_deref() == Some(kid_value))
			});

			if let Some(key) = matched_key {
				verify_detached_jws(&signature.protected, &signature.signature, &payload, key)?;
			} else {
				verify_with_any_key(signature, &payload, keys)?;
			}
		}

		Ok(())
	}
}

// Recursively sort all JSON object keys lexicographically in place.
//
// Produces a canonical form whose byte representation is stable regardless
// of the originating language or library's struct field ordering.
pub(crate) fn canonicalise_json(value: &mut serde_json::Value) {
	match value {
		serde_json::Value::Object(map) => {
			for value in map.values_mut() {
				canonicalise_json(value);
			}
			let sorted: std::collections::BTreeMap<String, serde_json::Value> =
				std::mem::take(map).into_iter().collect();
			*map = sorted.into_iter().collect();
		}
		serde_json::Value::Array(arr) => {
			for value in arr {
				canonicalise_json(value);
			}
		}
		_ => {}
	}
}

fn extract_kid_from_signature(signature: &AgentCardSignature) -> Option<String> {
	use base64::Engine;

	if let Some(header) = &signature.header
		&& let Some(kid) = header.get("kid").and_then(|value| value.as_str())
	{
		return Some(kid.to_string());
	}

	let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
		.decode(&signature.protected)
		.ok()?;
	let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
	json.get("kid")
		.and_then(|value| value.as_str())
		.map(String::from)
}

fn verify_with_any_key(
	signature: &AgentCardSignature,
	payload: &[u8],
	keys: &[VerificationKey],
) -> Result<(), VerificationError> {
	let mut compatible_error: Option<VerificationError> = None;
	let mut mismatch_error: Option<VerificationError> = None;

	for key in keys {
		match verify_detached_jws(&signature.protected, &signature.signature, payload, key) {
			Ok(_algorithm) => return Ok(()),
			Err(error @ VerificationError::AlgorithmKeyMismatch { .. }) => {
				mismatch_error.get_or_insert(error);
			}
			Err(error) => {
				compatible_error = Some(error);
			}
		}
	}

	Err(compatible_error
		.or(mismatch_error)
		.unwrap_or(VerificationError::SignatureInvalid))
}
