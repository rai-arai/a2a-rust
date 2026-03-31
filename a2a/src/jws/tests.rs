// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// JWS module tests.
//
// Tests are grouped by the concept they exercise: algorithm identifier
// parsing, error display, key construction from JWK, and end-to-end
// detached JWS verification. Each test comment explains what is being
// verified and why that property matters for the A2A protocol.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::agent_card::canonicalise_json;
use crate::agent_card::{
	AgentCapabilities, AgentCard, AgentCardRequired, AgentCardSignature, AgentInterface,
};
use crate::jws::key::VerificationKeyInner;
use crate::jws::{
	JwsAlgorithm, VerificationError, VerificationKey, parse_jwks, verify_detached_jws,
};

// Generate a fresh ES256 signing key and a matching VerificationKey.
//
// Tests that exercise signing and verification call this to avoid hardcoded
// key material. Every call produces a unique key pair via the OS CSPRNG so
// that tests cannot accidentally pass because they share key state across runs.
fn generate_es256_test_keys() -> (p256::ecdsa::SigningKey, VerificationKey) {
	use rand_core::OsRng;

	let signing_key = p256::ecdsa::SigningKey::random(&mut OsRng);
	let public_point = signing_key.verifying_key().to_encoded_point(false);
	let verifying_key = VerificationKey::es256_from_encoded_point(public_point.as_bytes())
		.expect("ES256 key from test-generated signing key must be valid");
	(signing_key, verifying_key)
}

// Generate a fresh RS256 signing key and a matching VerificationKey.
//
// RSA key generation is expensive relative to ECDSA—restrict RS256 helpers to
// tests that specifically exercise the RSA code path so the overall test suite
// remains fast. Tests that only need to check error paths (bad header, wrong
// algorithm, etc.) should use the ES256 helpers.
fn generate_rs256_test_keys() -> (rsa::pkcs1v15::SigningKey<sha2::Sha256>, VerificationKey) {
	use rand_core::OsRng;
	use rsa::traits::PublicKeyParts;

	let private_key = rsa::RsaPrivateKey::new(&mut OsRng, 2048)
		.expect("RSA key generation must succeed in tests");
	let public_key = private_key.to_public_key();
	let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(private_key);
	let verifying_key = VerificationKey::rs256_from_components(
		&public_key.n().to_bytes_be(),
		&public_key.e().to_bytes_be(),
	)
	.expect("RS256 key from test-generated key pair must be valid");
	(signing_key, verifying_key)
}

// Sign `payload` with an ES256 key and return `(protected_b64, signature_b64)`.
//
// Constructs the standard JWS protected header `{"alg":"ES256"}`, encodes it
// and the payload as Base64url, signs the resulting ASCII signing input, and
// returns both components ready to pass to `verify_detached_jws`. The payload
// is NOT embedded in the returned strings—callers must pass it separately,
// which is the detached-payload contract.
fn sign_detached_es256(signing_key: &p256::ecdsa::SigningKey, payload: &[u8]) -> (String, String) {
	use p256::ecdsa::signature::Signer;

	let header = r#"{"alg":"ES256"}"#;
	let protected_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());
	let payload_b64 = URL_SAFE_NO_PAD.encode(payload);
	let signing_input = format!("{protected_b64}.{payload_b64}");

	let signature: p256::ecdsa::Signature = signing_key.sign(signing_input.as_bytes());
	let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

	(protected_b64, signature_b64)
}

// Sign `payload` with an RS256 key and return `(protected_b64, signature_b64)`.
//
// Mirrors `sign_detached_es256` but uses RSASSA-PKCS1-v1_5 with SHA-256.
// The signing input construction is identical to the ES256 path—only the
// signing primitive changes—confirming that the algorithm dispatch inside
// `verify_detached_jws` is the only difference between the two paths.
fn sign_detached_rs256(
	signing_key: &rsa::pkcs1v15::SigningKey<sha2::Sha256>,
	payload: &[u8],
) -> (String, String) {
	use rsa::signature::SignatureEncoding;
	use rsa::signature::Signer;

	let header = r#"{"alg":"RS256"}"#;
	let protected_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());
	let payload_b64 = URL_SAFE_NO_PAD.encode(payload);
	let signing_input = format!("{protected_b64}.{payload_b64}");

	let signature: rsa::pkcs1v15::Signature = signing_key.sign(signing_input.as_bytes());
	let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

	(protected_b64, signature_b64)
}

// Verify that the canonical JWS algorithm identifiers as written in RFC 7518 §3
// parse correctly into their enum variants.
//
// The string form is the value that appears in the `alg` field of every JWS
// protected header. A wrong mapping here would cause every signature on a
// correctly-produced agent card to be rejected as unsupported.
#[test]
fn jws_algorithm_from_str_known_values() {
	assert_eq!(
		"ES256".parse::<JwsAlgorithm>().unwrap(),
		JwsAlgorithm::Es256
	);
	assert_eq!(
		"RS256".parse::<JwsAlgorithm>().unwrap(),
		JwsAlgorithm::Rs256
	);
}

