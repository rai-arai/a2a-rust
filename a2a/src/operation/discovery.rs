// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Discovery operation types for the A2A protocol.
//!
//! The discovery domain has a single operation:
//! `GetExtendedAgentCard`. This returns an `AgentCard`
//! that may contain additional capabilities, skills, or security
//! information not present in the public agent card served at
//! /.well-known/agent.json.
//!
//! The public agent card is fetched via plain HTTP GET with no
//! authentication. The extended card requires authentication and
//! goes through the JSON-RPC layer, which is why it has its own
//! operation type here.
//!
//! The response type is simply AgentCard—the extended card uses
//! the same structure as the public card, just with more fields
//! populated. There's no separate `ExtendedAgentCard` type in the
//! spec.

use serde::{Deserialize, Serialize};

/// Parameters for the `GetExtendedAgentCard` operation.
///
/// Intentionally minimal—the operation just needs to know which
/// agent to query, and authentication is handled at the transport
/// layer (HTTP headers, etc.). The optional tenant field scopes the
/// request to a specific tenant in multi-tenant deployments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExtendedAgentCardParams {
	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,
}

impl GetExtendedAgentCardParams {
	/// Create empty params for an extended card request.
	///
	/// The extended card operation typically needs no parameters —
	/// authentication is handled at the transport layer (HTTP
	/// headers), not in the JSON-RPC params. Chain `with_tenant()`
	/// to scope the request to a specific tenant in multi-tenant
	/// deployments.
	///
	/// ```
	/// # use a2a::GetExtendedAgentCardParams;
	/// let params = GetExtendedAgentCardParams::new();
	/// assert!(params.tenant.is_none());
	/// ```
	#[must_use]
	pub fn new() -> Self {
		Self { tenant: None }
	}

	/// Scope this request to a specific tenant.
	///
	/// In multi-tenant deployments the agent may serve multiple
	/// isolated tenants. Providing a tenant identifier routes the
	/// extended card lookup to the correct tenant's configuration.
	/// When omitted the agent uses its default tenant resolution
	/// strategy.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

impl Default for GetExtendedAgentCardParams {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// The extended card request typically has no parameters at all —
	// authentication is in HTTP headers, not in the JSON-RPC params.
	// When tenant is None, the serialised form should be just an
	// empty object {}.
	#[test]
	fn empty_params_serialise_to_empty_object() {
		let params = GetExtendedAgentCardParams { tenant: None };

		let json = serde_json::to_value(&params).unwrap();
		let obj = json.as_object().unwrap();
		assert!(obj.is_empty(), "empty params should serialise as {{}}");
	}

	// When tenant is provided, it should appear in the serialised
	// output using the camelCase wire key. This is the only field,
	// so the output is just {"tenant": "..."}.
	#[test]
	fn params_with_tenant() {
		let params = GetExtendedAgentCardParams {
			tenant: Some("acme-corp".into()),
		};

		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["tenant"], "acme-corp");

		// The old metadata field must never appear—it was replaced
		// by tenant in the v1.0 GetExtendedAgentCardParams definition.
		assert!(
			json.as_object().unwrap().get("metadata").is_none(),
			"metadata field was removed in v1.0"
		);
	}

	// Round-trip to confirm nothing is lost. Even for a simple
	// struct, this guards against future field additions breaking
	// serialisation.
	#[test]
	fn params_round_trip() {
		let params = GetExtendedAgentCardParams {
			tenant: Some("tenant-xyz-789".into()),
		};

		let json = serde_json::to_string(&params).unwrap();
		let back: GetExtendedAgentCardParams = serde_json::from_str(&json).unwrap();
		assert_eq!(back, params);
	}

	// GetExtendedAgentCardParams::new() creates empty params with no
	// tenant. This is the typical construction—single-tenant
	// deployments never need to specify a tenant.
	#[test]
	fn new_creates_empty_params() {
		let params = GetExtendedAgentCardParams::new();
		assert!(params.tenant.is_none());
	}

	// with_tenant() scopes the extended card request to the named
	// tenant. This is the primary builder setter for multi-tenant
	// deployments where the caller must identify which tenant's
	// card they want.
	#[test]
	fn new_with_tenant() {
		let params = GetExtendedAgentCardParams::new().with_tenant("org-123");
		assert_eq!(params.tenant, Some("org-123".into()));
	}

	// The builder-constructed params must produce the same wire
	// format as an equivalent struct literal.
	#[test]
	fn builder_matches_struct_literal() {
		let from_builder = GetExtendedAgentCardParams::new();
		let from_literal = GetExtendedAgentCardParams { tenant: None };
		assert_eq!(
			serde_json::to_value(&from_builder).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// GetExtendedAgentCardParams::default() must produce the same result
	// as GetExtendedAgentCardParams::new(). This is a contract that the
	// Default trait impl delegates correctly.
	#[test]
	fn default_matches_new() {
		let from_new = GetExtendedAgentCardParams::new();
		let from_default = GetExtendedAgentCardParams::default();
		assert_eq!(from_new, from_default);
	}
}
