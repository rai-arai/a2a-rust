// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-request context and transport-level metadata types.
//!
//! This module provides the types that flow from the transport layer
//! (HTTP headers, connection metadata) down into handler methods.
//! Handlers that need authentication, tenant scoping, or distributed
//! tracing read values from the Extensions map. Handlers that don't
//! care about transport metadata simply ignore the context parameter.
//!
//! The design is deliberately transport-agnostic: nothing here depends
//! on axum, actix, or any HTTP crate. This means handlers can be
//! written and unit-tested without pulling in any transport dependency.
//! Transport adapters (`axum_integration`, etc.) construct a
//! `RequestContext` from the raw HTTP request before calling dispatch.

use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Type-erased extension map for per-request metadata.
///
/// Stores values indexed by their Rust type, allowing transport
/// layers to insert context (auth tokens, correlation IDs, tenant
/// info) and handlers to retrieve it without the server trait
/// knowing about HTTP or any specific transport.
///
/// This is deliberately minimal—no dependency on the `http` crate.
/// Modelled after `http::Extensions` but self-contained for Wasm
/// compatibility and transport independence.
///
/// Values are keyed by `TypeId`, so each type may appear at most
/// once. Inserting a value for a type that already exists overwrites
/// it and returns the previous value.
#[derive(Default)]
pub struct Extensions {
	map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Extensions {
	/// Create an empty extension map.
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}

	/// Insert a value into the map.
	///
	/// Returns the previously inserted value of type `T`, if any.
	/// The value is stored under `TypeId::of::<T>()`, so each Rust
	/// type maps to at most one slot.
	pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
		self.map
			.insert(TypeId::of::<T>(), Box::new(value))
			.and_then(|prev| prev.downcast().ok().map(|boxed| *boxed))
	}

	/// Retrieve a shared reference to the value of type `T`.
	///
	/// Returns `None` if no value of that type has been inserted.
	#[must_use]
	pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
		self.map
			.get(&TypeId::of::<T>())
			.and_then(|boxed| boxed.downcast_ref())
	}

	/// Return `true` if a value of type `T` is present in the map.
	#[must_use]
	pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
		self.map.contains_key(&TypeId::of::<T>())
	}
}

impl std::fmt::Debug for Extensions {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("Extensions")
			.field("count", &self.map.len())
			.finish()
	}
}

/// Per-request context carrying transport-level metadata alongside
/// the JSON-RPC parameters.
///
/// Transport layers (axum, actix, custom HTTP servers) construct a
/// `RequestContext` from the incoming HTTP request and pass it through
/// the dispatch layer into handler methods. Handlers that need
/// authentication, tenant scoping, or request tracing read from the
/// extensions map. Handlers that don't need context simply ignore it.
///
/// The type is deliberately transport-agnostic—it lives in the
/// `server` feature, not the `axum` feature, so handlers can be
/// written and tested without any transport dependency.
#[derive(Debug, Default)]
pub struct RequestContext {
	extensions: Extensions,
}

impl RequestContext {
	/// Create an empty `RequestContext`.
	#[must_use]
	pub fn new() -> Self {
		Self::default()
	}

	/// Return a shared reference to the extension map.
	#[must_use]
	pub fn extensions(&self) -> &Extensions {
		&self.extensions
	}

	/// Return an exclusive reference to the extension map for insertion.
	pub fn extensions_mut(&mut self) -> &mut Extensions {
		&mut self.extensions
	}
}

/// Bearer token extracted from the HTTP `Authorization` header.
///
/// The token is stored as a raw string—validation (JWT decoding,
/// token introspection) is the handler's responsibility. The
/// `build_request_context` function in the axum integration strips
/// the `Bearer ` prefix before inserting this value.
///
/// The `Debug` impl redacts the token value so that formatting with
/// `{:?}` in logs, panic messages, or test output never exposes the
/// raw credential. Use `.0` to access the value directly when needed.
#[derive(Clone)]
pub struct BearerToken(pub String);

impl std::fmt::Debug for BearerToken {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_tuple("BearerToken")
			.field(&"[REDACTED]")
			.finish()
	}
}

/// API key extracted from the `X-API-Key` header or similar.
///
/// Handlers that authenticate via API keys retrieve this value from
/// the context rather than parsing HTTP headers themselves, keeping
/// the handler transport-agnostic.
///
/// The `Debug` impl redacts the key value to prevent credential
/// leakage through logging or diagnostic output.
#[derive(Clone)]
pub struct ApiKey(pub String);

impl std::fmt::Debug for ApiKey {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_tuple("ApiKey")
			.field(&"[REDACTED]")
			.finish()
	}
}

/// Opaque correlation identifier for request tracing.
///
/// Typically sourced from the `X-Request-Id` or `X-Correlation-Id`
/// headers. Handlers and middleware can attach this to log records
/// and outgoing requests to link all operations belonging to a single
/// logical trace. Not redacted in Debug output because trace IDs are
/// not credentials and are expected to appear in logs.
#[derive(Debug, Clone)]
pub struct CorrelationId(pub String);

