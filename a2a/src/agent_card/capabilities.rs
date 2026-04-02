// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};

use super::security::SecurityRequirement;

/// Protocol features the agent supports.
///
/// Callers check these before attempting streaming or push
/// notification operations. Attempting an unsupported operation
/// returns an `UnsupportedOperationError`.
///
/// All fields are optional—absent fields mean the capability
/// is not declared (treated as unsupported by callers).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
	/// Whether the agent supports SSE streaming (`SendStreamingMessage`,
	/// `SubscribeToTask`).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub streaming: Option<bool>,

	/// Whether the agent supports webhook push notifications.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub push_notifications: Option<bool>,

	/// Whether this agent provides an extended card with additional
	/// detail after authentication. When true, authenticated callers
	/// can fetch a richer card via `GetExtendedAgentCard`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub extended_agent_card: Option<bool>,

	/// Protocol extensions this agent's capabilities include.
	/// Empty by default; serialised only when non-empty.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub extensions: Vec<AgentExtension>,
}

/// A discrete action the agent can perform.
///
/// Skills help callers route requests to the right agent. Tags
/// provide searchable keywords, examples show usage patterns,
/// and input/output modes declare supported media types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
	/// Unique identifier for this skill within the agent.
	pub id: String,

	/// Human-readable name.
	pub name: String,

	/// Description of what this skill does.
	pub description: String,

	/// Searchable tags for skill discovery and routing.
	/// Callers use these to match requests to the right skill
	/// without parsing free-text descriptions.
	pub tags: Vec<String>,

	/// Example prompts or inputs that demonstrate how to use
	/// this skill. Helps callers understand the expected input
	/// format and phrasing.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub examples: Vec<String>,

	/// Media types this skill accepts as input.
	/// For example, `["text/plain", "application/json"]`.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub input_modes: Vec<String>,

	/// Media types this skill can produce as output.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub output_modes: Vec<String>,

	/// Security requirements specific to this skill.
	/// Overrides the card-level security requirements when present.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub security_requirements: Vec<SecurityRequirement>,
}

impl AgentSkill {
	/// Create a new skill with the required fields.
	///
	/// Every skill needs an ID (unique within the agent), a
	/// human-readable name, a description of what it does, and
	/// searchable tags for discovery. Collection fields (examples,
	/// input/output modes, `security_requirements`) default to empty
	/// vecs—use the `with_*` chainable setters to populate them.
	///
	/// ```
	/// # use a2a::AgentSkill;
	/// let skill = AgentSkill::new(
	///     "web-search",
	///     "Web Search",
	///     "Searches the web and returns summarised results",
	///     vec!["search".into(), "web".into()],
	/// )
	/// .with_examples(vec!["search for Rust async patterns".into()]);
	/// ```
	#[must_use]
	pub fn new(
		id: impl Into<String>,
		name: impl Into<String>,
		description: impl Into<String>,
		tags: Vec<String>,
	) -> Self {
		Self {
			id: id.into(),
			name: name.into(),
			description: description.into(),
			tags,
			examples: Vec::new(),
			input_modes: Vec::new(),
			output_modes: Vec::new(),
			security_requirements: Vec::new(),
		}
	}

	/// Add example prompts or inputs that demonstrate how to
	/// invoke this skill.
	///
	/// Helps callers and orchestrators understand the expected
	/// input format and phrasing. Good examples make it easier
	/// for LLM-based routers to match requests to the right skill.
	#[must_use]
	pub fn with_examples(mut self, examples: Vec<String>) -> Self {
		self.examples = examples;
		self
	}

	/// Declare which media types this skill accepts as input.
	///
	/// Overrides the card-level `default_input_modes` for this
	/// specific skill. When omitted, the skill inherits the
	/// card-level defaults.
	#[must_use]
	pub fn with_input_modes(mut self, input_modes: Vec<String>) -> Self {
		self.input_modes = input_modes;
		self
	}

	/// Declare which media types this skill can produce as output.
	///
	/// Overrides the card-level `default_output_modes` for this
	/// specific skill. When omitted, the skill inherits the
	/// card-level defaults.
	#[must_use]
	pub fn with_output_modes(mut self, output_modes: Vec<String>) -> Self {
		self.output_modes = output_modes;
		self
	}

	/// Set security requirements specific to this skill.
	///
	/// Each `SecurityRequirement` lists the scheme names and scope
	/// lists that must be satisfied. Overrides the card-level
	/// `security_requirements` when present. Use this when a skill
	/// requires elevated permissions beyond the agent's baseline.
	#[must_use]
	pub fn with_security_requirements(
		mut self,
		security_requirements: Vec<SecurityRequirement>,
	) -> Self {
		self.security_requirements = security_requirements;
		self
	}
}

impl AgentCapabilities {
	/// Declare whether the agent supports SSE streaming.
	///
	/// When true, callers can use `SendStreamingMessage` and `SubscribeToTask`.
	/// When false or absent, those operations return `UnsupportedOperation`.
	#[must_use]
	pub fn with_streaming(mut self, streaming: bool) -> Self {
		self.streaming = Some(streaming);
		self
	}