// Verify that an unrecognised algorithm name produces `UnsupportedAlgorithm`
// rather than a panic or a silent fallback.
//
// This is the expected result when a card is signed with an algorithm the
// library does not yet implement (e.g. EdDSA). The operator should receive a
// clear error—not a confusing failure during cryptographic verification.
#[test]
fn jws_algorithm_from_str_unknown_returns_error() {
	let result = "EdDSA".parse::<JwsAlgorithm>();
	assert!(
		matches!(result, Err(VerificationError::UnsupportedAlgorithm(ref name)) if name == "EdDSA"),
		"unknown algorithm must return UnsupportedAlgorithm with the exact algorithm name"
	);
}

// Verify that `Display` for `JwsAlgorithm` produces the RFC 7518 string and
// that `from_str` and `Display` are mutual inverses.
//
// If `Display` and `from_str` diverge, any code that serialises an algorithm
// value and then re-parses it will silently produce the wrong algorithm.
// The round-trip property is required for any diagnostic or logging layer
// that captures and replays algorithm identifiers.
#[test]
fn jws_algorithm_display_round_trips() {
	assert_eq!(JwsAlgorithm::Es256.to_string(), "ES256");
	assert_eq!(JwsAlgorithm::Rs256.to_string(), "RS256");
	assert_eq!(
		JwsAlgorithm::Es256
			.to_string()
			.parse::<JwsAlgorithm>()
			.unwrap(),
		JwsAlgorithm::Es256
	);
	assert_eq!(
		JwsAlgorithm::Rs256
			.to_string()
			.parse::<JwsAlgorithm>()
			.unwrap(),
		JwsAlgorithm::Rs256
	);
}

// Verify that `Display` for every `VerificationError` variant produces a
// non-empty, human-readable string.
//
// Operators will see these strings in log output and error responses. An
// empty or near-empty message makes diagnosis impossible—each variant must
// contain enough context to understand the failure without looking at source.
#[test]
fn verification_error_display_is_non_empty() {
	let errors: Vec<VerificationError> = vec![
		VerificationError::InvalidProtectedHeader("bad header".into()),
		VerificationError::MissingAlgorithm,
		VerificationError::UnsupportedAlgorithm("EdDSA".into()),
		VerificationError::InvalidSignature("bad bytes".into()),
		VerificationError::SignatureInvalid,
		VerificationError::AlgorithmKeyMismatch {
			header_alg: JwsAlgorithm::Es256,
			key_alg: "RS256",
		},
		VerificationError::InvalidJwk("missing x".into()),
		VerificationError::NoSignatures,
		VerificationError::Serialization("internal error".into()),
	];

	for error in &errors {
		let message = error.to_string();
		assert!(
			!message.is_empty(),
			"VerificationError variant produced an empty Display string: {error:?}"
		);
	}
}

// Verify that a well-formed EC JWK with `crv: "P-256"` is parsed correctly
// and the resulting key can verify a genuine ES256 signature.
//
// This exercises the full JWK→key→verify pipeline. A mistake in coordinate
// byte ordering, Base64url padding handling, or EncodedPoint construction
// would fail either at parse time (InvalidJwk) or at verification time
// (SignatureInvalid)—both failures would immediately indicate the broken step.
#[test]
fn verification_key_from_jwk_ec_p256() {
	let (signing_key, _) = generate_es256_test_keys();
	let encoded_point = signing_key.verifying_key().to_encoded_point(false);
	let x_b64 = URL_SAFE_NO_PAD.encode(encoded_point.x().expect("x coordinate"));
	let y_b64 = URL_SAFE_NO_PAD.encode(encoded_point.y().expect("y coordinate"));

	let jwk = serde_json::json!({
		"kty": "EC",
		"crv": "P-256",
		"x": x_b64,
		"y": y_b64,
		"kid": "test-ec-key-1"
	});

	let key = VerificationKey::from_jwk(&jwk).expect("from_jwk must succeed for a valid P-256 JWK");

	assert_eq!(key.kid.as_deref(), Some("test-ec-key-1"));

	let payload = b"agent-card-json-here";
	let (protected_b64, signature_b64) = sign_detached_es256(&signing_key, payload);
	verify_detached_jws(&protected_b64, &signature_b64, payload, &key)
		.expect("signature from the matching key must verify");
}

