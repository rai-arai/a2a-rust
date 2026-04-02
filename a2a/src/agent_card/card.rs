// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::capabilities::{AgentCapabilities, AgentInterface, AgentSkill};
use super::security::{SecurityRequirement, SecurityScheme};

/// The full agent discovery card.
///
/// Served at /.well-known/agent.json on the agent's host. Callers
/// fetch this to learn what the agent offers and how to connect.
///
/// Required fields: name, description, `supported_interfaces`, version,
/// capabilities, skills, `default_input_modes`, `default_output_modes`.
///
/// The first entry in `supported_interfaces` is the preferred interface.
/// All additional entries are alternatives the caller may choose from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
	/// Human-readable name for this agent.
	pub name: String,

	/// Description of what this agent does.
	/// Required—helps callers decide whether to interact with
	/// this agent before sending messages.
	pub description: String,

	/// All protocol interfaces this agent supports.
	///
	/// The first entry is the preferred interface. Callers should
	/// attempt the first entry before falling back to later entries.
	/// Replaces the old `url`, `protocol_version`, `preferred_transport`,
	/// and `additional_interfaces` fields from the pre-v1.0 spec.
	pub supported_interfaces: Vec<AgentInterface>,

	/// Card schema version. Tracks changes to the card's structure,
	/// not the agent's implementation.
	pub version: String,

	/// Protocol features this agent supports.
	pub capabilities: AgentCapabilities,

	/// The actions this agent can perform.
	/// Each skill describes a discrete capability with tags,
	/// examples, and supported media types.
	pub skills: Vec<AgentSkill>,

	/// Default media types this agent accepts as input.
	/// Applied to all skills unless overridden at the skill level.
	pub default_input_modes: Vec<String>,

	/// Default media types this agent can produce as output.
	/// Applied to all skills unless overridden at the skill level.
	pub default_output_modes: Vec<String>,

	/// The organisation or person that operates this agent.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub provider: Option<AgentProvider>,

	/// Authentication methods this agent accepts.
	/// Keys are scheme names referenced by the `security_requirements` field.
	/// Values define the scheme parameters (API key, `OAuth2`, etc.).
	#[serde(default, skip_serializing_if = "HashMap::is_empty")]
	pub security_schemes: HashMap<String, SecurityScheme>,

	/// Security requirements that apply to all operations.
	/// Each entry lists the schemes that must be satisfied.
	/// References entries in `security_schemes`.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub security_requirements: Vec<SecurityRequirement>,

	/// URL to the agent's documentation.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub documentation_url: Option<String>,

	/// URL to the agent's icon image.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub icon_url: Option<String>,

	/// JWS signatures for card integrity verification.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub signatures: Vec<AgentCardSignature>,
}

/// A JWS signature of an `AgentCard` for integrity verification.
///
/// Follows the JSON format of RFC 7515 JSON Web Signature (JWS).
/// Callers can verify these to confirm the card hasn't been
/// tampered with in transit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCardSignature {
	/// The protected JWS header, Base64url-encoded JSON per RFC 7515.
	pub protected: String,

	/// The computed signature, Base64url-encoded.
	pub signature: String,

	/// Optional unprotected JWS header values.
	/// Constrained to a JSON object per proto3 `google.protobuf.Struct`
	/// semantics—arrays and scalars are not valid header values.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub header: Option<serde_json::Map<String, serde_json::Value>>,
}

/// The organisation or person operating an agent.
///
/// Both fields are required per the spec. The organization field
/// names the entity; the url points to their website or contact page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
	/// Organisation or person name.
	pub organization: String,

	/// Website or contact URL.
	pub url: String,
}

impl AgentProvider {
	/// Create a new agent provider.
	///
	/// Both fields are required per the spec—the organization name
	/// identifies who operates the agent, and the URL points to their
	/// website or contact page.
	///
	/// ```
	/// # use a2a::AgentProvider;
	/// let provider = AgentProvider::new("Arai", "https://arai.dev");
	/// assert_eq!(provider.organization, "Arai");
	/// ```
	#[must_use]
	pub fn new(organization: impl Into<String>, url: impl Into<String>) -> Self {
		Self {
			organization: organization.into(),
			url: url.into(),
		}
	}
}

impl AgentCardSignature {
	/// Create a new JWS signature for card integrity verification.
	///
	/// Takes the Base64url-encoded protected JWS header and the
	/// computed signature per RFC 7515. The optional unprotected
	/// header defaults to None; chain `with_header()` to attach it.
	///
	/// ```
	/// # use a2a::AgentCardSignature;
	/// let sig = AgentCardSignature::new("eyJhbGciOiJFZERTQSJ9", "abc123");
	/// assert_eq!(sig.protected, "eyJhbGciOiJFZERTQSJ9");
	/// ```
	#[must_use]
	pub fn new(protected: impl Into<String>, signature: impl Into<String>) -> Self {
		Self {
			protected: protected.into(),
			signature: signature.into(),
			header: None,
		}
	}

	/// Attach the optional unprotected JWS header.
	///
	/// Carries metadata like the key ID that doesn't need integrity
	/// protection but helps callers select the right verification key.
	#[must_use]
	pub fn with_header(mut self, header: serde_json::Map<String, serde_json::Value>) -> Self {
		self.header = Some(header);
		self
	}
}

