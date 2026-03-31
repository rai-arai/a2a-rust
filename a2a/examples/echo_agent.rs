// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A minimal echo agent that demonstrates the A2A crate's builder
//! patterns and axum integration.
//!
//! The agent echoes back whatever text the caller sends, wrapped in
//! a direct Message response (no task tracking). This is the simplest
//! possible A2A agent—it implements only `message_send` and lets
//! every other operation fall through to the default
//! UnsupportedOperation response.
//!
//! Run with:
//!   cargo run --example echo_agent --features axum
//!
//! Test with curl:
//!   curl -X POST http://localhost:3000 \
//!     -H "Content-Type: application/json" \
//!     -d '{
//!       "jsonrpc": "2.0",
//!       "method": "SendMessage",
//!       "id": "1",
//!       "params": {
//!         "message": {
//!           "messageId": "msg-1",
//!           "role": "user",
//!           "parts": [{"text": "Hello, agent!"}]
//!         }
//!       }
//!     }'
//!
//! Discover the agent card:
//!   curl http://localhost:3000/.well-known/agent.json

use a2a::{
	A2AError, A2AHandler, AgentCapabilities, AgentCard, AgentCardRequired, AgentInterface, Message,
	RequestContext, Role, SendMessageParams, SendMessageResult, a2a_router,
};

/// The echo agent handler.
///
/// Stateless—no fields, no shared state. Each request is handled
/// independently. For agents that need shared state (task stores,
/// database connections, external clients), add fields here and
/// pass them through via the constructor.
struct EchoAgent;

impl A2AHandler for EchoAgent {
	/// Echo the caller's message parts back as an agent reply.
	///
	/// Extracts the parts from the incoming message, wraps them in a
	/// new Message with the agent role, and returns it directly. No
	/// task is created—the caller receives an immediate response.
	///
	/// The response preserves the caller's context_id (if provided)
	/// so the conversation threading works correctly for callers that
	/// track conversations across multiple exchanges.
	fn message_send(
		&self,
		_context: &RequestContext,
		params: SendMessageParams,
	) -> impl std::future::Future<Output = Result<SendMessageResult, A2AError>> + Send + '_ {
		async move {
			let reply = Message::new("echo-reply", Role::Agent, params.message.parts)
				.with_context_id(
					params
						.message
						.context_id
						.unwrap_or_else(|| "echo-ctx".into()),
				);

			Ok(SendMessageResult::Message(reply))
		}
	}
}

#[tokio::main]
async fn main() {
	// Build the agent card that describes this agent's identity and
	// capabilities. Served at /.well-known/agent.json for discovery
	// by other agents and orchestrators.
	let card = AgentCard::new(AgentCardRequired {
		name: "Echo Agent".into(),
		description:
			"Echoes messages back to the caller—a minimal A2A agent for testing and development"
				.into(),
		supported_interfaces: vec![AgentInterface::new(
			"http://localhost:3000",
			"JSONRPC",
			"1.0",
		)],
		version: "1.0".into(),
		capabilities: AgentCapabilities::default(),
		skills: vec![],
		default_input_modes: vec!["text/plain".into()],
		default_output_modes: vec!["text/plain".into()],
	});

	// Build the axum router with the echo handler and agent card.
	// The router exposes POST / for JSON-RPC and GET /.well-known/agent.json
	// for agent discovery.
	let router = a2a_router(EchoAgent, card);

	let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
		.await
		.expect("failed to bind to port 3000");

	println!("Echo agent listening on http://localhost:3000");
	println!("Agent card at http://localhost:3000/.well-known/agent.json");

	axum::serve(listener, router)
		.await
		.expect("server terminated unexpectedly");
}
