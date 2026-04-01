// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A streaming agent that demonstrates SSE event delivery.
//!
//! Unlike the echo agent (which returns immediately), this agent
//! creates a task and emits progress updates as it "works". The
//! caller receives a stream of StreamResponse values carrying
//! TaskStatusUpdateEvents and a TaskArtifactUpdateEvent with the
//! final result.
//!
//! This pattern is typical for agents that perform long-running
//! operations—LLM inference, document processing, web scraping—where
//! the caller wants visibility into progress without polling.
//!
//! Run with:
//!   cargo run --example streaming_agent --features axum
//!
//! Test with curl (SSE stream):
//!   curl -N -X POST http://localhost:3001 \
//!     -H "Content-Type: application/json" \
//!     -d '{
//!       "jsonrpc": "2.0",
//!       "method": "SendStreamingMessage",
//!       "id": "1",
//!       "params": {
//!         "message": {
//!           "messageId": "msg-1",
//!           "role": "user",
//!           "parts": [{"text": "Count to three"}]
//!         }
//!       }
//!     }'

use a2a::{
	A2AError, A2AHandler, AgentCapabilities, AgentCard, AgentCardRequired, AgentInterface,
	Artifact, EventStream, Part, RequestContext, SendMessageParams, SendMessageResult,
	StreamResponse, Task, TaskArtifactUpdateEvent, TaskStatus, TaskStatusUpdateEvent, a2a_router,
};

/// A streaming agent that simulates progress updates.
///
/// Creates a task, emits working status updates, then completes
/// with an artifact. The delays between events simulate real work
/// like LLM token generation or document processing stages.
struct StreamingAgent;

impl A2AHandler for StreamingAgent {
	/// Handle synchronous message/send by creating a submitted task.
	///
	/// Callers that use message/send instead of message/stream still
	/// get a task handle they can poll for updates. The task starts
	/// in the submitted state—the agent hasn't begun work yet.
	fn message_send(
		&self,
		_context: &RequestContext,
		_params: SendMessageParams,
	) -> impl std::future::Future<Output = Result<SendMessageResult, A2AError>> + Send + '_ {
		async move {
			let task = Task::new("stream-task-1", "stream-context-1", TaskStatus::submitted());
			Ok(SendMessageResult::Task(task))
		}
	}

	/// Handle message/stream by emitting a sequence of status and
	/// artifact events that simulate task progress.
	///
	/// The stream emits:
	///   1. Working status (agent has started processing)
	///   2. Working status with progress message (midway update)
	///   3. Artifact update with the result
	///   4. Completed status marked as final
	fn message_stream(
		&self,
		_context: &RequestContext,
		_params: SendMessageParams,
	) -> impl std::future::Future<Output = Result<EventStream<'_>, A2AError>> + Send + '_ {
		async move {
			let task_id = "stream-task-1";
			let context_id = "stream-context-1";

			let events: Vec<Result<StreamResponse, A2AError>> = vec![
				Ok(StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
					task_id,
					context_id,
					TaskStatus::working(),
				))),
				Ok(StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
					task_id,
					context_id,
					TaskStatus::working().with_message(a2a::Message::text(
						"progress-1",
						a2a::Role::Agent,
						"Processing step 2 of 3...",
					)),
				))),
				Ok(StreamResponse::ArtifactUpdate(
					TaskArtifactUpdateEvent::new(
						task_id,
						context_id,
						Artifact::new("result-artifact-1", vec![Part::text("One, two, three.")])
							.with_name("counting result"),
					)
					.with_last_chunk(true),
				)),
				Ok(StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
					task_id,
					context_id,
					TaskStatus::completed(),
				))),
			];

			// In a production agent, replace stream::iter() with a tokio channel
			// or async generator for truly asynchronous event delivery. The
			// synchronous iterator used here is for demonstration only.
			Ok(Box::pin(futures_util::stream::iter(events)) as EventStream<'_>)
		}
	}
}

#[tokio::main]
async fn main() {
	// Get a TCP listener with a random port
	let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
		.await
		.expect("failed to bind to a port");
	let port = listener
		.local_addr()
		.expect("failed to get local address")
		.port();

	let card = AgentCard::new(AgentCardRequired {
		name: "Streaming Agent".into(),
		description: "Demonstrates SSE streaming with progress updates and artifact delivery"
			.into(),
		supported_interfaces: vec![AgentInterface::new(
			format!("http://localhost:{port}"),
			"JSONRPC",
			"1.0",
		)],
		version: "1.0".into(),
		capabilities: AgentCapabilities::default().with_streaming(true),
		skills: vec![],
		default_input_modes: vec!["text/plain".into()],
		default_output_modes: vec!["text/plain".into()],
	});

	let router = a2a_router(StreamingAgent, card);

	println!("Streaming agent listening on http://localhost:{port}");
	println!("Agent card at http://localhost:{port}/.well-known/agent.json");

	axum::serve(listener, router)
		.await
		.expect("server terminated unexpectedly");
}
