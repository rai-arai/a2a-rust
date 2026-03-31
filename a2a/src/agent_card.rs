// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Agent discovery and capability declaration.
//!
//! Before communicating with an agent, callers discover what the agent
//! can do by fetching its Agent Card from the well-known endpoint
//! (/.well-known/agent.json). The card declares the agent's identity,
//! capabilities, skills, supported interfaces, and security requirements.
//!
//! The card is the handshake mechanism—it tells a potential caller
//! everything needed to decide whether and how to interact with the
//! agent. This is analogous to `OpenAPI` specs for REST APIs, but
//! designed for the agent-to-agent paradigm where both sides are
//! autonomous software agents.
//!
//! Skills describe what the agent can do (e.g. "render web pages",
//! "translate documents"). Capabilities declare protocol features
//! the agent supports (streaming, push notifications). Interfaces
//! list the protocol bindings (JSON-RPC endpoint URLs). Security
//! schemes define how callers authenticate.
//!
//! The card can optionally be signed (`AgentCardSignature`) so callers
//! can verify it hasn't been tampered with in transit.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
	#[serde(skip_serializing_if = "Option::is_none")]
	pub header: Option<serde_json::Value>,
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
	#[serde(skip_serializing_if = "Option::is_none")]
	pub params: Option<serde_json::Value>,
}

/// A security requirement entry referencing one or more security schemes.
///
/// Maps scheme names to their required scope lists. The scope list
/// is wrapped in a `StringList` to match the proto3 JSON wire format,
/// where `map<string, StringList>` serialises each value as
/// `{"list": [...]}` rather than a bare array.
///
/// Multiple `SecurityRequirement` entries in a Vec represent
/// alternatives—the caller must satisfy any one of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityRequirement {
	/// Map of security scheme name to the list of required scopes.
	///
	/// Example: `{"oauth2": {"list": ["read", "write"]}}` means
	/// the caller must present an oauth2 token with read and write
	/// scopes. An empty list means any valid credential suffices.
	pub schemes: HashMap<String, StringList>,
}

/// A named list of strings, matching the proto3 `StringList` message.
///
/// Used as the value type in `SecurityRequirement.schemes` to
/// faithfully represent the proto3 JSON wire format where
/// `map<string, StringList>` serialises values as `{"list": [...]}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StringList {
	/// The list of scope strings.
	pub list: Vec<String>,
}

impl SecurityRequirement {
	/// Create a security requirement from a map of scheme name to scope list.
	///
	/// Each entry maps a scheme name (which must reference an entry in
	/// the card-level `security_schemes`) to the required scopes.
	///
	/// ```
	/// # use std::collections::HashMap;
	/// # use a2a::{SecurityRequirement, StringList};
	/// let req = SecurityRequirement::new(HashMap::from([
	///     ("oauth2".into(), StringList { list: vec!["read".into()] }),
	/// ]));
	/// ```
	#[must_use]
	pub fn new(schemes: HashMap<String, StringList>) -> Self {
		Self { schemes }
	}

	/// Build a security requirement for a single scheme and scope list.
	///
	/// Constructs the internal `HashMap` from the given scheme name and
	/// a slice of scope string literals. This is the most common
	/// construction path when a single scheme covers all required scopes:
	///
	/// ```
	/// # use a2a::SecurityRequirement;
	/// let req = SecurityRequirement::from_scheme("oauth2", &["read", "write"]);
	/// assert!(req.schemes.contains_key("oauth2"));
	/// assert_eq!(req.schemes["oauth2"].list, vec!["read", "write"]);
	/// ```
	#[must_use]
	pub fn from_scheme(name: impl Into<String>, scopes: &[&str]) -> Self {
		let mut schemes = std::collections::HashMap::new();
		schemes.insert(
			name.into(),
			StringList {
				list: scopes.iter().map(|scope| (*scope).to_string()).collect(),
			},
		);
		Self { schemes }
	}
}

impl StringList {
	/// Create a `StringList` from a vec of scope strings.
	///
	/// ```
	/// # use a2a::StringList;
	/// let scopes = StringList::new(vec!["read".into(), "write".into()]);
	/// assert_eq!(scopes.list.len(), 2);
	/// ```
	#[must_use]
	pub fn new(list: Vec<String>) -> Self {
		Self { list }
	}
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
	pub fn with_params(mut self, params: serde_json::Value) -> Self {
		self.params = Some(params);
		self
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
	pub fn with_header(mut self, header: serde_json::Value) -> Self {
		self.header = Some(header);
		self
	}
}

/// Authentication methods the agent accepts.
///
/// Modelled after `OpenAPI` security schemes. The "type" field
/// discriminates between API key, HTTP auth, `OAuth2`, `OpenID`
/// Connect, and mutual TLS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum SecurityScheme {
	/// API key passed in a header or query parameter.
	#[serde(rename = "apiKey", rename_all = "camelCase")]
	ApiKey {
		/// The header or query parameter name.
		name: String,
		/// Where the key is sent: "header" or "query".
		#[serde(rename = "in")]
		location: String,
		/// Human-readable description.
		#[serde(skip_serializing_if = "Option::is_none")]
		description: Option<String>,
	},

	/// Standard HTTP authentication (Bearer, Basic, etc.).
	#[serde(rename = "http", rename_all = "camelCase")]
	Http {
		/// The HTTP auth scheme (e.g. "bearer", "basic").
		scheme: String,
		/// Hint about the token format (e.g. "JWT").
		#[serde(skip_serializing_if = "Option::is_none")]
		bearer_format: Option<String>,
		/// Human-readable description.
		#[serde(skip_serializing_if = "Option::is_none")]
		description: Option<String>,
	},

	/// OAuth 2.0 authentication flows.
	#[serde(rename = "oauth2", rename_all = "camelCase")]
	OAuth2 {
		/// The supported `OAuth2` flow definitions.
		/// Boxed because `OAuthFlows` contains multiple nested flow
		/// structs with URLs and scopes, making it significantly
		/// larger than the other `SecurityScheme` variants. Without
		/// boxing, the entire enum would be sized to the `OAuth2`
		/// variant even for simple `ApiKey` or Http schemes.
		flows: Box<OAuthFlows>,
		/// Human-readable description.
		#[serde(skip_serializing_if = "Option::is_none")]
		description: Option<String>,
		/// URL to the `OAuth2` server metadata endpoint (RFC 8414).
		/// Callers can fetch this to discover token and authorisation
		/// endpoints instead of configuring them statically.
		#[serde(skip_serializing_if = "Option::is_none")]
		oauth2_metadata_url: Option<String>,
	},

	/// `OpenID` Connect discovery-based authentication.
	#[serde(rename = "openIdConnect", rename_all = "camelCase")]
	OpenIdConnect {
		/// The `OpenID` Connect discovery endpoint URL.
		open_id_connect_url: String,
		/// Human-readable description.
		#[serde(skip_serializing_if = "Option::is_none")]
		description: Option<String>,
	},

	/// Mutual TLS authentication.
	#[serde(rename = "mutualTLS", rename_all = "camelCase")]
	MutualTls {
		/// Human-readable description.
		#[serde(skip_serializing_if = "Option::is_none")]
		description: Option<String>,
	},
}

impl SecurityScheme {
	/// Create an API key security scheme.
	///
	/// The name is the header or query parameter that carries the key,
	/// and the location is "header" or "query". Description defaults
	/// to None; chain `with_description()` to add it.
	///
	/// ```
	/// # use a2a::SecurityScheme;
	/// let scheme = SecurityScheme::api_key("X-API-Key", "header");
	/// ```
	#[must_use]
	pub fn api_key(name: impl Into<String>, location: impl Into<String>) -> Self {
		Self::ApiKey {
			name: name.into(),
			location: location.into(),
			description: None,
		}
	}

	/// Create an HTTP authentication security scheme.
	///
	/// The scheme parameter is the HTTP auth scheme name (e.g.
	/// "bearer", "basic"). Bearer format and description default
	/// to None; chain `with_bearer_format()` and `with_description()`
	/// to populate them.
	///
	/// ```
	/// # use a2a::SecurityScheme;
	/// let scheme = SecurityScheme::http("bearer").with_bearer_format("JWT");
	/// ```
	#[must_use]
	pub fn http(scheme: impl Into<String>) -> Self {
		Self::Http {
			scheme: scheme.into(),
			bearer_format: None,
			description: None,
		}
	}

	/// Create an OAuth 2.0 security scheme from flow definitions.
	///
	/// Takes an `OAuthFlows` instance describing the supported grant
	/// types. Description and `oauth2_metadata_url` default to None.
	///
	/// ```
	/// # use a2a::{SecurityScheme, OAuthFlows, ClientCredentialsOAuthFlow};
	/// # use std::collections::HashMap;
	/// let scheme = SecurityScheme::oauth2(
	///     OAuthFlows::default().with_client_credentials(
	///         ClientCredentialsOAuthFlow::new(
	///             "https://auth.example.com/token",
	///             HashMap::new(),
	///         )
	///     )
	/// );
	/// ```
	#[must_use]
	pub fn oauth2(flows: OAuthFlows) -> Self {
		Self::OAuth2 {
			flows: Box::new(flows),
			description: None,
			oauth2_metadata_url: None,
		}
	}