// Verify that a well-formed RSA JWK is parsed correctly and the resulting key
// can verify a genuine RS256 signature.
//
// RSA key size is fixed at 2048 bits; this is the minimum widely acceptable
// size and keeps the test fast enough for CI. The test exercises the BigUint
// reconstruction path from raw `n` and `e` base64url values.
#[test]
fn verification_key_from_jwk_rsa() {
	use rsa::traits::PublicKeyParts;

	let private_key =
		rsa::RsaPrivateKey::new(&mut rand_core::OsRng, 2048).expect("RSA key generation");
	let public_key = private_key.to_public_key();
	let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(private_key);

	let n_b64 = URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be());
	let e_b64 = URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be());

	let jwk = serde_json::json!({
		"kty": "RSA",
		"n": n_b64,
		"e": e_b64,
		"kid": "test-rsa-key-1"
	});

	let key = VerificationKey::from_jwk(&jwk).expect("from_jwk must succeed for a valid RSA JWK");
	assert_eq!(key.kid.as_deref(), Some("test-rsa-key-1"));

	let payload = b"agent-card-json-content";
	let (protected_b64, signature_b64) = sign_detached_rs256(&signing_key, payload);
	verify_detached_jws(&protected_b64, &signature_b64, payload, &key)
		.expect("RS256 signature from the matching JWK key must verify");
}

// Verify that a JWK for an EC curve other than P-256 is rejected with
// `UnsupportedAlgorithm` rather than an opaque error or a panic.
//
// Operators who deploy agents with P-384 or P-521 keys should receive a clear
// message indicating that the curve is not supported, rather than a silent
// rejection or a confusing parse failure.
#[test]
fn verification_key_from_jwk_unsupported_curve() {
	let jwk = serde_json::json!({
		"kty": "EC",
		"crv": "P-384",
		"x": "placeholder",
		"y": "placeholder"
	});

	let result = VerificationKey::from_jwk(&jwk);
	assert!(
		matches!(result, Err(VerificationError::UnsupportedAlgorithm(_))),
		"P-384 JWK must return UnsupportedAlgorithm, got: {result:?}"
	);
}

// Verify that a JWK missing the mandatory `kty` field returns `InvalidJwk`.
//
// Every JWK must have a `kty` field per RFC 7517 §4.1. Without it there is no
// way to determine which algorithm family the key belongs to, so the error must
// be reported before any key material is inspected.
#[test]
fn verification_key_from_jwk_missing_kty() {
	let jwk = serde_json::json!({ "crv": "P-256", "x": "a", "y": "b" });
	let result = VerificationKey::from_jwk(&jwk);
	assert!(
		matches!(result, Err(VerificationError::InvalidJwk(_))),
		"JWK without kty must return InvalidJwk, got: {result:?}"
	);
}

// Verify that `es256_from_encoded_point` accepts a valid SEC1 uncompressed
// point and produces a key that verifies real signatures.
//
// SEC1 is the wire format for embedded EC key material in many protocols.
// Callers who receive a raw point (65 bytes: `0x04 || x || y`) can use this
// constructor directly instead of going through JWK.
#[test]
fn es256_from_encoded_point_round_trip() {
	let (signing_key, _) = generate_es256_test_keys();
	let encoded_point = signing_key.verifying_key().to_encoded_point(false);
	let point_bytes = encoded_point.as_bytes();

	let key = VerificationKey::es256_from_encoded_point(point_bytes)
		.expect("valid SEC1 point must construct successfully");

	let payload = b"verify me";
	let (protected_b64, signature_b64) = sign_detached_es256(&signing_key, payload);
	verify_detached_jws(&protected_b64, &signature_b64, payload, &key)
		.expect("key from SEC1 encoded point must verify the matching signature");
}

// Verify that `rs256_from_components` reconstructs a working key from raw
// modulus and exponent bytes and that signatures produced by the original key
// verify correctly.
//
// This constructor is the low-level equivalent of parsing `n` and `e` from an
// RSA JWK after Base64url-decoding. Testing it separately from `from_jwk`
// ensures the component-level construction is correct independent of the JWK
// parsing layer.
#[test]
fn rs256_from_components_round_trip() {
	use rsa::traits::PublicKeyParts;

	let private_key =
		rsa::RsaPrivateKey::new(&mut rand_core::OsRng, 2048).expect("RSA key generation");
	let public_key = private_key.to_public_key();
	let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(private_key);

	let n_bytes = public_key.n().to_bytes_be();
	let e_bytes = public_key.e().to_bytes_be();

	let key = VerificationKey::rs256_from_components(&n_bytes, &e_bytes)
		.expect("valid modulus and exponent must construct successfully");

	let payload = b"rs256 payload";
	let (protected_b64, signature_b64) = sign_detached_rs256(&signing_key, payload);
	verify_detached_jws(&protected_b64, &signature_b64, payload, &key)
		.expect("key from raw RSA components must verify the matching signature");
}

// Verify that a correctly constructed ES256 detached JWS passes verification
// and that the returned algorithm value matches what the header declared.
//
// This is the primary happy path. ECDSA over P-256 is the preferred algorithm
// for new A2A deployments due to smaller key and signature sizes compared to RSA.
#[test]
fn verify_detached_jws_es256_happy_path() {
	let (signing_key, verifying_key) = generate_es256_test_keys();
	let payload = b"{\"name\":\"Echo Agent\"}";
	let (protected_b64, signature_b64) = sign_detached_es256(&signing_key, payload);

	let algorithm = verify_detached_jws(&protected_b64, &signature_b64, payload, &verifying_key)
		.expect("valid ES256 JWS must verify successfully");

	assert_eq!(algorithm, JwsAlgorithm::Es256);
}

