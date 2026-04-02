// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