	/// Create an `OpenID` Connect discovery-based security scheme.
	///
	/// Takes the `OpenID` Connect discovery endpoint URL. Callers
	/// fetch the OIDC configuration from this URL to learn the
	/// authorization, token, and userinfo endpoints.
	///
	/// ```
	/// # use a2a::SecurityScheme;
	/// let scheme = SecurityScheme::openid_connect(
	///     "https://auth.example.com/.well-known/openid-configuration"
	/// );
	/// ```
	#[must_use]
	pub fn openid_connect(url: impl Into<String>) -> Self {
		Self::OpenIdConnect {
			open_id_connect_url: url.into(),
			description: None,
		}
	}

	/// Create a mutual TLS security scheme.
	///
	/// No configuration is needed—the TLS layer handles certificate
	/// exchange. Description defaults to None.
	///
	/// ```
	/// # use a2a::SecurityScheme;
	/// let scheme = SecurityScheme::mutual_tls();
	/// ```
	#[must_use]
	pub fn mutual_tls() -> Self {
		Self::MutualTls { description: None }
	}

	/// Attach a human-readable description to this security scheme.
	///
	/// Works on all variants. The description helps callers understand
	/// what credentials are expected and how to obtain them.
	#[must_use]
	pub fn with_description(mut self, description: impl Into<String>) -> Self {
		match &mut self {
			Self::ApiKey {
				description: desc, ..
			}
			| Self::Http {
				description: desc, ..
			}
			| Self::OAuth2 {
				description: desc, ..
			}
			| Self::OpenIdConnect {
				description: desc, ..
			}
			| Self::MutualTls { description: desc } => {
				*desc = Some(description.into());
			}
		}
		self
	}

	/// Set the bearer token format hint for HTTP auth schemes.
	///
	/// Only meaningful on the Http variant—hints at the token
	/// format (e.g. "JWT") for documentation purposes. On other
	/// variants this is a no-op.
	#[must_use]
	pub fn with_bearer_format(mut self, format: impl Into<String>) -> Self {
		if let Self::Http { bearer_format, .. } = &mut self {
			*bearer_format = Some(format.into());
		}
		self
	}

	/// Set the `OAuth2` server metadata URL for `OAuth2` schemes.
	///
	/// Only meaningful on the `OAuth2` variant—points callers to the
	/// RFC 8414 server metadata document where token and authorization
	/// endpoints can be discovered. On other variants this is a no-op.
	#[must_use]
	pub fn with_oauth2_metadata_url(mut self, url: impl Into<String>) -> Self {
		if let Self::OAuth2 {
			oauth2_metadata_url,
			..
		} = &mut self
		{
			*oauth2_metadata_url = Some(url.into());
		}
		self
	}
}

/// OAuth 2.0 flow definitions.
///
/// At least one flow should be defined. Each flow specifies the
/// endpoints needed for that particular OAuth grant type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlows {
	/// Authorization code flow (recommended for interactive callers).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub authorization_code: Option<AuthorizationCodeOAuthFlow>,

	/// Client credentials flow (for service-to-service communication).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub client_credentials: Option<ClientCredentialsOAuthFlow>,

	/// Device code flow (for constrained environments).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub device_code: Option<DeviceCodeOAuthFlow>,

	/// Implicit flow. Deprecated in the v1.0 spec — prefer
	/// authorization code with PKCE for interactive callers. Present
	/// so agent cards that still declare it can be deserialised.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub implicit: Option<ImplicitOAuthFlow>,

	/// Resource owner password credentials flow. Deprecated in the
	/// v1.0 spec. Present so agent cards that still declare it can
	/// be deserialised.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub password: Option<PasswordOAuthFlow>,
}

impl OAuthFlows {
	/// Attach an authorization code flow definition.
	#[must_use]
	pub fn with_authorization_code(mut self, flow: AuthorizationCodeOAuthFlow) -> Self {
		self.authorization_code = Some(flow);
		self
	}

	/// Attach a client credentials flow definition.
	#[must_use]
	pub fn with_client_credentials(mut self, flow: ClientCredentialsOAuthFlow) -> Self {
		self.client_credentials = Some(flow);
		self
	}

	/// Attach a device code flow definition.
	#[must_use]
	pub fn with_device_code(mut self, flow: DeviceCodeOAuthFlow) -> Self {
		self.device_code = Some(flow);
		self
	}
}

/// Authorization code OAuth flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationCodeOAuthFlow {
	/// Where to send the person for authorization.
	pub authorization_url: String,

	/// Where to exchange the code for an access token.
	pub token_url: String,

	/// Where to refresh an expired token.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub refresh_url: Option<String>,

	/// Whether PKCE (RFC 7636) is required for this flow.
	/// When true, callers must send a `code_challenge` with the
	/// authorization request and a `code_verifier` at token exchange.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub pkce_required: Option<bool>,

	/// Available permission scopes (name -> description).
	pub scopes: HashMap<String, String>,
}

impl AuthorizationCodeOAuthFlow {
	/// Create an authorization code flow with the required endpoints.
	///
	/// Takes the authorization URL (where the person is redirected),
	/// the token URL (where the code is exchanged for a token), and
	/// the available scopes. Refresh URL and `pkce_required` default
	/// to None.
	///
	/// ```
	/// # use a2a::AuthorizationCodeOAuthFlow;
	/// # use std::collections::HashMap;
	/// let flow = AuthorizationCodeOAuthFlow::new(
	///     "https://auth.example.com/authorize",
	///     "https://auth.example.com/token",
	///     HashMap::from([("read".into(), "read access".into())]),
	/// );
	/// ```
	#[must_use]
	pub fn new(
		authorization_url: impl Into<String>,
		token_url: impl Into<String>,
		scopes: HashMap<String, String>,
	) -> Self {
		Self {
			authorization_url: authorization_url.into(),
			token_url: token_url.into(),
			refresh_url: None,
			pkce_required: None,
			scopes,
		}
	}

	/// Set the token refresh endpoint URL.
	#[must_use]
	pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
		self.refresh_url = Some(url.into());
		self
	}

	/// Require PKCE (RFC 7636) for this authorization code flow.
	///
	/// When true, callers must include a `code_challenge` parameter
	/// with the authorization request and a `code_verifier` parameter
	/// at token exchange. Strongly recommended for public clients.
	#[must_use]
	pub fn with_pkce_required(mut self, pkce_required: bool) -> Self {
		self.pkce_required = Some(pkce_required);
		self
	}
}

/// Client credentials OAuth flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCredentialsOAuthFlow {
	/// Where to request an access token.
	pub token_url: String,

	/// Where to refresh an expired token.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub refresh_url: Option<String>,

	/// Available permission scopes (name -> description).
	pub scopes: HashMap<String, String>,
}

impl ClientCredentialsOAuthFlow {
	/// Create a client credentials flow with the required fields.
	///
	/// Takes the token URL and available scopes. Refresh URL defaults
	/// to None. This is the most common flow for service-to-service
	/// agent communication.
	///
	/// ```
	/// # use a2a::ClientCredentialsOAuthFlow;
	/// # use std::collections::HashMap;
	/// let flow = ClientCredentialsOAuthFlow::new(
	///     "https://auth.example.com/token",
	///     HashMap::from([("read".into(), "read access".into())]),
	/// );
	/// ```
	#[must_use]
	pub fn new(token_url: impl Into<String>, scopes: HashMap<String, String>) -> Self {
		Self {
			token_url: token_url.into(),
			refresh_url: None,
			scopes,
		}
	}

	/// Set the token refresh endpoint URL.
	#[must_use]
	pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
		self.refresh_url = Some(url.into());
		self
	}
}

/// Device code OAuth flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeOAuthFlow {
	/// Where to request a device code.
	pub device_authorization_url: String,

	/// Where to poll for the access token.
	pub token_url: String,

	/// Where to refresh an expired token.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub refresh_url: Option<String>,

	/// Available permission scopes (name -> description).
	pub scopes: HashMap<String, String>,
}

impl DeviceCodeOAuthFlow {
	/// Create a device code flow with the required fields.
	///
	/// Takes the device authorization URL (where the device requests
	/// a code), the token URL (where the device polls for the token),
	/// and the available scopes. Refresh URL defaults to None.
	///
	/// ```
	/// # use a2a::DeviceCodeOAuthFlow;
	/// # use std::collections::HashMap;
	/// let flow = DeviceCodeOAuthFlow::new(
	///     "https://auth.example.com/device",
	///     "https://auth.example.com/token",
	///     HashMap::new(),
	/// );
	/// ```
	#[must_use]
	pub fn new(
		device_authorization_url: impl Into<String>,
		token_url: impl Into<String>,
		scopes: HashMap<String, String>,
	) -> Self {
		Self {
			device_authorization_url: device_authorization_url.into(),
			token_url: token_url.into(),
			refresh_url: None,
			scopes,
		}
	}