// Verify that a correctly constructed RS256 detached JWS passes verification
// and that the returned algorithm value is `Rs256`.
//
// RSA is still widely deployed and many operators generate RSA keys by default.
// The RS256 path must work end-to-end from header parsing through PKCS#1 v1.5
// signature verification.
#[test]
fn verify_detached_jws_rs256_happy_path() {
	let (signing_key, verifying_key) = generate_rs256_test_keys();
	let payload = b"{\"name\":\"Echo Agent\"}";
	let (protected_b64, signature_b64) = sign_detached_rs256(&signing_key, payload);

	let algorithm = verify_detached_jws(&protected_b64, &signature_b64, payload, &verifying_key)
		.expect("valid RS256 JWS must verify successfully");

	assert_eq!(algorithm, JwsAlgorithm::Rs256);
}

// Verify that modifying a single byte of the payload causes verification to
// fail with `SignatureInvalid`.
//
// This is the core security property of a signature scheme: any change to the
// signed content must invalidate the signature. The byte is modified in the
// middle of the payload (not at the start or end) to avoid hitting any edge
// cases in the Base64url encoding at block boundaries.
#[test]
fn verify_detached_jws_tampered_payload_fails() {
	let (signing_key, verifying_key) = generate_es256_test_keys();
	let payload = b"{\"name\":\"Legitimate Agent\"}";
	let (protected_b64, signature_b64) = sign_detached_es256(&signing_key, payload);

	let mut tampered = payload.to_vec();
	tampered[8] ^= 0xFF; // flip all bits in byte 8

	let result = verify_detached_jws(&protected_b64, &signature_b64, &tampered, &verifying_key);
	assert!(
		matches!(result, Err(VerificationError::SignatureInvalid)),
		"tampered payload must produce SignatureInvalid, got: {result:?}"
	);
}

// Verify that a signature produced by a different key of the same algorithm
// fails with `SignatureInvalid`.
//
// This simulates a replay attack where an adversary replaces the signature with
// one from a different (attacker-controlled) key. The verification key must be
// the operator's genuine public key for the check to mean anything.
#[test]
fn verify_detached_jws_wrong_key_fails() {
	let (signing_key_a, _) = generate_es256_test_keys();
	let (_, verifying_key_b) = generate_es256_test_keys();
	let payload = b"agent card content";
	let (protected_b64, signature_b64) = sign_detached_es256(&signing_key_a, payload);

	let result = verify_detached_jws(&protected_b64, &signature_b64, payload, &verifying_key_b);
	assert!(
		matches!(result, Err(VerificationError::SignatureInvalid)),
		"signature from a different key must fail, got: {result:?}"
	);
}

// Verify that a protected header that is not valid Base64url returns
// `InvalidProtectedHeader` rather than panicking.
//
// Malformed headers can arrive when a card is hand-edited, truncated in
// transit, or produced by a buggy third-party implementation. The error must
// be specific enough for the operator to diagnose the problem from logs alone.
#[test]
fn verify_detached_jws_invalid_base64_header() {
	let (_, verifying_key) = generate_es256_test_keys();
	let result = verify_detached_jws("!!!not-base64!!!", "sig", b"payload", &verifying_key);
	assert!(
		matches!(result, Err(VerificationError::InvalidProtectedHeader(_))),
		"invalid base64url header must return InvalidProtectedHeader, got: {result:?}"
	);
}

// Verify that a protected header that is valid Base64url but not valid JSON
// returns `InvalidProtectedHeader`.
//
// Distinct from the base64url test above: here the encoding is valid but the
// content is not JSON. Both paths are `InvalidProtectedHeader` but they
// exercise different branches of the decode→parse pipeline.
#[test]
fn verify_detached_jws_non_json_header() {
	let (_, verifying_key) = generate_es256_test_keys();
	let protected_b64 = URL_SAFE_NO_PAD.encode(b"this is not json");
	let result = verify_detached_jws(&protected_b64, "sig", b"payload", &verifying_key);
	assert!(
		matches!(result, Err(VerificationError::InvalidProtectedHeader(_))),
		"non-JSON header must return InvalidProtectedHeader, got: {result:?}"
	);
}

// Verify that a JWS header that is valid JSON but has no `alg` field returns
// `MissingAlgorithm`.
//
// Per RFC 7515 §4.1.1 the `alg` header parameter is mandatory. A header
// without it cannot be processed because there is no way to select the
// verification algorithm. The specific error lets the operator know exactly
// which field to add.
#[test]
fn verify_detached_jws_missing_alg_field() {
	let (_, verifying_key) = generate_es256_test_keys();
	let header_no_alg = r#"{"kid":"key-1","typ":"JWT"}"#;
	let protected_b64 = URL_SAFE_NO_PAD.encode(header_no_alg.as_bytes());

	let result = verify_detached_jws(&protected_b64, "sig", b"payload", &verifying_key);
	assert!(
		matches!(result, Err(VerificationError::MissingAlgorithm)),
		"header without alg must return MissingAlgorithm, got: {result:?}"
	);
}

