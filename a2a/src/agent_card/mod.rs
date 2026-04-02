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
//! agent. Skills describe what the agent can do, capabilities declare
//! protocol features it supports, interfaces list endpoint URLs, and
//! security schemes define how callers authenticate.

pub mod capabilities;
mod card;
pub mod security;

#[cfg(test)]
mod tests;

pub use capabilities::{AgentCapabilities, AgentExtension, AgentInterface, AgentSkill};
pub use card::{AgentCard, AgentCardRequired, AgentCardSignature, AgentProvider};
pub use security::{
	AuthorizationCodeOAuthFlow, ClientCredentialsOAuthFlow, DeviceCodeOAuthFlow, ImplicitOAuthFlow,
	OAuthFlows, PasswordOAuthFlow, SecurityRequirement, SecurityScheme, StringList,
};