	/// Set the token refresh endpoint URL.
	#[must_use]
	pub fn with_refresh_url(mut self, url: impl Into<String>) -> Self {
		self.refresh_url = Some(url.into());
		self
	}
}

/// Implicit OAuth flow.
///
/// Deprecated in the A2A v1.0 spec — the authorization code flow with
/// PKCE is the recommended replacement for interactive callers. This
/// type exists so that agent cards from implementations that still
/// declare the implicit flow can be deserialised without error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImplicitOAuthFlow {
	/// The authorization endpoint URL.
	pub authorization_url: String,

	/// The token refresh endpoint URL.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub refresh_url: Option<String>,

	/// Available scopes and their descriptions.
	#[serde(default, skip_serializing_if = "HashMap::is_empty")]
	pub scopes: HashMap<String, String>,
}

/// Resource owner password credentials OAuth flow.
///
/// Deprecated in the A2A v1.0 spec — exposing user credentials to
/// third-party agents is discouraged. This type exists so that agent
/// cards from implementations that still declare the password flow
/// can be deserialised without error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordOAuthFlow {
	/// The token endpoint URL.
	pub token_url: String,

	/// The token refresh endpoint URL.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub refresh_url: Option<String>,

	/// Available scopes and their descriptions.
	#[serde(default, skip_serializing_if = "HashMap::is_empty")]
	pub scopes: HashMap<String, String>,
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

// Recursively sort all JSON object keys lexicographically in place.
//
// serde_json::Map preserves insertion order, which is determined by the struct
// field declaration order in Rust. Different implementations and different Rust
// struct layouts would produce different field orderings for the same logical
// document—making the raw serialised bytes non-deterministic across
// implementations. Sorting all object keys at every nesting level before
// serialising produces a canonical form whose byte representation is stable
// regardless of the originating language or library.
//
// This matches the JWS convention for detached payloads used by the A2A spec:
// the signer and verifier must independently produce identical bytes from the
// same logical card, so the serialisation algorithm must be fully specified.
#[cfg(feature = "jws")]
pub(crate) fn canonicalise_json(value: &mut serde_json::Value) {
	match value {
		serde_json::Value::Object(map) => {
			for v in map.values_mut() {
				canonicalise_json(v);
			}
			// Take ownership of the map's entries to avoid cloning every key
			// and value. std::mem::take replaces the map with an empty one and
			// gives us the original, which we drain into a BTreeMap for
			// lexicographic key ordering. The sorted entries are then collected
			// back into the now-empty map. This is O(n log n) for the sort
			// with zero deep clones, versus the previous approach which cloned
			// every nested Value before sorting.
			let sorted: std::collections::BTreeMap<String, serde_json::Value> =
				std::mem::take(map).into_iter().collect();
			*map = sorted.into_iter().collect();
		}
		serde_json::Value::Array(arr) => {
			for v in arr {
				canonicalise_json(v);
			}
		}
		_ => {}
	}
}

#[cfg(feature = "jws")]
impl AgentCard {
	/// Verifies all signatures on this agent card against the provided keys.
	///
	/// For each signature the method extracts the key ID (`kid`) from the
	/// protected or unprotected header, finds a matching key by `kid`, and
	/// verifies the signature over the card's canonical payload.
	///
	/// The canonical payload is this card serialised to JSON with the
	/// `signatures` field set to an empty vec—reproducing the content that
	/// was originally signed. This mirrors the signing convention described
	/// in the A2A spec: the signer removes the `signatures` field before
	/// computing the signature, so the verifier must do the same.
	///
	/// Canonical form is defined as: all JSON object keys sorted
	/// lexicographically at every nesting level, with the `signatures` field
	/// omitted. This removes any dependence on struct field declaration order
	/// or insertion order so that signers and verifiers written in different
	/// languages or using different libraries produce identical bytes.
	///
	/// Returns `Ok(())` if every signature verifies successfully, or the
	/// first verification error encountered. Returns `NoSignatures` if the
	/// card carries no signatures—callers that require integrity verification
	/// should treat this as a failure.
	///
	/// # Errors
	///
	/// Returns [`VerificationError::NoSignatures`](crate::jws::VerificationError::NoSignatures)
	/// if the card has no signatures. Returns other `VerificationError` variants
	/// if serialisation fails, a key ID has no match, or a signature does not
	/// verify against any of the provided keys.
	pub fn verify_signatures(
		&self,
		keys: &[crate::jws::VerificationKey],
	) -> Result<(), crate::jws::VerificationError> {
		if self.signatures.is_empty() {
			return Err(crate::jws::VerificationError::NoSignatures);
		}

		// Build the canonical payload: clone the card, clear the signatures
		// field, serialise to a serde_json::Value, sort all object keys
		// lexicographically at every nesting level, then convert to bytes.
		// The sort step ensures the byte representation is identical across
		// implementations regardless of struct field declaration order.
		let mut canonical = self.clone();
		canonical.signatures = vec![];
		let mut canonical_value = serde_json::to_value(&canonical).map_err(|error| {
			crate::jws::VerificationError::Serialization(format!(
				"failed to serialise canonical card: {error}"
			))
		})?;
		canonicalise_json(&mut canonical_value);
		let payload = serde_json::to_vec(&canonical_value).map_err(|error| {
			crate::jws::VerificationError::Serialization(format!(
				"failed to convert canonical value to bytes: {error}"
			))
		})?;

		for signature in &self.signatures {
			let kid = extract_kid_from_signature(signature);

			// If a kid is available, try the matching key first. This avoids
			// trying every key in a large set for every signature, which is
			// especially important when the set contains RSA keys where
			// verification is relatively expensive.
			let matched_key = kid.as_deref().and_then(|kid_value| {
				keys.iter()
					.find(|key| key.kid.as_deref() == Some(kid_value))
			});

			if let Some(key) = matched_key {
				crate::jws::verify_detached_jws(
					&signature.protected,
					&signature.signature,
					&payload,
					key,
				)?;
			} else {
				verify_with_any_key(signature, &payload, keys)?;
			}
		}

		Ok(())
	}
}

// Extract the key ID (`kid`) from a JWS signature, checking the unprotected
// header first and then the protected header.
//
// The unprotected header is already parsed JSON, so it is the cheaper path.
// The protected header must be Base64url-decoded and parsed, so it is only
// tried when the unprotected header does not provide a `kid`. RFC 7515 §4.1.4
// states that `kid` may appear in either location; checking both ensures that
// cards produced by any conformant implementation are handled correctly.
#[cfg(feature = "jws")]
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

/// Try every key and accept the first that successfully verifies the
/// signature. When no key succeeds, returns the most diagnostically
/// useful error.
///
/// Error priority: if any key had a matching algorithm but the signature
/// did not verify (`SignatureInvalid`, `InvalidSignature`), that error is
/// returned — it tells the operator the key was rotated or the card was
/// tampered with. `AlgorithmKeyMismatch` is only returned when no key in
/// the set had a compatible algorithm, which points to a configuration
/// issue rather than a key rotation problem.
#[cfg(feature = "jws")]
fn verify_with_any_key(
	signature: &AgentCardSignature,
	payload: &[u8],
	keys: &[crate::jws::VerificationKey],
) -> Result<(), crate::jws::VerificationError> {
	use crate::jws::VerificationError;

	let mut compatible_error: Option<VerificationError> = None;
	let mut mismatch_error: Option<VerificationError> = None;

	for key in keys {
		match crate::jws::verify_detached_jws(
			&signature.protected,
			&signature.signature,
			payload,
			key,
		) {
			Ok(_algorithm) => return Ok(()),
			Err(error @ VerificationError::AlgorithmKeyMismatch { .. }) => {
				mismatch_error.get_or_insert(error);
			}
			Err(error) => {
				compatible_error = Some(error);
			}
		}
	}

	// Prefer the error from a compatible key — it is more actionable.
	// Fall back to the mismatch error if no compatible key was found.
	// If the key slice was empty, return SignatureInvalid as a default.
	Err(compatible_error
		.or(mismatch_error)
		.unwrap_or(VerificationError::SignatureInvalid))
}

#[cfg(test)]
mod tests {
	use super::*;

	// Helper that builds the minimal valid AgentCard using struct
	// literal syntax. Tests that verify field presence or serialisation
	// behaviour use this as a baseline—only the field being tested
	// differs from this baseline. Keeping the helper exhaustive ensures
	// any new required fields cause a compile error here first.
	fn minimal_card() -> AgentCard {
		AgentCard {
			name: "Test Agent".into(),
			description: "A test agent for unit tests".into(),
			supported_interfaces: vec![AgentInterface {
				url: "https://agent.example.com/a2a".into(),
				protocol_binding: "JSONRPC".into(),
				tenant: None,
				protocol_version: "1.0".into(),
			}],
			version: "1.0".into(),
			capabilities: AgentCapabilities {
				streaming: None,
				push_notifications: None,
				extended_agent_card: None,
				extensions: vec![],
			},
			skills: vec![],
			default_input_modes: vec!["text/plain".into()],
			default_output_modes: vec!["text/plain".into()],
			provider: None,
			security_schemes: HashMap::new(),
			security_requirements: Vec::new(),
			documentation_url: None,
			icon_url: None,
			signatures: Vec::new(),
		}
	}

