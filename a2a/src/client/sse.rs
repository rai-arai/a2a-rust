// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// SSE (Server-Sent Events) stream parsing for A2A streaming operations.
//
// The A2A protocol uses SSE for streaming operations (SendStreamingMessage,
// SubscribeToTask). The agent sends a series of events, each
// containing a JSON-encoded StreamResponse. This module converts a raw
// reqwest response into a typed Stream<Item = Result<StreamResponse, A2AError>>.
//
// The conversion pipeline is:
//   reqwest::Response → bytes_stream() → eventsource_stream → parse JSON → StreamResponse
//
// eventsource-stream handles the SSE wire format (event:, data:, id:
// fields). We take its parsed Event objects and deserialise the data
// field as A2A StreamResponse JSON.

use std::pin::Pin;

use eventsource_stream::Eventsource;
use futures_core::Stream;
use futures_util::StreamExt;

use crate::error::A2AError;
use crate::stream_event::StreamResponse;

// The tests below exercise the exact code path that `into_event_stream`
// uses when it receives an SSE event. The map closure calls
// `serde_json::from_str::<StreamResponse>(&event.data)`, so testing
// that function with realistic data strings validates the behaviour
// of the stream parser without requiring a live HTTP connection.
//
// Note: `stream_event.rs` already covers wire-format serialisation and
// camelCase/snake_case alias behaviour at the type level. These tests
// focus on the *string parsing* path—raw SSE data frames arriving as
// `&str`—which is the exact contract `into_event_stream` relies on.

/// Convert a reqwest response into a stream of typed A2A streaming responses.
///
/// The response must be an SSE stream (Content-Type: text/event-stream).
/// Each SSE event's data field is parsed as a JSON `StreamResponse`.
/// Parse failures are emitted as `A2AError` items in the stream rather
/// than terminating it, so callers can decide how to handle malformed
/// events.
pub fn into_event_stream(
	response: reqwest::Response,
) -> Pin<Box<dyn Stream<Item = Result<StreamResponse, A2AError>> + Send>> {
	let event_stream = response.bytes_stream().eventsource();

	Box::pin(event_stream.map(|result| match result {
		Ok(event) => serde_json::from_str::<StreamResponse>(&event.data).map_err(|error| {
			A2AError::transport(format!("failed to parse SSE event data: {error}"))
		}),
		Err(error) => Err(A2AError::transport(format!("SSE stream error: {error}"))),
	}))
}

#[cfg(test)]
mod tests {
	use crate::stream_event::StreamResponse;
	use crate::task_state::TaskState;

	// Helper that mirrors the exact parsing expression used inside the
	// `into_event_stream` map closure:
	//
	//   serde_json::from_str::<StreamResponse>(&event.data)
	//
	// By calling this helper with the same string that an SSE data frame
	// would carry, each test below exercises the precise code path that
	// runs during live streaming—without needing a real HTTP connection.
	fn parse(data: &str) -> Result<StreamResponse, serde_json::Error> {
		serde_json::from_str::<StreamResponse>(data)
	}

	// A statusUpdate SSE data frame must deserialise into
	// StreamResponse::StatusUpdate and expose the inner event fields.
	// This is the most common event type during task processing—agents
	// emit it every time the task state changes (submitted → working →
	// completed). Asserting on task_id and status.state ensures the
	// camelCase field mapping (taskId, contextId, state) is correct
	// across the full parsing chain.
	#[test]
	fn parses_status_update_from_sse_data() {
		let data =
			r#"{"statusUpdate":{"taskId":"t1","contextId":"c1","status":{"state":"working"}}}"#;
		let event = parse(data).expect("valid statusUpdate frame must parse without error");
		match event {
			StreamResponse::StatusUpdate(update) => {
				assert_eq!(
					update.task_id, "t1",
					"task_id must match the taskId field on the wire"
				);
				assert_eq!(
					update.status.state,
					TaskState::Working,
					"state must deserialise from the working wire string"
				);
			}
			other => panic!("expected StatusUpdate variant, got {other:?}"),
		}
	}