/// Required fields for constructing an `AgentCard`.
///
/// Separates the 8 required fields from the optional ones, keeping
/// the constructor under clippy's argument limit while still requiring
/// all mandatory fields at compile time. Pass this to `AgentCard::new()`
/// and then chain optional setters.
///
/// ```
/// # use a2a::{AgentCardRequired, AgentCard, AgentCapabilities, AgentInterface};
/// let card = AgentCard::new(AgentCardRequired {
///     name: "Echo Agent".into(),
///     description: "Echoes messages back".into(),
///     supported_interfaces: vec![
///         AgentInterface::new("https://localhost:3000", "JSONRPC", "1.0"),
///     ],
///     version: "1.0".into(),
///     capabilities: AgentCapabilities::default(),
///     skills: vec![],
///     default_input_modes: vec!["text/plain".into()],
///     default_output_modes: vec!["text/plain".into()],
/// });
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AgentCardRequired {
	/// Human-readable name for this agent.
	pub name: String,

	/// Description of what this agent does.
	pub description: String,

	/// All protocol interfaces this agent supports.
	/// The first entry is the preferred interface.
	pub supported_interfaces: Vec<AgentInterface>,

	/// Card schema version.
	pub version: String,

	/// Protocol features this agent supports.
	pub capabilities: AgentCapabilities,

	/// The actions this agent can perform.
	pub skills: Vec<AgentSkill>,

	/// Default media types this agent accepts as input.
	pub default_input_modes: Vec<String>,

	/// Default media types this agent can produce as output.
	pub default_output_modes: Vec<String>,
}

impl AgentCard {
	/// Create a new agent card from the required fields.
	///
	/// Takes an `AgentCardRequired` struct containing all 8 mandatory
	/// fields and initialises collection fields to empty. Use
	/// the `with_*` chainable setters to populate optional fields.
	///
	/// ```
	/// # use a2a::{AgentCard, AgentCardRequired, AgentCapabilities, AgentInterface};
	/// let card = AgentCard::new(AgentCardRequired {
	///     name: "Echo Agent".into(),
	///     description: "Echoes messages back to the caller".into(),
	///     supported_interfaces: vec![
	///         AgentInterface::new("https://localhost:3000", "JSONRPC", "1.0"),
	///     ],
	///     version: "1.0".into(),
	///     capabilities: AgentCapabilities::default(),
	///     skills: vec![],
	///     default_input_modes: vec!["text/plain".into()],
	///     default_output_modes: vec!["text/plain".into()],
	/// })
	/// .with_documentation_url("https://docs.example.com");
	/// ```
	#[must_use]
	pub fn new(required: AgentCardRequired) -> Self {
		Self {
			name: required.name,
			description: required.description,
			supported_interfaces: required.supported_interfaces,
			version: required.version,
			capabilities: required.capabilities,
			skills: required.skills,
			default_input_modes: required.default_input_modes,
			default_output_modes: required.default_output_modes,
			provider: None,
			security_schemes: HashMap::new(),
			security_requirements: Vec::new(),
			documentation_url: None,
			icon_url: None,
			signatures: Vec::new(),
		}
	}

	/// Declare the organisation or person operating this agent.
	///
	/// The provider appears in agent discovery responses and helps
	/// callers identify who is responsible for the agent. Both the
	/// organization name and URL are required in the `AgentProvider`
	/// struct.
	#[must_use]
	pub fn with_provider(mut self, provider: AgentProvider) -> Self {
		self.provider = Some(provider);
		self
	}

	/// Define the authentication methods this agent accepts.
	///
	/// Keys are scheme names (e.g. "bearer-jwt", "api-key") that
	/// the `security_requirements` field references. Values are
	/// `SecurityScheme` definitions describing the scheme parameters.
	/// Modelled after `OpenAPI` security scheme definitions.
	#[must_use]
	pub fn with_security_schemes(mut self, schemes: HashMap<String, SecurityScheme>) -> Self {
		self.security_schemes = schemes;
		self
	}

	/// Set the security requirements that apply to all operations.
	///
	/// Each `SecurityRequirement` maps scheme names to required scopes.
	/// The scheme names must reference entries in `security_schemes`.
	/// Multiple entries represent alternative authentication options
	/// (the caller satisfies any one of them).
	#[must_use]
	pub fn with_security_requirements(
		mut self,
		security_requirements: Vec<SecurityRequirement>,
	) -> Self {
		self.security_requirements = security_requirements;
		self
	}

	/// Link to the agent's documentation.
	///
	/// Provides a URL where callers can find detailed usage guides,
	/// API references, or integration instructions beyond what the
	/// agent card's description and skill metadata convey.
	#[must_use]
	pub fn with_documentation_url(mut self, url: impl Into<String>) -> Self {
		self.documentation_url = Some(url.into());
		self
	}

	/// Set the agent's icon image URL.
	///
	/// Used by discovery interfaces and agent directories to display a
	/// visual representation of the agent alongside its name and
	/// description.
	#[must_use]
	pub fn with_icon_url(mut self, url: impl Into<String>) -> Self {
		self.icon_url = Some(url.into());
		self
	}

	/// Attach JWS signatures for card integrity verification.
	///
	/// Follows RFC 7515 JSON Web Signature format. Callers can verify
	/// these signatures to confirm the card hasn't been tampered with
	/// in transit—important when agent cards are cached or proxied
	/// through intermediaries.
	#[must_use]
	pub fn with_signatures(mut self, signatures: Vec<AgentCardSignature>) -> Self {
		self.signatures = signatures;
		self
	}
}