	// A minimal agent card should serialise with only the required
	// fields present. Optional fields and empty collection fields must
	// be absent from the wire format—not null, not empty arrays, just
	// not there. This keeps discovery responses compact and matches
	// how other A2A implementations behave.
	//
	// security_schemes, security_requirements, signatures, and
	// extensions are all Vec/HashMap fields that serialise as absent
	// when empty because of skip_serializing_if = "*.is_empty".
	#[test]
	fn minimal_card_wire_format() {
		let card = minimal_card();
		let json = serde_json::to_value(&card).unwrap();
		let obj = json.as_object().unwrap();

		assert_eq!(obj["name"], "Test Agent");
		assert_eq!(obj["version"], "1.0");

		// supportedInterfaces must be present (required field).
		assert!(obj.contains_key("supportedInterfaces"));
		assert!(obj["supportedInterfaces"].as_array().unwrap().len() == 1);

		// Old top-level URL field must be absent.
		assert!(!obj.contains_key("url"));

		// All optional fields must be absent.
		assert!(!obj.contains_key("provider"));
		assert!(!obj.contains_key("preferredTransport"));
		assert!(!obj.contains_key("additionalInterfaces"));
		assert!(!obj.contains_key("securitySchemes"));
		assert!(!obj.contains_key("securityRequirements"));
		assert!(!obj.contains_key("supportsAuthenticatedExtendedCard"));
		assert!(!obj.contains_key("documentationUrl"));
		assert!(!obj.contains_key("iconUrl"));
		assert!(!obj.contains_key("signatures"));

		// extensions must be absent when the Vec is empty.
		let caps = obj["capabilities"].as_object().unwrap();
		assert!(!caps.contains_key("extensions"));
	}

	// AgentInterface must serialise the new field names introduced in
	// v1.0. The `protocol` field is gone; `protocol_binding` takes its
	// place. Optional fields `tenant` and `protocol_version` must be
	// absent when None.
	#[test]
	fn interface_wire_format_v1() {
		let iface = AgentInterface {
			url: "https://example.com/a2a".into(),
			protocol_binding: "JSONRPC".into(),
			tenant: None,
			protocol_version: "1.0".into(),
		};
		let json = serde_json::to_value(&iface).unwrap();
		let obj = json.as_object().unwrap();

		assert_eq!(json["url"], "https://example.com/a2a");
		assert_eq!(json["protocolBinding"], "JSONRPC");
		assert_eq!(json["protocolVersion"], "1.0");

		// Old field name must not appear.
		assert!(!obj.contains_key("transport"));
		assert!(!obj.contains_key("protocol"));

		// The optional tenant field must be absent when None.
		assert!(!obj.contains_key("tenant"));
	}

	// AgentInterface optional tenant field must appear in the wire
	// format when set. protocol_version is now required—always present.
	#[test]
	fn interface_optional_fields_appear_when_set() {
		let iface = AgentInterface::new("https://example.com/a2a", "JSONRPC", "1.0")
			.with_tenant("acme-corp");

		let json = serde_json::to_value(&iface).unwrap();
		assert_eq!(json["tenant"], "acme-corp");
		assert_eq!(json["protocolVersion"], "1.0");
	}

	// AgentInterface::new() takes (url, protocol_binding, protocol_version)—
	// all three are required. An interface without any of these is
	// meaningless for discovery.
	#[test]
	fn interface_new_sets_required_fields() {
		let iface = AgentInterface::new("https://example.com/a2a", "JSONRPC", "1.0");
		assert_eq!(iface.url, "https://example.com/a2a");
		assert_eq!(iface.protocol_binding, "JSONRPC");
		assert_eq!(iface.protocol_version, "1.0");
		assert!(iface.tenant.is_none());
	}