	// A task SSE data frame—sent at the start of a stream to carry the
	// initial task snapshot—must deserialise into StreamResponse::Task.
	// Only the variant is asserted here because the Task type is tested
	// in depth within task.rs and stream_event.rs; this test focuses on
	// the outer discriminator key ("task") being recognised by the parser.
	#[test]
	fn parses_task_from_sse_data() {
		let data = r#"{"task":{"id":"t1","contextId":"c1","status":{"state":"submitted"}}}"#;
		let event = parse(data).expect("valid task frame must parse without error");
		assert!(
			matches!(event, StreamResponse::Task(_)),
			"task outer key must produce the Task variant"
		);
	}

	// A message SSE data frame—returned when an agent replies without
	// creating a persistent task—must deserialise into
	// StreamResponse::Message. The "message" discriminator key and the
	// inner Message schema must both be recognised.
	#[test]
	fn parses_message_from_sse_data() {
		let data = r#"{"message":{"messageId":"m1","role":"agent","parts":[{"text":"hi"}]}}"#;
		let event = parse(data).expect("valid message frame must parse without error");
		assert!(
			matches!(event, StreamResponse::Message(_)),
			"message outer key must produce the Message variant"
		);
	}

	// An artifactUpdate SSE data frame must deserialise into
	// StreamResponse::ArtifactUpdate. Artifact events carry nested
	// objects (artifact → parts) so this also validates that multi-level
	// camelCase field name mapping works through the full parse path.
	#[test]
	fn parses_artifact_update_from_sse_data() {
		let data = r#"{"artifactUpdate":{"taskId":"t1","contextId":"c1","artifact":{"artifactId":"a1","parts":[{"text":"done"}]}}}"#;
		let event = parse(data).expect("valid artifactUpdate frame must parse without error");
		assert!(
			matches!(event, StreamResponse::ArtifactUpdate(_)),
			"artifactUpdate outer key must produce the ArtifactUpdate variant"
		);
	}

	// The proto3 JSON specification allows either camelCase or
	// snake_case for field names. StreamResponse carries `#[serde(alias
	// = "status_update")]` to accept snake_case variant keys that some
	// server implementations emit. This test verifies that the alias is
	// recognised during the string-parsing step inside into_event_stream,
	// not just at the serde_json::Value level.
	#[test]
	fn parses_snake_case_variant_key() {
		let data =
			r#"{"status_update":{"taskId":"t1","contextId":"c1","status":{"state":"completed"}}}"#;
		let event = parse(data).expect("snake_case status_update alias must parse without error");
		assert!(
			matches!(event, StreamResponse::StatusUpdate(_)),
			"status_update alias must resolve to the StatusUpdate variant"
		);
	}

	// When an SSE data frame contains syntactically invalid JSON the
	// parse step must return an Err rather than panic. In the live
	// stream this Err is forwarded to the caller as an A2AError::Transport
	// item so the stream can continue rather than being terminated.
	// The test validates the Err path to ensure malformed frames are
	// handled gracefully.
	#[test]
	fn rejects_invalid_json() {
		let data = "this is not json";
		let result = parse(data);
		assert!(
			result.is_err(),
			"non-JSON SSE data must produce a deserialisation error, not a value"
		);
	}

	// An empty JSON object `{}` has no variant discriminator key.
	// Serde's externally-tagged enum representation requires exactly one
	// known key; an empty object must therefore fail. This protects
	// against keep-alive or heartbeat SSE events that happen to carry
	// an empty data frame being silently accepted as a valid event.
	#[test]
	fn rejects_empty_object() {
		let data = "{}";
		let result = parse(data);
		assert!(
			result.is_err(),
			"empty object has no variant key and must produce a deserialisation error"
		);
	}
}
