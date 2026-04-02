// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Agent discovery via the well-known agent card endpoint.
//
// The A2A protocol specifies that agents expose their capabilities
// at /.well-known/agent.json. This module fetches and parses that
// document, giving callers a typed AgentCard they can inspect to
// determine what operations the agent supports, which content types
// it accepts, and how to authenticate.
//
// Discovery is the first step in any A2A interaction—before sending
// messages, the caller should fetch the agent card to understand the
// agent's capabilities and avoid sending unsupported requests.

use a2a::agent_card::AgentCard;
use a2a::error::A2AError;

/// Fetch an agent's card from its well-known discovery endpoint.
///
/// Constructs the URL by appending `/.well-known/agent.json` to the
/// base URL and performs an HTTP GET. The base URL should be the
/// agent's root (e.g. `https://agent.example.com`) without a
/// trailing slash.
///
/// Returns the parsed `AgentCard` on success, or an `A2AError` if the
/// request fails or the response isn't valid JSON.
///
/// # Errors
///
/// Returns an [`A2AError`] if the HTTP request fails, the server returns a
/// non-success status code, or the response body cannot be parsed as an
/// [`AgentCard`].
pub async fn discover_agent(http: &reqwest::Client, base_url: &str) -> Result<AgentCard, A2AError> {
	// The well-known path /.well-known/agent.json is specified by the
	// A2A specification v1.0. If the spec changes this path in a future
	// revision, this line and the route in axum_integration/router.rs
	// must both be updated to match.
	let url = format!("{}/.well-known/agent.json", base_url.trim_end_matches('/'));

	let response = http.get(&url).send().await.map_err(|error| {
		let category = if error.is_timeout() {
			"agent card request timed out"
		} else if error.is_connect() {
			"failed to connect to agent"
		} else {
			"failed to fetch agent card"
		};
		A2AError::transport(category)
	})?;

	if !response.status().is_success() {
		return Err(A2AError::transport(format!(
			"agent card request failed with status {}",
			response.status()
		)));
	}

	let card: AgentCard = response
		.json()
		.await
		.map_err(|_| A2AError::transport("agent card response is not valid JSON"))?;

	Ok(card)
}