// Verify that a valid `alg` value that is not supported by this module
// returns `UnsupportedAlgorithm`.
//
// EdDSA and ES384 are real algorithms a card could declare. The error must
// clearly tell the operator that the algorithm is recognised but not
// implemented, rather than claiming the signature is invalid.
#[test]
fn verify_detached_jws_unsupported_algorithm() {
	let (_, verifying_key) = generate_es256_test_keys();
	let header = r#"{"alg":"EdDSA"}"#;
	let protected_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());

	let result = verify_detached_jws(&protected_b64, "sig", b"payload", &verifying_key);
	assert!(
		matches!(result, Err(VerificationError::UnsupportedAlgorithm(_))),
		"unsupported alg must return UnsupportedAlgorithm, got: {result:?}"
	);
}

// Verify that an ES256 header paired with an RS256 key returns
// `AlgorithmKeyMismatch` with the correct `header_alg` and `key_alg` values.
//
// Mixing algorithm declarations and key types is an operator configuration
// mistake. Returning a specific mismatch error instead of letting it fall
// through to a generic cryptographic failure makes the root cause immediately
// clear in logs.
#[test]
fn verify_detached_jws_algorithm_key_mismatch_es256_with_rsa_key() {
	let (_, rs256_key) = generate_rs256_test_keys();
	let header = r#"{"alg":"ES256"}"#;
	let protected_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());

	let result = verify_detached_jws(&protected_b64, "sig", b"payload", &rs256_key);
	match result {
		Err(VerificationError::AlgorithmKeyMismatch {
			header_alg,
			key_alg,
		}) => {
			assert_eq!(header_alg, JwsAlgorithm::Es256);
			assert_eq!(key_alg, "RS256");
		}
		other => panic!("expected AlgorithmKeyMismatch, got: {other:?}"),
	}
}

// Verify that an RS256 header paired with an ES256 key returns
// `AlgorithmKeyMismatch` with the correct fields.
//
// Complements the previous test—both mismatch directions must be caught.
// An RS256 header with an EC key is equally invalid and equally confusing
// if it produced only a generic verification failure.
#[test]
fn verify_detached_jws_algorithm_key_mismatch_rs256_with_ec_key() {
	let (_, es256_key) = generate_es256_test_keys();
	let header = r#"{"alg":"RS256"}"#;
	let protected_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());

	let result = verify_detached_jws(&protected_b64, "sig", b"payload", &es256_key);
	match result {
		Err(VerificationError::AlgorithmKeyMismatch {
			header_alg,
			key_alg,
		}) => {
			assert_eq!(header_alg, JwsAlgorithm::Rs256);
			assert_eq!(key_alg, "ES256");
		}
		other => panic!("expected AlgorithmKeyMismatch, got: {other:?}"),
	}
}

// Verify that signature bytes that are valid Base64url but structurally invalid
// for P-256 compact encoding return `InvalidSignature`.
//
// A P-256 compact signature is exactly 64 bytes. An arbitrary byte string of
// the wrong length fails before reaching the cryptographic verification step
// and must produce `InvalidSignature` rather than `SignatureInvalid`, because
// the failure is a structural problem with the bytes—not a legitimate signature
// that failed to verify.
#[test]
fn verify_detached_jws_invalid_signature_bytes_es256() {
	let (signing_key, verifying_key) = generate_es256_test_keys();
	let payload = b"test payload";
	let (protected_b64, _) = sign_detached_es256(&signing_key, payload);

	let garbage_signature = URL_SAFE_NO_PAD.encode(b"this is not a valid ECDSA signature at all");
	let result = verify_detached_jws(&protected_b64, &garbage_signature, payload, &verifying_key);
	assert!(
		matches!(result, Err(VerificationError::InvalidSignature(_))),
		"structurally invalid ES256 signature bytes must return InvalidSignature, got: {result:?}"
	);
}

