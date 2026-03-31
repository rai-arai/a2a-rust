// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conversation roles in the A2A protocol.
//!
//! Every message in the protocol is attributed to either a "user" (the
//! calling agent or human operator) or an "agent" (the receiving agent).
//! This is a deliberate simplification—unlike chat protocols with
//! system roles or multi-party conversations, A2A models every
//! interaction as a two-party exchange between a requester and a
//! responder.
//!
//! The role determines how the message is interpreted during task
//! execution. Requester messages are inputs (requests, follow-ups,
//! additional context). Agent messages are outputs (responses, status
//! updates, clarification requests). This asymmetry matters for
//! streaming—only agent messages generate SSE events.

use serde::{Deserialize, Serialize};

/// Which party sent a message—the requester or the responder.
///
/// Serialises to lowercase strings ("user", "agent") per the
/// JSON-RPC wire format. The proto definition uses an enum with
/// `ROLE_UNSPECIFIED`, `ROLE_USER`, `ROLE_AGENT` but the JSON binding
/// drops the prefix and uses lowercase.
///
/// Unlike [`TaskState`](crate::TaskState), this enum intentionally
/// has no `Unknown(String)` fallback. Unrecognised role strings
/// cause a deserialisation error. The A2A protocol's two-party
/// model (requester vs responder) is foundational—a message from
/// an unrecognised party cannot be routed or interpreted, so hard
/// rejection is the correct behaviour. The `#[non_exhaustive]`
/// attribute is present solely to allow adding variants in future
/// major versions without a semver break for downstream match arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Role {
	/// The calling agent or human—the party that initiated the
	/// conversation or is providing additional input.
	User,

	/// The receiving agent—the party doing the work and
	/// producing responses, artifacts, and status updates.
	Agent,
}

impl std::fmt::Display for Role {
	/// Formats the role as its wire-format string ("user" or "agent").
	///
	/// Matches the lowercase strings used on the JSON-RPC wire so
	/// that Display output in logs and diagnostics is consistent
	/// with what appears in protocol messages.
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let label = match self {
			Role::User => "user",
			Role::Agent => "agent",
		};
		formatter.write_str(label)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// The spec requires lowercase "user" and "agent" on the wire.
	// Both directions (serialise and deserialise) need to agree
	// because clients and servers both produce and consume these
	// strings. A mismatch means one side rejects the other's messages.
	#[test]
	fn serialises_to_lowercase_wire_values() {
		assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
		assert_eq!(serde_json::to_string(&Role::Agent).unwrap(), "\"agent\"");
	}

	// Round-trip to confirm deserialisation agrees with serialisation.
	// This catches cases where serde's rename_all generates different
	// strings for serialise vs deserialise (which shouldn't happen with
	// rename_all, but it's worth confirming for spec compliance).
	#[test]
	fn round_trips_both_roles() {
		for role in [Role::User, Role::Agent] {
			let json = serde_json::to_string(&role).unwrap();
			let back: Role = serde_json::from_str(&json).unwrap();
			assert_eq!(back, role);
		}
	}

	// Reject strings that aren't in the spec. Agents and clients should
	// fail fast on unknown roles rather than guessing the sender's intent.
	#[test]
	fn rejects_unknown_role() {
		assert!(serde_json::from_str::<Role>("\"system\"").is_err());
		assert!(serde_json::from_str::<Role>("\"assistant\"").is_err());
	}

	// Display must produce the exact wire-format strings ("user",
	// "agent") so that log output is consistent with protocol
	// messages. The Display impl uses a manual match that must
	// agree with serde's rename_all = "lowercase".
	#[test]
	fn display_matches_wire_format_strings() {
		assert_eq!(Role::User.to_string(), "user");
		assert_eq!(Role::Agent.to_string(), "agent");
	}

	// Cross-check Display output against serde serialisation to
	// catch divergence between the two separate implementations.
	#[test]
	fn display_agrees_with_serde_serialisation() {
		for role in [Role::User, Role::Agent] {
			let display_string = role.to_string();
			let serde_string = serde_json::to_string(&role).unwrap();
			let serde_unquoted = serde_string.trim_matches('"');
			assert_eq!(
				display_string, serde_unquoted,
				"Display and serde disagree for {:?}",
				role
			);
		}
	}
}
