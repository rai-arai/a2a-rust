// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::HashMap;

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
	let iface =
		AgentInterface::new("https://example.com/a2a", "JSONRPC", "1.0").with_tenant("acme-corp");

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
	let from_builder = AgentSkill::new("echo", "Echo", "Echoes input back", vec!["echo".into()])
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
	let sig = AgentCardSignature::new("eyJhbGciOiJFZERTQSJ9", "abc123signature").with_header(
		serde_json::json!({"kid": "key-1"})
			.as_object()
			.unwrap()
			.clone(),
	);
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
		.with_params(
			serde_json::json!({"version": "2.0"})
				.as_object()
				.unwrap()
				.clone(),
		);
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
	let scheme =
		SecurityScheme::openid_connect("https://auth.example.com/.well-known/openid-configuration");
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
	let flow = ClientCredentialsOAuthFlow::new("https://auth.example.com/token", HashMap::new())
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