// Verify that a JWKS document with one P-256 key and one RSA key returns two
// VerificationKey values with the correct `kid` values.
//
// This is the happy path for JWKS parsing—the common case where the agent
// operator publishes a JWKS endpoint containing all their signing keys.
#[test]
fn parse_jwks_returns_supported_keys() {
	use rsa::traits::PublicKeyParts;

	let (signing_key_ec, _) = generate_es256_test_keys();
	let encoded_point = signing_key_ec.verifying_key().to_encoded_point(false);
	let x_b64 = URL_SAFE_NO_PAD.encode(encoded_point.x().expect("x coordinate"));
	let y_b64 = URL_SAFE_NO_PAD.encode(encoded_point.y().expect("y coordinate"));

	let private_rsa =
		rsa::RsaPrivateKey::new(&mut rand_core::OsRng, 2048).expect("RSA key generation");
	let public_rsa = private_rsa.to_public_key();
	let n_b64 = URL_SAFE_NO_PAD.encode(public_rsa.n().to_bytes_be());
	let e_b64 = URL_SAFE_NO_PAD.encode(public_rsa.e().to_bytes_be());

	let jwks = serde_json::json!({
		"keys": [
			{ "kty": "EC", "crv": "P-256", "x": x_b64, "y": y_b64, "kid": "ec-key-1" },
			{ "kty": "RSA", "n": n_b64, "e": e_b64, "kid": "rsa-key-1" }
		]
	});

	let keys = parse_jwks(&jwks).expect("valid JWKS must parse successfully");
	assert_eq!(keys.len(), 2);

	let kids: Vec<Option<&str>> = keys.iter().map(|key| key.kid.as_deref()).collect();
	assert!(kids.contains(&Some("ec-key-1")));
	assert!(kids.contains(&Some("rsa-key-1")));
}

// Verify that a JWKS document that mixes supported and unsupported key types
// returns only the supported keys, without failing.
//
// RFC 7517 §5 explicitly allows a JWKS to contain keys for multiple use cases
// and implementations. A JWKS that includes an OKP/EdDSA key alongside P-256
// keys must not cause the A2A verifier to reject the entire set—only the
// unsupported keys are silently skipped.
#[test]
fn parse_jwks_skips_unsupported_key_types() {
	let (signing_key_ec, _) = generate_es256_test_keys();
	let encoded_point = signing_key_ec.verifying_key().to_encoded_point(false);
	let x_b64 = URL_SAFE_NO_PAD.encode(encoded_point.x().expect("x coordinate"));
	let y_b64 = URL_SAFE_NO_PAD.encode(encoded_point.y().expect("y coordinate"));

	let jwks = serde_json::json!({
		"keys": [
			{ "kty": "EC", "crv": "P-256", "x": x_b64, "y": y_b64, "kid": "supported" },
			{ "kty": "OKP", "crv": "Ed25519", "x": "placeholder", "kid": "unsupported-okp" }
		]
	});

	let keys = parse_jwks(&jwks).expect("mixed JWKS must not fail");
	assert_eq!(keys.len(), 1, "only the P-256 key should be returned");
	assert_eq!(keys[0].kid.as_deref(), Some("supported"));
}

// Verify that a JWKS document with no `keys` array returns `InvalidJwk`.
//
// The `keys` array is the only mandatory field in a JWKS document
// (RFC 7517 §5). Without it the document is structurally invalid and no
// keys can be extracted at all.
#[test]
fn parse_jwks_missing_keys_array_returns_error() {
	let jwks = serde_json::json!({ "not_keys": [] });
	let result = parse_jwks(&jwks);
	assert!(
		matches!(result, Err(VerificationError::InvalidJwk(_))),
		"JWKS without 'keys' array must return InvalidJwk, got: {result:?}"
	);
}

// Verify that an empty JWKS `keys` array returns an empty Vec without error.
//
// An agent might publish a JWKS with no keys during a key rotation transition.
// Callers should handle this as "no keys available" rather than as a parse
// error—the document is structurally valid even if it has no usable content.
#[test]
fn parse_jwks_empty_keys_array_returns_empty_vec() {
	let jwks = serde_json::json!({ "keys": [] });
	let keys = parse_jwks(&jwks).expect("empty JWKS is structurally valid");
	assert!(keys.is_empty());
}

// Verify that `Debug` for `VerificationKey` does not print raw key bytes
// while still printing useful identifying information.
//
// Key material in debug output creates a risk of accidental exposure in log
// aggregation systems. The output must include the algorithm label and `kid`
// (if present) so that log lines remain diagnostic without compromising the key.
#[test]
fn verification_key_debug_does_not_print_key_material() {
	let (signing_key, key_no_kid) = generate_es256_test_keys();

	let debug_output = format!("{key_no_kid:?}");

	assert!(
		debug_output.contains("ES256"),
		"Debug output must include algorithm label; got: {debug_output}"
	);

	// A P-256 public key is 65 bytes in uncompressed form. We cannot check
	// for exact bytes since they are random, but a suspiciously long output
	// would suggest key bytes leaked into the string.
	assert!(
		debug_output.len() < 200,
		"Debug output suspiciously long—may contain key bytes: {debug_output}"
	);

	let key_with_kid = VerificationKey {
		kid: Some("signing-key-2025".to_string()),
		inner: VerificationKeyInner::Es256(*signing_key.verifying_key()),
	};
	let debug_with_kid = format!("{key_with_kid:?}");
	assert!(
		debug_with_kid.contains("signing-key-2025"),
		"Debug output must include kid when present; got: {debug_with_kid}"
	);
}