	/// Declare whether the agent supports webhook push notifications.
	///
	/// When true, callers can register push notification configs.
	/// When false or absent, push notification operations return
	/// `PushNotificationNotSupported`.
	#[must_use]
	pub fn with_push_notifications(mut self, push_notifications: bool) -> Self {
		self.push_notifications = Some(push_notifications);
		self
	}

	/// Declare whether this agent provides an authenticated extended card.
	///
	/// When true, authenticated callers can fetch a richer agent card
	/// via the `GetExtendedAgentCard` operation that may include additional
	/// skills or details not visible in the public discovery card.
	#[must_use]
	pub fn with_extended_agent_card(mut self, extended_agent_card: bool) -> Self {
		self.extended_agent_card = Some(extended_agent_card);
		self
	}

	/// Attach protocol extensions to the capabilities.
	///
	/// Extensions let agents advertise non-standard features beyond
	/// the core A2A protocol. Callers can ignore unknown extensions
	/// unless marked as required.
	#[must_use]
	pub fn with_extensions(mut self, extensions: Vec<AgentExtension>) -> Self {
		self.extensions = extensions;
		self
	}
}

/// A protocol binding the agent supports.
///
/// Each interface specifies the protocol binding and endpoint URL.
/// The `protocol_binding` field identifies the A2A binding specification
/// (e.g. "JSONRPC", "GRPC") and the url is the endpoint address.
///
/// Optional fields allow the operator to scope an interface to a
/// specific tenant or to pin the protocol version for compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterface {
	/// The endpoint URL for this interface.
	pub url: String,

	/// The protocol binding identifier (e.g. "JSONRPC", "GRPC").
	pub protocol_binding: String,

	/// Optional tenant scope for multi-tenant deployments.
	/// When present, the interface is scoped to this tenant only.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,

	/// The protocol version for this interface.
	/// Callers use this to detect version compatibility before
	/// sending requests.
	pub protocol_version: String,
}

/// A protocol extension the agent supports or requires.
///
/// Extensions let agents advertise non-standard capabilities.
/// Callers can ignore unknown extensions unless required is true,
/// in which case the caller must understand the extension to
/// interact with the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExtension {
	/// Globally unique URI identifying the extension spec.
	pub uri: String,

	/// Human-readable description of what this extension provides.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub description: Option<String>,

	/// Whether callers must support this extension.
	/// If true, callers that don't understand the extension
	/// should not interact with the agent.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub required: Option<bool>,

	/// Extension-specific parameters or configuration.
	/// Constrained to a JSON object per proto3 `google.protobuf.Struct`
	/// semantics—arrays and scalars are not valid parameter values.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub params: Option<serde_json::Map<String, serde_json::Value>>,
}

impl AgentInterface {
	/// Create a new protocol interface binding.
	///
	/// All three fields are required—an interface without a URL,
	/// protocol binding, or protocol version is meaningless for
	/// discovery. The `protocol_version` lets callers detect version
	/// compatibility before sending requests.
	///
	/// The optional `tenant` field defaults to None. Use
	/// `with_tenant()` to scope the interface to a specific tenant
	/// in multi-tenant deployments.
	///
	/// ```
	/// # use a2a::AgentInterface;
	/// let iface = AgentInterface::new("https://example.com/a2a", "JSONRPC", "1.0");
	/// assert_eq!(iface.protocol_binding, "JSONRPC");
	/// assert_eq!(iface.protocol_version, "1.0");
	/// ```
	#[must_use]
	pub fn new(
		url: impl Into<String>,
		protocol_binding: impl Into<String>,
		protocol_version: impl Into<String>,
	) -> Self {
		Self {
			url: url.into(),
			protocol_binding: protocol_binding.into(),
			tenant: None,
			protocol_version: protocol_version.into(),
		}
	}

	/// Scope this interface to a specific tenant.
	///
	/// Used in multi-tenant deployments where a single endpoint
	/// serves multiple organisations. Callers that match this
	/// tenant use this interface.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

impl AgentExtension {
	/// Create a new protocol extension declaration.
	///
	/// Takes the globally unique URI identifying the extension spec.
	/// Optional fields (description, required, params) default to
	/// None; chain `with_description()`, `with_required()`, and
	/// `with_params()` to populate them.
	///
	/// ```
	/// # use a2a::AgentExtension;
	/// let ext = AgentExtension::new("urn:example:custom-extension")
	///     .with_required(true);
	/// ```
	#[must_use]
	pub fn new(uri: impl Into<String>) -> Self {
		Self {
			uri: uri.into(),
			description: None,
			required: None,
			params: None,
		}
	}

	/// Attach a human-readable description of this extension.
	#[must_use]
	pub fn with_description(mut self, description: impl Into<String>) -> Self {
		self.description = Some(description.into());
		self
	}

	/// Mark whether callers must support this extension to interact
	/// with the agent.
	#[must_use]
	pub fn with_required(mut self, required: bool) -> Self {
		self.required = Some(required);
		self
	}

	/// Attach extension-specific parameters or configuration.
	#[must_use]
	pub fn with_params(mut self, params: serde_json::Map<String, serde_json::Value>) -> Self {
		self.params = Some(params);
		self
	}
}