	// AgentInterface constructed via new() must match the struct
	// literal wire format exactly, confirming the constructor is a
	// drop-in replacement without changing serialisation behaviour.
	#[test]
	fn interface_new_matches_struct_literal() {
		let from_constructor = AgentInterface::new("https://example.com/grpc", "GRPC", "1.0");
		let from_literal = AgentInterface {
			url: "https://example.com/grpc".into(),
			protocol_binding: "GRPC".into(),
			tenant: None,
			protocol_version: "1.0".into(),
		};
		assert_eq!(
			serde_json::to_value(&from_constructor).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// All capability fields default to None or empty-Vec when omitted
	// from JSON. Callers should treat absent capabilities as unsupported.
	// The extensions Vec must deserialise to an empty Vec (not fail)
	// when the key is absent.
	#[test]
	fn capabilities_default_to_none() {
		let json = r#"{}"#;
		let caps: AgentCapabilities = serde_json::from_str(json).unwrap();
		assert!(caps.streaming.is_none());
		assert!(caps.push_notifications.is_none());
		assert!(caps.extended_agent_card.is_none());
		assert!(caps.extensions.is_empty());
	}

	// API key is the simplest security scheme—just a name and
	// location. The "type" tag must be "apiKey" (camelCase, not
	// "api_key" or "ApiKey") and the location field must serialise
	// as "in" per the OpenAPI-derived spec.
	#[test]
	fn api_key_security_scheme_wire_format() {
		let scheme = SecurityScheme::ApiKey {
			name: "X-API-Key".into(),
			location: "header".into(),
			description: None,
		};
		let json = serde_json::to_value(&scheme).unwrap();
		assert_eq!(json["type"], "apiKey");
		assert_eq!(json["name"], "X-API-Key");
		assert_eq!(json["in"], "header");
	}

	// HTTP bearer authentication is the most common scheme for
	// service-to-service agent communication. The "type" tag must
	// be "http" and the bearerFormat field (camelCase) hints at
	// the token format for documentation purposes.
	#[test]
	fn http_bearer_security_scheme_wire_format() {
		let scheme = SecurityScheme::Http {
			scheme: "bearer".into(),
			bearer_format: Some("JWT".into()),
			description: None,
		};
		let json = serde_json::to_value(&scheme).unwrap();
		assert_eq!(json["type"], "http");
		assert_eq!(json["scheme"], "bearer");
		assert_eq!(json["bearerFormat"], "JWT");
	}

	// OAuth2 is the most complex security scheme with nested flow
	// definitions. This test exercises the client credentials flow
	// (the most common for agent-to-agent auth) and verifies the
	// deep nesting serialises with correct camelCase field names.
	// Scopes are now HashMap<String, String> rather than serde_json::Value.
	#[test]
	fn oauth2_security_scheme_wire_format() {
		let scheme = SecurityScheme::OAuth2 {
			flows: Box::new(OAuthFlows {
				authorization_code: None,
				client_credentials: Some(ClientCredentialsOAuthFlow {
					token_url: "https://auth.example.com/token".into(),
					refresh_url: None,
					scopes: HashMap::from([
						("read".into(), "read access".into()),
						("write".into(), "write access".into()),
					]),
				}),
				device_code: None,
				implicit: None,
				password: None,
			}),
			description: None,
			oauth2_metadata_url: None,
		};
		let json = serde_json::to_value(&scheme).unwrap();
		assert_eq!(json["type"], "oauth2");
		assert!(json["flows"]["clientCredentials"].is_object());
		assert_eq!(
			json["flows"]["clientCredentials"]["tokenUrl"],
			"https://auth.example.com/token"
		);
	}

	// oauth2_metadata_url must appear in the wire format when set on
	// the OAuth2 variant, and must be absent when None.
	#[test]
	fn oauth2_metadata_url_wire_format() {
		let scheme = SecurityScheme::oauth2(OAuthFlows::default()).with_oauth2_metadata_url(
			"https://auth.example.com/.well-known/oauth-authorization-server",
		);

		let json = serde_json::to_value(&scheme).unwrap();
		assert_eq!(
			json["oauth2MetadataUrl"],
			"https://auth.example.com/.well-known/oauth-authorization-server"
		);

		// Verify it is absent when not set.
		let without_url = SecurityScheme::oauth2(OAuthFlows::default());
		let json2 = serde_json::to_value(&without_url).unwrap();
		assert!(!json2.as_object().unwrap().contains_key("oauth2MetadataUrl"));
	}

	// with_oauth2_metadata_url() must be a no-op on non-OAuth2 variants.
	// Applying it to an Http scheme must not modify any field.
	#[test]
	fn oauth2_metadata_url_noop_on_non_oauth2() {
		let scheme = SecurityScheme::http("bearer").with_oauth2_metadata_url(
			"https://auth.example.com/.well-known/oauth-authorization-server",
		);
		match &scheme {
			SecurityScheme::Http {
				bearer_format,
				description,
				..
			} => {
				assert!(bearer_format.is_none());
				assert!(description.is_none());
			}
			_ => panic!("expected Http variant"),
		}
	}

	// A fully populated card with skills, security, provider, and
	// supported_interfaces exercises the deepest nesting the card
	// structure can have. Round-tripping this confirms no fields
	// are lost or mangled at any level of the hierarchy.
	#[test]
	fn full_card_round_trips() {
		let mut security_schemes = HashMap::new();
		security_schemes.insert(
			"bearer-jwt".into(),
			SecurityScheme::Http {
				scheme: "bearer".into(),
				bearer_format: Some("JWT".into()),
				description: Some("Service-to-service JWT authentication".into()),
			},
		);

		let card = AgentCard {
			name: "Browser Rendering Agent".into(),
			description: "Renders web pages via headless browser".into(),
			supported_interfaces: vec![
				AgentInterface {
					url: "https://browser.agent.example.com/a2a".into(),
					protocol_binding: "JSONRPC".into(),
					tenant: None,
					protocol_version: "1.0".into(),
				},
				AgentInterface {
					url: "https://browser.agent.example.com/grpc".into(),
					protocol_binding: "GRPC".into(),
					tenant: None,
					protocol_version: "1.0".into(),
				},
			],
			version: "1.0".into(),
			capabilities: AgentCapabilities {
				streaming: Some(true),
				push_notifications: Some(false),
				extended_agent_card: None,
				extensions: vec![],
			},
			skills: vec![AgentSkill {
				id: "render-page".into(),
				name: "Render Web Page".into(),
				description: "Fetches and renders a URL via headless browser".into(),
				tags: vec!["browser".into(), "rendering".into()],
				examples: vec!["Render https://example.com".into()],
				input_modes: vec!["text/plain".into()],
				output_modes: vec!["text/html".into(), "text/plain".into()],
				security_requirements: vec![],
			}],
			default_input_modes: vec!["text/plain".into()],
			default_output_modes: vec!["text/plain".into(), "text/html".into()],
			provider: Some(AgentProvider {
				organization: "Arai".into(),
				url: "https://arai.dev".into(),
			}),
			security_schemes,
			security_requirements: vec![SecurityRequirement {
				schemes: HashMap::from([("bearer-jwt".into(), StringList { list: vec![] })]),
			}],
			documentation_url: Some("https://docs.example.com/browser-agent".into()),
			icon_url: None,
			signatures: vec![],
		};

		let json = serde_json::to_string(&card).unwrap();
		let back: AgentCard = serde_json::from_str(&json).unwrap();
		assert_eq!(back, card);
	}

	// Deserialise an agent card from wire JSON as it would arrive
	// from another A2A implementation's /.well-known/agent.json
	// endpoint. This validates the inbound discovery path—if we
	// can't parse cards from other implementations, we can't
	// discover any agents.
	#[test]
	fn deserialises_from_external_agent_card() {
		let json = r#"{
            "name": "Remote Agent",
            "description": "A remote translation agent",
            "supportedInterfaces": [
                {"url": "https://remote-agent.io/rpc", "protocolBinding": "JSONRPC", "protocolVersion": "1.0"}
            ],
            "version": "1.0",
            "capabilities": {
                "streaming": true
            },
            "skills": [
                {
                    "id": "translate",
                    "name": "Translate",
                    "description": "Translates text between languages",
                    "tags": ["translation", "nlp"]
                }
            ],
            "defaultInputModes": ["text/plain"],
            "defaultOutputModes": ["text/plain"]
        }"#;

		let card: AgentCard = serde_json::from_str(json).unwrap();
		assert_eq!(card.name, "Remote Agent");
		assert_eq!(card.capabilities.streaming, Some(true));
		assert_eq!(card.supported_interfaces.len(), 1);
		assert_eq!(card.supported_interfaces[0].protocol_binding, "JSONRPC");

		// push_notifications was absent in the JSON, so it should
		// be None rather than failing to deserialise.
		assert!(card.capabilities.push_notifications.is_none());

		assert_eq!(card.skills.len(), 1);
		assert_eq!(card.skills[0].id, "translate");
	}

	// AgentCard::new creates a card from the required fields struct
	// and leaves all optional fields as None. The wire output should
	// contain exactly the required fields and nothing else.
	#[test]
	fn new_from_required_fields_matches_minimal_card() {
		let from_builder = AgentCard::new(AgentCardRequired {
			name: "Test Agent".into(),
			description: "A test agent for unit tests".into(),
			supported_interfaces: vec![AgentInterface::new(
				"https://agent.example.com/a2a",
				"JSONRPC",
				"1.0",
			)],
			version: "1.0".into(),
			capabilities: AgentCapabilities::default(),
			skills: vec![],
			default_input_modes: vec!["text/plain".into()],
			default_output_modes: vec!["text/plain".into()],
		});

		let from_literal = minimal_card();

		let builder_json = serde_json::to_value(&from_builder).unwrap();
		let literal_json = serde_json::to_value(&from_literal).unwrap();
		assert_eq!(builder_json, literal_json);
	}

	// AgentCard chainable setters populate the optional fields
	// independently. Each setter must set exactly one field without
	// disturbing the others.
	#[test]
	fn card_setters_populate_optional_fields() {
		let card = AgentCard::new(AgentCardRequired {
			name: "Settable Agent".into(),
			description: "Tests all optional setters".into(),
			supported_interfaces: vec![AgentInterface::new(
				"https://agent.example.com/a2a",
				"JSONRPC",
				"1.0",
			)],
			version: "1.0".into(),
			capabilities: AgentCapabilities::default(),
			skills: vec![],
			default_input_modes: vec!["text/plain".into()],
			default_output_modes: vec!["text/plain".into()],
		})
		.with_provider(AgentProvider {
			organization: "Arai".into(),
			url: "https://arai.dev".into(),
		})
		.with_documentation_url("https://docs.example.com")
		.with_icon_url("https://example.com/icon.png");

		assert_eq!(card.provider.as_ref().unwrap().organization, "Arai");
		assert_eq!(
			card.documentation_url.as_deref(),
			Some("https://docs.example.com")
		);
		assert_eq!(
			card.icon_url.as_deref(),
			Some("https://example.com/icon.png")
		);

		// Fields not set must remain empty.
		assert!(card.security_schemes.is_empty());
		assert!(card.security_requirements.is_empty());
		assert!(card.signatures.is_empty());
	}

	// AgentSkill::new creates a skill with the required fields and
	// all optional fields as None. This is the common construction
	// path for agents declaring their capabilities.
	#[test]
	fn skill_new_sets_required_fields_and_defaults() {
		let skill = AgentSkill::new(
			"web-search",
			"Web Search",
			"Searches the web and returns summarised results",
			vec!["search".into(), "web".into()],
		);

		assert_eq!(skill.id, "web-search");
		assert_eq!(skill.name, "Web Search");
		assert_eq!(
			skill.description,
			"Searches the web and returns summarised results"
		);
		assert_eq!(skill.tags, vec!["search", "web"]);
		assert!(skill.examples.is_empty());
		assert!(skill.input_modes.is_empty());
		assert!(skill.output_modes.is_empty());
		assert!(skill.security_requirements.is_empty());
	}

	// AgentSkill chainable setters populate each optional field
	// independently. A fully configured skill built through the
	// builder should round-trip through serde identically to
	// struct literal construction.
	#[test]
	fn skill_setters_populate_optional_fields() {
		let skill = AgentSkill::new(
			"document-analysis",
			"Document Analysis",
			"Analyses documents and extracts structured data",
			vec!["documents".into(), "analysis".into()],
		)
		.with_examples(vec![
			"analyse this PDF for compliance issues".into(),
			"extract contact information from this document".into(),
		])
		.with_input_modes(vec!["application/pdf".into(), "text/plain".into()])
		.with_output_modes(vec!["application/json".into()]);

		assert_eq!(skill.examples.len(), 2);
		assert_eq!(skill.input_modes, &["application/pdf", "text/plain"]);
		assert_eq!(skill.output_modes, &["application/json"]);
		assert!(skill.security_requirements.is_empty());

		// Round-trip to verify wire compatibility.
		let json = serde_json::to_string(&skill).unwrap();
		let back: AgentSkill = serde_json::from_str(&json).unwrap();
		assert_eq!(back, skill);
	}

	// AgentSkill builder produces the same wire format as struct
	// literal construction. This ensures the builder is a drop-in
	// replacement for manual construction without changing semantics.
	#[test]
	fn skill_builder_matches_struct_literal() {
		let from_builder =
			AgentSkill::new("echo", "Echo", "Echoes input back", vec!["echo".into()])
				.with_examples(vec!["say hello".into()]);

		let from_literal = AgentSkill {
			id: "echo".into(),
			name: "Echo".into(),
			description: "Echoes input back".into(),
			tags: vec!["echo".into()],
			examples: vec!["say hello".into()],
			input_modes: vec![],
			output_modes: vec![],
			security_requirements: vec![],
		};

		let builder_json = serde_json::to_value(&from_builder).unwrap();
		let literal_json = serde_json::to_value(&from_literal).unwrap();
		assert_eq!(builder_json, literal_json);
	}

	// with_security_requirements() on AgentSkill sets skill-level
	// security requirements. The field must round-trip through serde
	// with the correct camelCase key name.
	#[test]
	fn skill_with_security_requirements() {
		let req = SecurityRequirement::new(HashMap::from([(
			"bearer-jwt".into(),
			StringList::new(vec!["read".into()]),
		)]));
		let skill = AgentSkill::new("secure-op", "Secure Op", "Needs auth", vec![])
			.with_security_requirements(vec![req]);

		assert!(!skill.security_requirements.is_empty());
		let json = serde_json::to_value(&skill).unwrap();
		assert!(json["securityRequirements"].is_array());
		let first = &json["securityRequirements"][0];
		assert!(first["schemes"]["bearer-jwt"]["list"].is_array());
		assert_eq!(first["schemes"]["bearer-jwt"]["list"][0], "read");
	}

	// AgentProvider::new() takes the two required fields—organization
	// and URL. Both are mandatory per the spec; there are no optional
	// fields so no chainable setters are needed.
	#[test]
	fn provider_new_sets_required_fields() {
		let provider = AgentProvider::new("Arai", "https://arai.dev");
		assert_eq!(provider.organization, "Arai");
		assert_eq!(provider.url, "https://arai.dev");
	}

	// AgentProvider constructed via new() must produce the same wire
	// format as a struct literal. This ensures the constructor is a
	// drop-in replacement without changing serialisation behaviour.
	#[test]
	fn provider_new_matches_struct_literal() {
		let from_constructor = AgentProvider::new("Arai", "https://arai.dev");
		let from_literal = AgentProvider {
			organization: "Arai".into(),
			url: "https://arai.dev".into(),
		};
		assert_eq!(
			serde_json::to_value(&from_constructor).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// AgentCardSignature::new() takes the two required JWS fields—
	// the protected header and the signature, both Base64url-encoded.
	// The optional unprotected header defaults to None.
	#[test]
	fn signature_new_sets_required_fields() {
		let sig = AgentCardSignature::new("eyJhbGciOiJFZERTQSJ9", "abc123signature");
		assert_eq!(sig.protected, "eyJhbGciOiJFZERTQSJ9");
		assert_eq!(sig.signature, "abc123signature");
		assert!(sig.header.is_none());
	}

	// with_header() attaches the optional unprotected JWS header to
	// a signature. This carries metadata like the key ID that doesn't
	// need integrity protection.
	#[test]
	fn signature_with_header() {
		let sig = AgentCardSignature::new("eyJhbGciOiJFZERTQSJ9", "abc123signature")
			.with_header(serde_json::json!({"kid": "key-1"}));
		assert_eq!(sig.header.as_ref().unwrap()["kid"], "key-1");
	}

	// AgentCardSignature constructed via new() must match the struct
	// literal wire format. The optional header field must be absent
	// from JSON when None.
	#[test]
	fn signature_new_matches_struct_literal() {
		let from_constructor = AgentCardSignature::new("eyJhbGciOiJFZERTQSJ9", "sig");
		let from_literal = AgentCardSignature {
			protected: "eyJhbGciOiJFZERTQSJ9".into(),
			signature: "sig".into(),
			header: None,
		};
		assert_eq!(
			serde_json::to_value(&from_constructor).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// AgentCapabilities has all optional fields, so the constructor
	// pattern uses chainable setters on Default. with_streaming() and
	// with_push_notifications() each set one capability flag independently.
	// The removed with_state_transition_history() must not exist here.
	#[test]
	fn capabilities_with_streaming() {
		let caps = AgentCapabilities::default().with_streaming(true);
		assert_eq!(caps.streaming, Some(true));
		assert!(caps.push_notifications.is_none());
		assert!(caps.extended_agent_card.is_none());
		assert!(caps.extensions.is_empty());
	}

	// Multiple capability setters can be chained to build up the
	// full capability declaration in a single expression.
	// extended_agent_card replaces the old supports_authenticated_extended_card
	// field which was on AgentCard, not AgentCapabilities.
	#[test]
	fn capabilities_chained_setters() {
		let caps = AgentCapabilities::default()
			.with_streaming(true)
			.with_push_notifications(true)
			.with_extended_agent_card(true);
		assert_eq!(caps.streaming, Some(true));
		assert_eq!(caps.push_notifications, Some(true));
		assert_eq!(caps.extended_agent_card, Some(true));
	}

	// with_extensions() attaches protocol extensions to the
	// capabilities. Extensions let agents advertise non-standard
	// features beyond the core A2A protocol.
	// In v1.0, extensions is Vec not Option, but serialises as absent
	// when empty.
	#[test]
	fn capabilities_with_extensions() {
		let ext = AgentExtension::new("urn:example:custom-extension");
		let caps = AgentCapabilities::default().with_extensions(vec![ext]);
		assert_eq!(caps.extensions.len(), 1);
		assert_eq!(caps.extensions[0].uri, "urn:example:custom-extension");

		// Extensions must appear in the wire format when non-empty.
		let json = serde_json::to_value(&caps).unwrap();
		assert!(json["extensions"].is_array());
		assert_eq!(json["extensions"].as_array().unwrap().len(), 1);
	}

	// AgentCapabilities built via chainable setters must produce the
	// same wire format as the equivalent struct literal.
	#[test]
	fn capabilities_setters_match_struct_literal() {
		let from_setters = AgentCapabilities::default()
			.with_streaming(true)
			.with_push_notifications(false);
		let from_literal = AgentCapabilities {
			streaming: Some(true),
			push_notifications: Some(false),
			extended_agent_card: None,
			extensions: vec![],
		};
		assert_eq!(
			serde_json::to_value(&from_setters).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// AgentExtension::new() takes just the URI—the globally unique
	// identifier for the extension spec. Optional fields (description,
	// required, params) default to None.
	#[test]
	fn extension_new_sets_uri() {
		let ext = AgentExtension::new("urn:example:ext");
		assert_eq!(ext.uri, "urn:example:ext");
		assert!(ext.description.is_none());
		assert!(ext.required.is_none());
		assert!(ext.params.is_none());
	}

	// AgentExtension chainable setters populate optional fields
	// independently. with_description(), with_required(), and
	// with_params() each set one field without disturbing the others.
	#[test]
	fn extension_chained_setters() {
		let ext = AgentExtension::new("urn:example:ext")
			.with_description("Custom extension for testing")
			.with_required(true)
			.with_params(serde_json::json!({"version": "2.0"}));
		assert_eq!(
			ext.description.as_deref(),
			Some("Custom extension for testing")
		);
		assert_eq!(ext.required, Some(true));
		assert_eq!(ext.params.as_ref().unwrap()["version"], "2.0");
	}

	// AgentExtension constructed via new() must produce the same
	// wire format as a struct literal with all optional fields None.
	#[test]
	fn extension_new_matches_struct_literal() {
		let from_constructor = AgentExtension::new("urn:example:ext");
		let from_literal = AgentExtension {
			uri: "urn:example:ext".into(),
			description: None,
			required: None,
			params: None,
		};
		assert_eq!(
			serde_json::to_value(&from_constructor).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// SecurityScheme::api_key() creates an API key scheme with the
	// header/query parameter name and its location. Description
	// defaults to None.
	#[test]
	fn security_scheme_api_key_constructor() {
		let scheme = SecurityScheme::api_key("X-API-Key", "header");
		match &scheme {
			SecurityScheme::ApiKey {
				name,
				location,
				description,
			} => {
				assert_eq!(name, "X-API-Key");
				assert_eq!(location, "header");
				assert!(description.is_none());
			}
			_ => panic!("expected ApiKey variant"),
		}
	}

	// SecurityScheme::http() creates an HTTP auth scheme with the
	// scheme name (e.g. "bearer", "basic"). Bearer format and
	// description default to None.
	#[test]
	fn security_scheme_http_constructor() {
		let scheme = SecurityScheme::http("bearer");
		match &scheme {
			SecurityScheme::Http {
				scheme: http_scheme,
				bearer_format,
				description,
			} => {
				assert_eq!(http_scheme, "bearer");
				assert!(bearer_format.is_none());
				assert!(description.is_none());
			}
			_ => panic!("expected Http variant"),
		}
	}

	// SecurityScheme::http() with chainable with_bearer_format()
	// populates the bearer format hint for documentation purposes.
	#[test]
	fn security_scheme_http_with_bearer_format() {
		let scheme = SecurityScheme::http("bearer").with_bearer_format("JWT");
		match &scheme {
			SecurityScheme::Http { bearer_format, .. } => {
				assert_eq!(bearer_format.as_deref(), Some("JWT"));
			}
			_ => panic!("expected Http variant"),
		}
	}

	// SecurityScheme::oauth2() creates an OAuth2 scheme from the
	// flow definitions. Description and oauth2_metadata_url default
	// to None.
	#[test]
	fn security_scheme_oauth2_constructor() {
		let flows = OAuthFlows::default().with_client_credentials(ClientCredentialsOAuthFlow::new(
			"https://auth.example.com/token",
			HashMap::from([("read".into(), "read access".into())]),
		));
		let scheme = SecurityScheme::oauth2(flows);
		match &scheme {
			SecurityScheme::OAuth2 {
				flows,
				description,
				oauth2_metadata_url,
			} => {
				assert!(flows.client_credentials.is_some());
				assert!(description.is_none());
				assert!(oauth2_metadata_url.is_none());
			}
			_ => panic!("expected OAuth2 variant"),
		}
	}

	// SecurityScheme::openid_connect() creates an OpenID Connect
	// scheme from the discovery URL. Description defaults to None.
	#[test]
	fn security_scheme_openid_connect_constructor() {
		let scheme = SecurityScheme::openid_connect(
			"https://auth.example.com/.well-known/openid-configuration",
		);
		match &scheme {
			SecurityScheme::OpenIdConnect {
				open_id_connect_url,
				description,
			} => {
				assert_eq!(
					open_id_connect_url,
					"https://auth.example.com/.well-known/openid-configuration"
				);
				assert!(description.is_none());
			}
			_ => panic!("expected OpenIdConnect variant"),
		}
	}

	// SecurityScheme::mutual_tls() creates a mutual TLS scheme.
	// Description defaults to None.
	#[test]
	fn security_scheme_mutual_tls_constructor() {
		let scheme = SecurityScheme::mutual_tls();
		match &scheme {
			SecurityScheme::MutualTls { description } => {
				assert!(description.is_none());
			}
			_ => panic!("expected MutualTls variant"),
		}
	}

	// with_description() works on all SecurityScheme variants. This
	// test verifies it chains correctly with each constructor.
	#[test]
	fn security_scheme_with_description_all_variants() {
		let desc = "Test description";

		let scheme = SecurityScheme::api_key("key", "header").with_description(desc);
		match &scheme {
			SecurityScheme::ApiKey { description, .. } => {
				assert_eq!(description.as_deref(), Some(desc));
			}
			_ => panic!("expected ApiKey variant"),
		}

		let scheme = SecurityScheme::http("bearer").with_description(desc);
		match &scheme {
			SecurityScheme::Http { description, .. } => {
				assert_eq!(description.as_deref(), Some(desc));
			}
			_ => panic!("expected Http variant"),
		}

		let scheme = SecurityScheme::mutual_tls().with_description(desc);
		match &scheme {
			SecurityScheme::MutualTls { description } => {
				assert_eq!(description.as_deref(), Some(desc));
			}
			_ => panic!("expected MutualTls variant"),
		}
	}

	// SecurityScheme constructors must produce the same wire format
	// as struct literal construction. This verifies api_key() matches
	// the existing test's struct literal.
	#[test]
	fn security_scheme_constructors_match_struct_literals() {
		let from_constructor = SecurityScheme::api_key("X-API-Key", "header");
		let from_literal = SecurityScheme::ApiKey {
			name: "X-API-Key".into(),
			location: "header".into(),
			description: None,
		};
		assert_eq!(
			serde_json::to_value(&from_constructor).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);

		let from_constructor = SecurityScheme::http("bearer").with_bearer_format("JWT");
		let from_literal = SecurityScheme::Http {
			scheme: "bearer".into(),
			bearer_format: Some("JWT".into()),
			description: None,
		};
		assert_eq!(
			serde_json::to_value(&from_constructor).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// OAuthFlows::default() creates empty flows with all fields None.
	// Chainable setters populate individual flow types independently.
	#[test]
	fn oauth_flows_default_is_empty() {
		let flows = OAuthFlows::default();
		assert!(flows.authorization_code.is_none());
		assert!(flows.client_credentials.is_none());
		assert!(flows.device_code.is_none());
	}

	// OAuthFlows chainable setters populate each flow type. Multiple
	// flows can be configured on the same OAuthFlows instance.
	#[test]
	fn oauth_flows_chained_setters() {
		let flows = OAuthFlows::default()
			.with_client_credentials(ClientCredentialsOAuthFlow::new(
				"https://auth.example.com/token",
				HashMap::new(),
			))
			.with_authorization_code(AuthorizationCodeOAuthFlow::new(
				"https://auth.example.com/authorize",
				"https://auth.example.com/token",
				HashMap::new(),
			));
		assert!(flows.client_credentials.is_some());
		assert!(flows.authorization_code.is_some());
		assert!(flows.device_code.is_none());
	}

	// AuthorizationCodeOAuthFlow::new() takes the authorization URL,
	// token URL, and scopes—all three are required. The optional
	// refresh URL and pkce_required default to None.
	#[test]
	fn auth_code_flow_new_sets_required_fields() {
		let flow = AuthorizationCodeOAuthFlow::new(
			"https://auth.example.com/authorize",
			"https://auth.example.com/token",
			HashMap::from([("read".into(), "read access".into())]),
		);
		assert_eq!(flow.authorization_url, "https://auth.example.com/authorize");
		assert_eq!(flow.token_url, "https://auth.example.com/token");
		assert!(flow.refresh_url.is_none());
		assert!(flow.pkce_required.is_none());
		assert_eq!(flow.scopes["read"], "read access");
	}

	// with_refresh_url() attaches the optional token refresh endpoint
	// to an authorization code flow.
	#[test]
	fn auth_code_flow_with_refresh_url() {
		let flow = AuthorizationCodeOAuthFlow::new(
			"https://auth.example.com/authorize",
			"https://auth.example.com/token",
			HashMap::new(),
		)
		.with_refresh_url("https://auth.example.com/refresh");
		assert_eq!(
			flow.refresh_url.as_deref(),
			Some("https://auth.example.com/refresh")
		);
	}

	// with_pkce_required() marks the authorization code flow as
	// requiring PKCE (RFC 7636). Must appear in wire format when set.
	#[test]
	fn auth_code_flow_with_pkce_required() {
		let flow = AuthorizationCodeOAuthFlow::new(
			"https://auth.example.com/authorize",
			"https://auth.example.com/token",
			HashMap::new(),
		)
		.with_pkce_required(true);

		assert_eq!(flow.pkce_required, Some(true));

		let json = serde_json::to_value(&flow).unwrap();
		assert_eq!(json["pkceRequired"], true);

		// Verify it is absent when not set.
		let flow_no_pkce = AuthorizationCodeOAuthFlow::new(
			"https://auth.example.com/authorize",
			"https://auth.example.com/token",
			HashMap::new(),
		);
		let json2 = serde_json::to_value(&flow_no_pkce).unwrap();
		assert!(!json2.as_object().unwrap().contains_key("pkceRequired"));
	}

	// ClientCredentialsOAuthFlow::new() takes the token URL and
	// scopes. Refresh URL defaults to None. Scopes are now
	// HashMap<String, String> rather than serde_json::Value.
	#[test]
	fn client_credentials_flow_new_sets_required_fields() {
		let flow = ClientCredentialsOAuthFlow::new(
			"https://auth.example.com/token",
			HashMap::from([("admin".into(), "admin access".into())]),
		);
		assert_eq!(flow.token_url, "https://auth.example.com/token");
		assert_eq!(flow.scopes["admin"], "admin access");
		assert!(flow.refresh_url.is_none());
	}

	// DeviceCodeOAuthFlow::new() takes the device authorization URL,
	// token URL, and scopes. All three are required. Refresh URL
	// defaults to None.
	#[test]
	fn device_code_flow_new_sets_required_fields() {
		let flow = DeviceCodeOAuthFlow::new(
			"https://auth.example.com/device",
			"https://auth.example.com/token",
			HashMap::new(),
		);
		assert_eq!(
			flow.device_authorization_url,
			"https://auth.example.com/device"
		);
		assert_eq!(flow.token_url, "https://auth.example.com/token");
		assert!(flow.refresh_url.is_none());
	}

	// DeviceCodeOAuthFlow has with_refresh_url() like other flow types.
	// This was not present in the pre-v1.0 implementation.
	#[test]
	fn device_code_flow_with_refresh_url() {
		let flow = DeviceCodeOAuthFlow::new(
			"https://auth.example.com/device",
			"https://auth.example.com/token",
			HashMap::new(),
		)
		.with_refresh_url("https://auth.example.com/refresh");
		assert_eq!(
			flow.refresh_url.as_deref(),
			Some("https://auth.example.com/refresh")
		);

		let json = serde_json::to_value(&flow).unwrap();
		assert_eq!(json["refreshUrl"], "https://auth.example.com/refresh");
	}

	// All OAuth flow types with optional refresh_url support the
	// with_refresh_url() chainable setter. This test verifies the
	// pattern works consistently across all flow types that have it.
	#[test]
	fn oauth_flows_with_refresh_url() {
		let flow =
			ClientCredentialsOAuthFlow::new("https://auth.example.com/token", HashMap::new())
				.with_refresh_url("https://auth.example.com/refresh");
		assert_eq!(
			flow.refresh_url.as_deref(),
			Some("https://auth.example.com/refresh")
		);

		let flow = DeviceCodeOAuthFlow::new(
			"https://auth.example.com/device",
			"https://auth.example.com/token",
			HashMap::new(),
		)
		.with_refresh_url("https://auth.example.com/refresh");
		assert_eq!(
			flow.refresh_url.as_deref(),
			Some("https://auth.example.com/refresh")
		);
	}

	// SecurityRequirement and StringList must serialise to the proto3
	// JSON wire format: {"schemes": {"schemeName": {"list": [...]}}}
	// The nested structure is intentional—it matches the proto3
	// map<string, StringList> serialisation format.
	#[test]
	fn security_requirement_wire_format() {
		let req = SecurityRequirement::new(HashMap::from([
			(
				"oauth2".into(),
				StringList::new(vec!["read".into(), "write".into()]),
			),
			("api-key".into(), StringList::new(vec![])),
		]));

		let json = serde_json::to_value(&req).unwrap();
		assert!(json["schemes"]["oauth2"]["list"].is_array());
		assert_eq!(json["schemes"]["oauth2"]["list"][0], "read");
		assert_eq!(json["schemes"]["oauth2"]["list"][1], "write");
		assert_eq!(
			json["schemes"]["api-key"]["list"].as_array().unwrap().len(),
			0
		);
	}

	// SecurityRequirement and StringList must round-trip through serde
	// without data loss, confirming the struct layout matches the wire.
	#[test]
	fn security_requirement_round_trips() {
		let req = SecurityRequirement::new(HashMap::from([(
			"bearer-jwt".into(),
			StringList::new(vec!["tasks:read".into(), "tasks:write".into()]),
		)]));

		let json = serde_json::to_string(&req).unwrap();
		let back: SecurityRequirement = serde_json::from_str(&json).unwrap();
		assert_eq!(back, req);
	}

	// StringList::new() is a convenience constructor that wraps a Vec.
	// The resulting struct must match the equivalent literal construction.
	#[test]
	fn string_list_new_matches_literal() {
		let from_constructor = StringList::new(vec!["read".into(), "write".into()]);
		let from_literal = StringList {
			list: vec!["read".into(), "write".into()],
		};
		assert_eq!(from_constructor, from_literal);
	}

	// card-level security_requirements must serialise with the correct
	// camelCase key and must be absent when empty.
	#[test]
	fn card_security_requirements_wire_format() {
		let card = AgentCard::new(AgentCardRequired {
			name: "Secure Agent".into(),
			description: "Requires authentication for all operations".into(),
			supported_interfaces: vec![AgentInterface::new(
				"https://secure.example.com/a2a",
				"JSONRPC",
				"1.0",
			)],
			version: "1.0".into(),
			capabilities: AgentCapabilities::default(),
			skills: vec![],
			default_input_modes: vec!["text/plain".into()],
			default_output_modes: vec!["text/plain".into()],
		})
		.with_security_requirements(vec![SecurityRequirement::new(HashMap::from([(
			"bearer-jwt".into(),
			StringList::new(vec![]),
		)]))]);

		let json = serde_json::to_value(&card).unwrap();
		assert!(json["securityRequirements"].is_array());
		assert_eq!(json["securityRequirements"].as_array().unwrap().len(), 1);
		assert!(json["securityRequirements"][0]["schemes"]["bearer-jwt"]["list"].is_array());
	}

	// SecurityRequirement::from_scheme() is a convenience constructor that
	// builds the internal HashMap from a single scheme name and a slice of
	// scope string literals. It eliminates the boilerplate of constructing a
	// HashMap<String, StringList> by hand when only one scheme is needed.
	// The resulting requirement must contain the scheme name as a key and
	// the scope strings as the list value, exactly matching the wire format
	// produced by SecurityRequirement::new() with an equivalent HashMap.
	#[test]
	fn security_requirement_from_scheme_sets_name_and_scopes() {
		let requirement = SecurityRequirement::from_scheme("oauth2", &["read", "write"]);

		assert!(
			requirement.schemes.contains_key("oauth2"),
			"from_scheme must insert the given scheme name as a key"
		);
		assert_eq!(
			requirement.schemes["oauth2"].list,
			vec!["read".to_string(), "write".to_string()],
			"from_scheme must convert the scope slice to owned strings"
		);
	}

	// from_scheme() with an empty scope slice must produce a StringList
	// with an empty list—not omit the scheme entry. Many security schemes
	// use an empty scope list to mean "any valid credential suffices", so
	// the empty case must be explicitly representable on the wire.
	#[test]
	fn security_requirement_from_scheme_empty_scopes() {
		let requirement = SecurityRequirement::from_scheme("api-key", &[]);

		assert!(requirement.schemes.contains_key("api-key"));
		assert!(
			requirement.schemes["api-key"].list.is_empty(),
			"empty scope slice must produce an empty list, not be absent"
		);

		// The wire format must include the scheme with an empty list,
		// confirming skip_serializing_if does not suppress the entry.
		let json = serde_json::to_value(&requirement).unwrap();
		assert_eq!(
			json["schemes"]["api-key"]["list"].as_array().unwrap().len(),
			0
		);
	}

	// from_scheme() must produce identical wire output to the equivalent
	// SecurityRequirement::new() call. If they diverge, callers that mix
	// the two constructors will produce different JSON for the same intent.
	#[test]
	fn security_requirement_from_scheme_matches_new() {
		let from_scheme = SecurityRequirement::from_scheme("bearer-jwt", &["tasks:read"]);
		let from_new = SecurityRequirement::new(HashMap::from([(
			"bearer-jwt".into(),
			StringList::new(vec!["tasks:read".into()]),
		)]));

		assert_eq!(
			serde_json::to_value(&from_scheme).unwrap(),
			serde_json::to_value(&from_new).unwrap(),
			"from_scheme and new() must produce identical wire output"
		);
	}

	// Deserialise an ApiKey SecurityScheme from raw JSON as another A2A
	// implementation would produce it. The "type" discriminator drives
	// variant selection; "in" maps to the location field via serde rename.
	// This validates that external wire JSON round-trips into the correct
	// variant with the correct field values.
	#[test]
	fn deserialises_api_key_scheme_from_wire_json() {
		let json = r#"{"type":"apiKey","name":"X-API-Key","in":"header"}"#;
		let scheme: SecurityScheme = serde_json::from_str(json).unwrap();

		match scheme {
			SecurityScheme::ApiKey {
				name,
				location,
				description,
			} => {
				assert_eq!(name, "X-API-Key", "name must match the wire value");
				assert_eq!(
					location, "header",
					"in field must map to the location field"
				);
				assert!(
					description.is_none(),
					"absent description must deserialise as None"
				);
			}
			other => panic!("expected ApiKey variant, got {other:?}"),
		}
	}

	// Deserialise an OAuth2 SecurityScheme with an authorizationCode flow
	// from raw JSON as another A2A implementation would produce it. The
	// deeply nested flows object must deserialise correctly—authorizationUrl
	// and tokenUrl are required fields on the flow; scopes is a required map.
	// This validates the full inbound parsing path for OAuth2 schemes.
	#[test]
	fn deserialises_oauth2_scheme_from_wire_json() {
		let json = r#"{"type":"oauth2","flows":{"authorizationCode":{"authorizationUrl":"https://auth.example.com","tokenUrl":"https://token.example.com","scopes":{"read":"Read access"}}}}"#;
		let scheme: SecurityScheme = serde_json::from_str(json).unwrap();

		match scheme {
			SecurityScheme::OAuth2 {
				flows,
				description,
				oauth2_metadata_url,
			} => {
				assert!(
					description.is_none(),
					"absent description must deserialise as None"
				);
				assert!(
					oauth2_metadata_url.is_none(),
					"absent oauth2MetadataUrl must deserialise as None"
				);
				let auth_code_flow = flows
					.authorization_code
					.expect("authorizationCode flow must be present");
				assert_eq!(auth_code_flow.authorization_url, "https://auth.example.com");
				assert_eq!(auth_code_flow.token_url, "https://token.example.com");
				assert_eq!(auth_code_flow.scopes["read"], "Read access");
			}
			other => panic!("expected OAuth2 variant, got {other:?}"),
		}
	}

	// Deserialise a MutualTLS SecurityScheme from raw JSON as another A2A
	// implementation would produce it. The "type" value "mutualTLS" must
	// drive selection of the MutualTls variant; the optional description
	// field must deserialise correctly when present. This validates the
	// least common but still spec-required scheme type.
	#[test]
	fn deserialises_mutual_tls_scheme_from_wire_json() {
		let json = r#"{"type":"mutualTLS","description":"Client certificate required"}"#;
		let scheme: SecurityScheme = serde_json::from_str(json).unwrap();

		match scheme {
			SecurityScheme::MutualTls { description } => {
				assert_eq!(
					description.as_deref(),
					Some("Client certificate required"),
					"description must be deserialised from the wire value"
				);
			}
			other => panic!("expected MutualTls variant, got {other:?}"),
		}
	}
}