// Verify that constructing an RSA verification key from raw components that
// are shorter than 2048 bits (< 256 bytes) is rejected with `InvalidJwk`.
//
// Sub-2048-bit RSA keys are considered cryptographically weak—NIST and IETF
// guidance deprecates them entirely. Accepting a 512-bit key would allow an
// attacker to forge signatures with modest compute; the library must reject
// such keys at construction time so the verification path never reaches a
// situation where a weak key is trusted.
//
// The test constructs a minimal valid RSA key (512 bits) from known
// components to avoid depending on RSA key generation infrastructure. A
// 512-bit key produced by `RsaPrivateKey::new` is used and its public
// components are extracted; this ensures the key is internally consistent
// (valid n and e) so that the rejection is triggered by the size check,
// not a malformed key.
#[test]
fn rsa_key_below_2048_bits_is_rejected() {
	use rsa::traits::PublicKeyParts;

	// Generate a deliberately weak 512-bit RSA key. This is well below
	// the 2048-bit floor enforced by the library. The goal is a valid but
	// undersized key that trips the size guard, not a malformed key.
	let private_key =
		rsa::RsaPrivateKey::new(&mut rand_core::OsRng, 512).expect("512-bit RSA key generation");
	let public_key = private_key.to_public_key();
	let n_bytes = public_key.n().to_bytes_be();
	let e_bytes = public_key.e().to_bytes_be();

	let result = VerificationKey::rs256_from_components(&n_bytes, &e_bytes);
	assert!(
		matches!(result, Err(VerificationError::InvalidJwk(ref msg)) if msg.contains("2048")),
		"512-bit RSA key must be rejected with InvalidJwk mentioning 2048 bits; got: {result:?}"
	);
}

// Verify that a 512-bit RSA JWK is also rejected through the `from_jwk` path.
//
// The size check must apply regardless of how the key is constructed—both the
// `from_jwk` and `rs256_from_components` paths must enforce the floor. This
// test confirms the JWK parsing route rejects undersized keys before they
// reach the verification layer, so operator configuration mistakes are caught
// at key-load time rather than silently during verification.
#[test]
fn rsa_jwk_below_2048_bits_is_rejected() {
	use rsa::traits::PublicKeyParts;

	let private_key =
		rsa::RsaPrivateKey::new(&mut rand_core::OsRng, 512).expect("512-bit RSA key generation");
	let public_key = private_key.to_public_key();
	let n_b64 = URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be());
	let e_b64 = URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be());

	let jwk = serde_json::json!({
		"kty": "RSA",
		"n": n_b64,
		"e": e_b64,
		"kid": "weak-512-bit-key"
	});

	let result = VerificationKey::from_jwk(&jwk);
	assert!(
		matches!(result, Err(VerificationError::InvalidJwk(ref msg)) if msg.contains("2048")),
		"512-bit RSA JWK must be rejected with InvalidJwk mentioning 2048 bits; got: {result:?}"
	);
}

// Build a minimal AgentCard for integration tests.
//
// Provides the required fields only. Tests that need custom fields
// should clone and modify this base card rather than constructing
// from scratch.
fn minimal_test_card() -> AgentCard {
	AgentCard::new(AgentCardRequired {
		name: "Integration Test Agent".into(),
		description: "Minimal card for JWS integration tests".into(),
		supported_interfaces: vec![AgentInterface::new(
			"https://test.example.com/a2a",
			"JSONRPC",
			"1.0",
		)],
		version: "1.0.0".into(),
		capabilities: AgentCapabilities::default(),
		skills: vec![],
		default_input_modes: vec!["text/plain".into()],
		default_output_modes: vec!["text/plain".into()],
	})
}