#[cfg(test)]
mod tests {
	use super::*;

	// Extensions::insert returns None when the type is new to the map,
	// and returns the previous value when overwriting an existing entry.
	// This mirrors the contract of HashMap::insert and is important for
	// callers that conditionally update context values.
	#[test]
	fn insert_returns_previous_value() {
		let mut ext = Extensions::new();

		// First insertion—no prior value.
		let prev = ext.insert(BearerToken("token-v1".into()));
		assert!(
			prev.is_none(),
			"first insert must return None because no prior value exists"
		);

		// Second insertion—previous value must be returned.
		let prev = ext.insert(BearerToken("token-v2".into()));
		assert!(
			prev.is_some(),
			"second insert must return the displaced value"
		);
		assert_eq!(
			prev.unwrap().0,
			"token-v1",
			"the displaced value must be the first token, not the replacement"
		);
	}

	// Extensions::get returns a shared reference to the stored value.
	// The reference must reflect the most recently inserted value and
	// must return None for a type that has not been inserted.
	#[test]
	fn get_retrieves_inserted_value() {
		let mut ext = Extensions::new();
		ext.insert(ApiKey("key-abc".into()));

		let key = ext.get::<ApiKey>();
		assert!(key.is_some(), "ApiKey must be retrievable after insertion");
		assert_eq!(
			key.unwrap().0,
			"key-abc",
			"retrieved ApiKey must match the inserted value"
		);

		// A type that was never inserted must yield None.
		let missing = ext.get::<BearerToken>();
		assert!(
			missing.is_none(),
			"BearerToken was not inserted so get must return None"
		);
	}

	// Extensions::contains reports presence without moving or cloning
	// the stored value. It must return true only for types that have
	// been explicitly inserted.
	#[test]
	fn contains_reflects_insertion_state() {
		let mut ext = Extensions::new();

		assert!(
			!ext.contains::<CorrelationId>(),
			"empty map must not contain CorrelationId"
		);

		ext.insert(CorrelationId("trace-001".into()));

		assert!(
			ext.contains::<CorrelationId>(),
			"map must contain CorrelationId after insertion"
		);
		assert!(
			!ext.contains::<BearerToken>(),
			"BearerToken was not inserted; contains must return false"
		);
	}

	// Multiple distinct types can coexist in the same Extensions map
	// because each type occupies a different TypeId slot. Inserting one
	// type must not displace or interfere with another.
	#[test]
	fn multiple_types_coexist_independently() {
		let mut ext = Extensions::new();
		ext.insert(BearerToken("jwt.abc".into()));
		ext.insert(ApiKey("api-xyz".into()));
		ext.insert(CorrelationId("trace-99".into()));

		assert_eq!(
			ext.get::<BearerToken>().map(|t| t.0.as_str()),
			Some("jwt.abc"),
			"BearerToken must be independently retrievable"
		);
		assert_eq!(
			ext.get::<ApiKey>().map(|k| k.0.as_str()),
			Some("api-xyz"),
			"ApiKey must be independently retrievable"
		);
		assert_eq!(
			ext.get::<CorrelationId>().map(|c| c.0.as_str()),
			Some("trace-99"),
			"CorrelationId must be independently retrievable"
		);
	}

	// RequestContext::new creates an empty context. Both extensions()
	// and extensions_mut() must return valid references to the same map.
	// Inserting via extensions_mut and reading via extensions must be
	// consistent—they are the same backing storage.
	#[test]
	fn request_context_extensions_accessors_share_storage() {
		let mut ctx = RequestContext::new();

		// Map must start empty.
		assert!(
			!ctx.extensions().contains::<BearerToken>(),
			"fresh RequestContext must have an empty extension map"
		);

		// Insert through the mutable accessor.
		ctx.extensions_mut().insert(BearerToken("ctx-token".into()));

		// Read back through the shared accessor—same map, same value.
		let token = ctx.extensions().get::<BearerToken>();
		assert!(
			token.is_some(),
			"BearerToken inserted via extensions_mut must be visible through extensions"
		);
		assert_eq!(
			token.unwrap().0,
			"ctx-token",
			"the retrieved token must match what was inserted"
		);
	}

	// Extensions::debug output must not expose the raw values (which
	// may contain sensitive tokens) but must reflect the count so that
	// log messages are informative without leaking credentials.
	#[test]
	fn extensions_debug_shows_count_not_values() {
		let mut ext = Extensions::new();
		ext.insert(BearerToken("secret-jwt".into()));
		ext.insert(ApiKey("secret-key".into()));

		let debug_output = format!("{ext:?}");

		// The count field must be present and correct.
		assert!(
			debug_output.contains("count: 2"),
			"debug output must include the entry count (got: {debug_output})"
		);

		// The raw token values must NOT appear in debug output, because
		// debug is often forwarded to structured logs and log aggregators.
		assert!(
			!debug_output.contains("secret-jwt"),
			"debug output must not expose BearerToken value"
		);
		assert!(
			!debug_output.contains("secret-key"),
			"debug output must not expose ApiKey value"
		);
	}
}