// Sign an AgentCard with an ES256 key and return the resulting signature.
//
// Clears `signatures` on a clone of the card, serialises to the canonical
// form (keys sorted lexicographically at all nesting levels), and produces
// a detached JWS signature. This mirrors the signing procedure an agent
// operator would follow when publishing a signed card.
//
// The `kid` parameter, when Some, is embedded in the protected header so
// that `verify_signatures` can locate the matching key by ID rather than
// trying every key in the set.
fn sign_agent_card_es256(
	card: &AgentCard,
	signing_key: &p256::ecdsa::SigningKey,
	kid: Option<&str>,
) -> AgentCardSignature {
	use p256::ecdsa::signature::Signer;

	let mut canonical = card.clone();
	canonical.signatures = vec![];
	let mut canonical_value = serde_json::to_value(&canonical).unwrap();
	canonicalise_json(&mut canonical_value);
	let payload = serde_json::to_vec(&canonical_value).unwrap();

	let header_json = match kid {
		Some(k) => format!(r#"{{"alg":"ES256","kid":"{k}"}}"#),
		None => r#"{"alg":"ES256"}"#.to_string(),
	};
	let protected_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
	let payload_b64 = URL_SAFE_NO_PAD.encode(&payload);
	let signing_input = format!("{protected_b64}.{payload_b64}");

	let signature: p256::ecdsa::Signature = signing_key.sign(signing_input.as_bytes());
	let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

	AgentCardSignature::new(protected_b64, signature_b64)
}

// Verify that a correctly signed AgentCard passes `verify_signatures`.
//
// This is the primary integration happy path: an operator signs a card using
// the canonical form, attaches the signature, and a caller verifies it using
// the matching public key. Every component of the pipeline is exercised—card
// serialisation, canonicalisation, JWS construction, and signature
// verification. A failure here points to a broken link in the
// sign-then-verify chain.
#[test]
fn agent_card_verify_valid_es256_signature() {
	let (signing_key, verifying_key) = generate_es256_test_keys();
	let mut card = minimal_test_card();
	let signature = sign_agent_card_es256(&card, &signing_key, None);
	card.signatures = vec![signature];

	let result = card.verify_signatures(&[verifying_key]);
	assert!(
		result.is_ok(),
		"valid ES256 signature on a canonical AgentCard must verify successfully; got: {result:?}"
	);
}

// Verify that modifying the card's name after signing invalidates the
// signature and `verify_signatures` returns `SignatureInvalid`.
//
// This confirms that the signature commits to the card's content. An
// adversary who intercepts a card and alters even a single field must
// not be able to present it as verified—the canonical payload would
// differ from what was originally signed and the signature check must fail.
#[test]
fn agent_card_verify_tampered_name_fails() {
	let (signing_key, verifying_key) = generate_es256_test_keys();
	let mut card = minimal_test_card();
	let signature = sign_agent_card_es256(&card, &signing_key, None);
	card.signatures = vec![signature];

	// Tamper with the card after signing.
	card.name = "Malicious Replacement Agent".into();

	let result = card.verify_signatures(&[verifying_key]);
	assert!(
		matches!(result, Err(VerificationError::SignatureInvalid)),
		"tampered card name must cause SignatureInvalid; got: {result:?}"
	);
}

// Verify that calling `verify_signatures` on a card with no signatures
// returns `NoSignatures` rather than `Ok(())`.
//
// Silently succeeding on an unsigned card would be a security hole:
// any card fetched from an unauthenticated source would appear verified.
// The `NoSignatures` variant forces callers to make an explicit decision
// about whether they accept unsigned cards.
#[test]
fn agent_card_verify_no_signatures_returns_error() {
	let (_, verifying_key) = generate_es256_test_keys();
	let card = minimal_test_card();

	assert!(
		card.signatures.is_empty(),
		"minimal_test_card must have no signatures by default"
	);

	let result = card.verify_signatures(&[verifying_key]);
	assert!(
		matches!(result, Err(VerificationError::NoSignatures)),
		"card with no signatures must return NoSignatures; got: {result:?}"
	);
}

// Verify that `verify_signatures` matches a signature to the correct key
// by `kid` when multiple keys are present in the set.
//
// Key ID matching is an optimisation that also ensures correctness: when two
// keys share the same algorithm but sign different cards, the `kid` in the
// protected header must route each signature to its specific key. This test
// constructs two key pairs, signs with one, and confirms the signature is
// accepted only when the matching key (identified by `kid`) is in the set.
// The second key must not cause a false positive.
#[test]
fn agent_card_verify_with_kid_matching() {
	let (signing_key_a, _) = generate_es256_test_keys();
	let (_, verifying_key_b) = generate_es256_test_keys();

	// Build a verifying key for key A with a known kid.
	let verifying_key_a = VerificationKey {
		kid: Some("card-signing-key-a".to_string()),
		inner: VerificationKeyInner::Es256(*signing_key_a.verifying_key()),
	};

	// Sign the card with key A, embedding the kid in the protected header.
	let mut card = minimal_test_card();
	let signature = sign_agent_card_es256(&card, &signing_key_a, Some("card-signing-key-a"));
	card.signatures = vec![signature];

	// Supply both keys in the set—the kid hint must route to key A.
	let result = card.verify_signatures(&[verifying_key_a, verifying_key_b]);
	assert!(
		result.is_ok(),
		"kid-matched signature must verify when the correct key is in the set; got: {result:?}"
	);
}

// Calling verify_signatures with an empty key slice is a plausible
// misconfiguration — the caller forgot to load keys from their JWKS
// endpoint or key store. The fallback loop runs zero iterations and
// should return SignatureInvalid rather than panicking or succeeding.
// This test ensures the empty-key path is explicitly covered.
#[test]
fn agent_card_verify_with_empty_key_slice_returns_error() {
	let (signing_key, _verifying_key) = generate_es256_test_keys();
	let mut card = minimal_test_card();
	let signature = sign_agent_card_es256(&card, &signing_key, None);
	card.signatures = vec![signature];

	let result = card.verify_signatures(&[]);
	assert!(result.is_err(), "verify_signatures with no keys must fail");
	assert_eq!(
		result.unwrap_err(),
		VerificationError::SignatureInvalid,
		"empty key set should produce SignatureInvalid"
	);
}
