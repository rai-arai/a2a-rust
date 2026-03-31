// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Streaming responses for real-time task observation.
//!
//! When a caller uses `SendStreamingMessage` or `SubscribeToTask`, the server
//! returns a Server-Sent Events (SSE) stream of updates. Each SSE data
//! line contains a JSON-encoded StreamResponse—exactly one of:
//! task, message, statusUpdate, or artifactUpdate.
//!
//! This matches the A2A v1.0 proto3 oneof wire format. The JSON key
//! is the camelCase variant name:
//!
//!   {"task": {...}}
//!   {"message": {...}}
//!   {"statusUpdate": {...}}
//!   {"artifactUpdate": {...}}
//!
//! The stream starts with the current Task state, then emits status and
//! artifact updates as the agent works. Errors are returned as JSON-RPC
//! error responses on the transport layer—not as stream events.
//!
//! The event types (`TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent`)
//! carry task identity and payload but no kind discriminator field.
//! The discriminator is the `StreamResponse` wrapper key itself.

use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;

use crate::message::Message;
use crate::task::Task;
use crate::task::TaskStatus;

/// A status change event emitted during task streaming.
///
/// Sent whenever the task's state, message, or timing changes.
/// The `task_id` identifies which task changed—relevant when multiple
/// tasks share a streaming connection via context-level subscriptions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusUpdateEvent {
	/// The task whose status changed.
	pub task_id: String,

	/// The conversation context this task belongs to.
	pub context_id: String,

	/// The new status snapshot.
	pub status: TaskStatus,

	/// Arbitrary metadata attached to this event.
	/// Constrained to a JSON object—matching proto3 google.protobuf.Struct
	/// which never serialises as an array or scalar.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TaskStatusUpdateEvent {
	/// Create a new status update event for the given task and status.
	///
	/// Leaves metadata as None. Use `with_metadata()` to attach
	/// arbitrary event-level data for tracing or instrumentation.
	///
	/// ```
	/// # use a2a::{TaskStatusUpdateEvent, TaskStatus, TaskState};
	/// # use time::macros::datetime;
	/// let event = TaskStatusUpdateEvent::new(
	///     "task-1",
	///     "context-1",
	///     TaskStatus::working().with_timestamp(datetime!(2026-03-16 10:00:00 UTC)),
	/// );
	/// ```
	#[must_use]
	pub fn new(
		task_id: impl Into<String>,
		context_id: impl Into<String>,
		status: TaskStatus,
	) -> Self {
		Self {
			task_id: task_id.into(),
			context_id: context_id.into(),
			status,
			metadata: None,
		}
	}

	/// Attach arbitrary metadata to this event.
	///
	/// Event-level metadata is distinct from the task's own metadata.
	/// Common uses include tracing spans, server timestamps, or
	/// instrumentation data that applies to this specific event
	/// rather than the task as a whole. The parameter type is
	/// `serde_json::Map<String, serde_json::Value>` so the type
	/// system enforces the proto3 google.protobuf.Struct constraint—
	/// arrays and scalars cannot be passed at compile time.
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}
}

/// An artifact update event emitted during task streaming.
///
/// Sent whenever a new artifact is produced or an existing artifact
/// is updated. The artifact field contains the new or updated content.
///
/// The append flag indicates whether the artifact's parts should be
/// appended to the existing artifact (true) or replace it (false).
/// The `last_chunk` flag indicates this is the final chunk for this
/// artifact in a multi-chunk streaming scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactUpdateEvent {
	/// The task that produced or updated this artifact.
	pub task_id: String,

	/// The conversation context this task belongs to.
	pub context_id: String,

	/// The new or updated artifact.
	pub artifact: Artifact,

	/// Whether to append this artifact's parts to the existing
	/// artifact (true) or replace it entirely (false). Defaults
	/// to false when absent on the wire—the caller treats a missing
	/// field as a full replacement.
	#[serde(default, skip_serializing_if = "crate::serde_helpers::is_false")]
	pub append: bool,

	/// Whether this is the last chunk for this artifact in a
	/// multi-chunk streaming scenario. Defaults to false when
	/// absent on the wire.
	#[serde(default, skip_serializing_if = "crate::serde_helpers::is_false")]
	pub last_chunk: bool,

	/// Arbitrary metadata attached to this event.
	/// Constrained to a JSON object—matching proto3 google.protobuf.Struct
	/// which never serialises as an array or scalar.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TaskArtifactUpdateEvent {
	/// Create a new artifact update event for the given task and artifact.
	///
	/// Initialises append and `last_chunk` to false. Use `with_append()`
	/// and `with_last_chunk()` to control multi-chunk streaming behaviour,
	/// and `with_metadata()` to attach arbitrary event-level data.
	///
	/// ```
	/// # use a2a::{TaskArtifactUpdateEvent, Artifact, Part};
	/// let event = TaskArtifactUpdateEvent::new(
	///     "task-1",
	///     "context-1",
	///     Artifact::new("artifact-1", vec![
	///         Part::text("result"),
	///     ]),
	/// );
	/// ```
	#[must_use]
	pub fn new(
		task_id: impl Into<String>,
		context_id: impl Into<String>,
		artifact: Artifact,
	) -> Self {
		Self {
			task_id: task_id.into(),
			context_id: context_id.into(),
			artifact,
			append: false,
			last_chunk: false,
			metadata: None,
		}
	}

	/// Control whether the artifact's parts should be appended to
	/// an existing artifact or replace it entirely.
	///
	/// When true, the caller should append the new parts to the
	/// existing artifact with the same ID. When false, the new
	/// artifact replaces the previous version.
	#[must_use]
	pub fn with_append(mut self, append: bool) -> Self {
		self.append = append;
		self
	}

	/// Signal whether this is the final chunk for this artifact
	/// in a multi-chunk streaming scenario.
	///
	/// Agents producing large artifacts may split them across
	/// multiple events with append=true. The last event should set
	/// `last_chunk=true` so callers know the artifact is complete
	/// and can begin processing it.
	#[must_use]
	pub fn with_last_chunk(mut self, last_chunk: bool) -> Self {
		self.last_chunk = last_chunk;
		self
	}

	/// Attach arbitrary metadata to this event.
	///
	/// Event-level metadata is distinct from the artifact's own
	/// metadata. Common uses include chunk sequence numbers, content
	/// hashes, or instrumentation data that applies to this specific
	/// event rather than the artifact as a whole. The parameter type
	/// is `serde_json::Map<String, serde_json::Value>` so the type
	/// system enforces the proto3 google.protobuf.Struct constraint—
	/// arrays and scalars cannot be passed at compile time.
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}
}

/// A single streamed response on an SSE connection.
///
/// Each SSE data line carries exactly one `StreamResponse`. The outer
/// JSON key is the camelCase variant name—this is the A2A v1.0
/// proto3 oneof wire format. Callers match on the variant to route
/// status updates, artifact updates, task snapshots, and direct
/// message replies to appropriate handlers.
///
/// Proto3 canonical JSON uses camelCase variant names. Some older SDK
/// implementations emit `snake_case` field names (`status_update`,
/// `artifact_update`) instead, so aliases are provided for backward
/// compatibility. Serialisation always produces the canonical camelCase form.
///
/// Errors are not carried in-stream. Fatal errors that terminate the
/// connection are returned as JSON-RPC error responses at the transport
/// layer, outside the SSE event sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum StreamResponse {
	/// The current task state—sent at the start of a stream or as
	/// the complete response for a non-streaming request.
	///
	/// Wire format: `{"task": {...}}`
	Task(Task),

	/// A direct response without task tracking—sent when an agent
	/// replies to `SendStreamingMessage` without creating a task.
	///
	/// Wire format: `{"message": {...}}`
	Message(Message),

	/// The task's status has changed.
	///
	/// Wire format: `{"statusUpdate": {...}}` (also accepts `"status_update"`)
	#[serde(alias = "status_update")]
	StatusUpdate(TaskStatusUpdateEvent),

	/// A new artifact was produced or an existing one was updated.
	///
	/// Wire format: `{"artifactUpdate": {...}}` (also accepts `"artifact_update"`)
	#[serde(alias = "artifact_update")]
	ArtifactUpdate(TaskArtifactUpdateEvent),
}

#[cfg(test)]
mod tests {
	use time::macros::datetime;

	use super::*;
	use crate::part::Part;
	use crate::role::Role;
	use crate::task::TaskStatus;
	use crate::task_state::TaskState;

	// StatusUpdate events serialise as an externally-tagged JSON
	// object with the key "statusUpdate". The inner object contains
	// taskId, contextId, and status—no kind field, no final field.
	// This is the A2A v1.0 wire format.
	#[test]
	fn status_update_event_wire_format() {
		let response = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
			task_id: "task-1".into(),
			context_id: "ctx-1".into(),
			status: TaskStatus {
				state: TaskState::Working,
				message: None,
				timestamp: Some(datetime!(2026-03-12 16:30:00 UTC)),
			},
			metadata: None,
		});

		let json = serde_json::to_value(&response).unwrap();

		// Outer key must be "statusUpdate"—the externally-tagged wrapper.
		assert!(
			json.as_object().unwrap().contains_key("statusUpdate"),
			"outer key must be statusUpdate"
		);
		assert_eq!(json["statusUpdate"]["taskId"], "task-1");
		assert_eq!(json["statusUpdate"]["contextId"], "ctx-1");
		assert_eq!(json["statusUpdate"]["status"]["state"], "working");

		// No kind field or final field in the v1.0 format.
		assert!(
			!json["statusUpdate"]
				.as_object()
				.unwrap()
				.contains_key("kind"),
			"kind field must be absent"
		);
		assert!(
			!json["statusUpdate"]
				.as_object()
				.unwrap()
				.contains_key("final"),
			"final field must be absent"
		);
	}

	// ArtifactUpdate events serialise as {"artifactUpdate": {...}}
	// with taskId, contextId, artifact, and optional streaming flags.
	// No kind discriminator field.
	#[test]
	fn artifact_update_event_wire_format() {
		let response = StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
			task_id: "task-1".into(),
			context_id: "ctx-1".into(),
			artifact: crate::artifact::Artifact {
				artifact_id: "art-1".into(),
				name: Some("result".into()),
				description: None,
				parts: vec![Part::data(serde_json::json!({"answer": 42}))],
				metadata: None,
				extensions: vec![],
			},
			append: false,
			last_chunk: true,
			metadata: None,
		});

		let json = serde_json::to_value(&response).unwrap();

		assert!(
			json.as_object().unwrap().contains_key("artifactUpdate"),
			"outer key must be artifactUpdate"
		);
		assert_eq!(json["artifactUpdate"]["taskId"], "task-1");
		assert_eq!(json["artifactUpdate"]["artifact"]["artifactId"], "art-1");
		// append is false—proto3 canonical form omits it from the wire.
		assert!(
			!json["artifactUpdate"]
				.as_object()
				.unwrap()
				.contains_key("append"),
			"append must be absent when false"
		);
		assert_eq!(json["artifactUpdate"]["lastChunk"], true);

		assert!(
			!json["artifactUpdate"]
				.as_object()
				.unwrap()
				.contains_key("kind"),
			"kind field must be absent"
		);
	}

	// Task events serialise as {"task": {...}}—the full Task object
	// nested under the "task" key. This is the first event on a stream,
	// carrying the initial task snapshot.
	#[test]
	fn task_event_wire_format() {
		let response = StreamResponse::Task(Task {
			id: "task-new".into(),
			context_id: "ctx-1".into(),
			status: TaskStatus {
				state: TaskState::Submitted,
				message: None,
				timestamp: Some(datetime!(2026-03-12 16:00:00 UTC)),
			},
			artifacts: vec![],
			history: vec![],
			metadata: None,
		});

		let json = serde_json::to_value(&response).unwrap();

		assert!(
			json.as_object().unwrap().contains_key("task"),
			"outer key must be task"
		);
		assert_eq!(json["task"]["id"], "task-new");
		assert_eq!(json["task"]["status"]["state"], "submitted");
	}

	// Message events serialise as {"message": {...}}—the full Message
	// object nested under the "message" key. Returned when an agent
	// replies directly without creating a task.
	#[test]
	fn message_event_wire_format() {
		let response = StreamResponse::Message(Message {
			message_id: "msg-1".into(),
			role: Role::Agent,
			parts: vec![Part::text("I can help with that")],
			context_id: None,
			task_id: None,
			reference_task_ids: vec![],
			metadata: None,
			extensions: vec![],
		});

		let json = serde_json::to_value(&response).unwrap();

		assert!(
			json.as_object().unwrap().contains_key("message"),
			"outer key must be message"
		);
		assert_eq!(json["message"]["role"], "agent");
		assert_eq!(json["message"]["messageId"], "msg-1");
	}

	// TaskStatusUpdateEvent::new creates an event with only the core
	// fields set—no metadata. The common path for agents emitting
	// interim status updates during task processing.
	#[test]
	fn status_update_event_new_sets_defaults() {
		let event = TaskStatusUpdateEvent::new(
			"task-status-new-1",
			"context-status-new-1",
			TaskStatus::working(),
		);

		assert_eq!(event.task_id, "task-status-new-1");
		assert_eq!(event.context_id, "context-status-new-1");
		assert_eq!(event.status.state, TaskState::Working);
		assert!(event.metadata.is_none());
	}

	// TaskStatusUpdateEvent chainable setter populates metadata
	// independently. A completed status with metadata is the standard
	// pattern for agents that attach tracing data to the final update.
	#[test]
	fn status_update_event_setters_populate_fields() {
		let event = TaskStatusUpdateEvent::new(
			"task-status-final-1",
			"context-status-final-1",
			TaskStatus::completed().with_timestamp(datetime!(2026-03-16 12:00:00 UTC)),
		)
		.with_metadata(
			serde_json::json!({"trace_id": "trace-abc"})
				.as_object()
				.unwrap()
				.clone(),
		);

		assert_eq!(event.metadata.as_ref().unwrap()["trace_id"], "trace-abc");

		// Round-trip to verify wire compatibility.
		let json = serde_json::to_string(&event).unwrap();
		let back: TaskStatusUpdateEvent = serde_json::from_str(&json).unwrap();
		assert_eq!(back, event);
	}

	// TaskArtifactUpdateEvent::new creates an event with append and
	// last_chunk both false—the safe default for single-chunk artifacts
	// that replace rather than extend the existing content. Callers must
	// explicitly opt into append or chunked streaming via the setters.
	#[test]
	fn artifact_update_event_new_sets_defaults() {
		let event = TaskArtifactUpdateEvent::new(
			"task-artifact-new-1",
			"context-artifact-new-1",
			crate::artifact::Artifact::new("artifact-event-1", vec![Part::text("output")]),
		);

		assert_eq!(event.task_id, "task-artifact-new-1");
		assert_eq!(event.context_id, "context-artifact-new-1");
		assert_eq!(event.artifact.artifact_id, "artifact-event-1");
		assert!(!event.append);
		assert!(!event.last_chunk);
		assert!(event.metadata.is_none());
	}

	// TaskArtifactUpdateEvent chainable setters must populate the
	// streaming control flags independently. A multi-chunk streaming
	// scenario uses append=true with last_chunk=true on the final event.
	#[test]
	fn artifact_update_event_setters_populate_fields() {
		let event = TaskArtifactUpdateEvent::new(
			"task-artifact-chunked-1",
			"context-artifact-chunked-1",
			crate::artifact::Artifact::new("artifact-chunked-1", vec![Part::text("final chunk")]),
		)
		.with_append(true)
		.with_last_chunk(true)
		.with_metadata(
			serde_json::json!({"chunk_sequence": 3})
				.as_object()
				.unwrap()
				.clone(),
		);

		assert!(event.append);
		assert!(event.last_chunk);
		assert_eq!(event.metadata.as_ref().unwrap()["chunk_sequence"], 3);

		// Round-trip to verify wire compatibility.
		let json = serde_json::to_string(&event).unwrap();
		let back: TaskArtifactUpdateEvent = serde_json::from_str(&json).unwrap();
		assert_eq!(back, event);
	}

	// All four StreamResponse variants must round-trip cleanly through
	// serde. This is critical for proxy and relay implementations that
	// deserialise events from one agent and re-serialise them for another.
	// Each variant serialises with the correct externally-tagged key and
	// deserialises back to the same value.
	#[test]
	fn all_variants_round_trip() {
		let responses = vec![
			StreamResponse::Task(Task {
				id: "task-roundtrip-1".into(),
				context_id: "context-roundtrip-1".into(),
				status: TaskStatus {
					state: TaskState::Submitted,
					message: None,
					timestamp: Some(datetime!(2026-03-12 16:00:00 UTC)),
				},
				artifacts: vec![],
				history: vec![],
				metadata: None,
			}),
			StreamResponse::Message(Message {
				message_id: "msg-roundtrip-1".into(),
				role: Role::Agent,
				parts: vec![Part::text("hello")],
				context_id: None,
				task_id: None,
				reference_task_ids: vec![],
				metadata: None,
				extensions: vec![],
			}),
			StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
				task_id: "task-roundtrip-2".into(),
				context_id: "context-roundtrip-2".into(),
				status: TaskStatus {
					state: TaskState::Completed,
					message: None,
					timestamp: Some(datetime!(2026-03-12 17:00:00 UTC)),
				},
				metadata: None,
			}),
			StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
				task_id: "task-roundtrip-3".into(),
				context_id: "context-roundtrip-3".into(),
				artifact: crate::artifact::Artifact {
					artifact_id: "artifact-roundtrip-1".into(),
					name: None,
					description: None,
					parts: vec![Part::text("done")],
					metadata: None,
					extensions: vec![],
				},
				append: false,
				last_chunk: false,
				metadata: None,
			}),
		];

		for response in responses {
			let json = serde_json::to_string(&response).unwrap();
			let back: StreamResponse = serde_json::from_str(&json).unwrap();
			assert_eq!(back, response);
		}
	}

	// Deserialisation from the v1.0 wire format must produce the
	// correct variant for each of the four externally-tagged keys.
	// This tests that the JSON key drives variant selection.
	#[test]
	fn all_variants_deserialise_from_wire() {
		let task_json = serde_json::json!({
			"task": {
				"id": "t1",
				"contextId": "c1",
				"status": {"state": "submitted"}
			}
		});
		let msg_json = serde_json::json!({
			"message": {
				"messageId": "m1",
				"role": "agent",
				"parts": [{"text": "hi"}]
			}
		});
		let status_json = serde_json::json!({
			"statusUpdate": {
				"taskId": "t1",
				"contextId": "c1",
				"status": {"state": "working"}
			}
		});
		let artifact_json = serde_json::json!({
			"artifactUpdate": {
				"taskId": "t1",
				"contextId": "c1",
				"artifact": {
					"artifactId": "a1",
					"parts": [{"text": "done"}]
				}
			}
		});

		let task: StreamResponse = serde_json::from_value(task_json).unwrap();
		assert!(matches!(task, StreamResponse::Task(_)));

		let msg: StreamResponse = serde_json::from_value(msg_json).unwrap();
		assert!(matches!(msg, StreamResponse::Message(_)));

		let status: StreamResponse = serde_json::from_value(status_json).unwrap();
		assert!(matches!(status, StreamResponse::StatusUpdate(_)));

		let artifact: StreamResponse = serde_json::from_value(artifact_json).unwrap();
		assert!(matches!(artifact, StreamResponse::ArtifactUpdate(_)));
	}

	// Proto3 canonical JSON uses camelCase variant names, but some older
	// SDK implementations emit snake_case keys (status_update,
	// artifact_update). The serde aliases on StatusUpdate and ArtifactUpdate
	// accept these snake_case forms on deserialisation for backward
	// compatibility, while serialisation always produces canonical camelCase.
	#[test]
	fn snake_case_aliases_deserialise_correctly() {
		let status_snake = serde_json::json!({
			"status_update": {
				"taskId": "t1",
				"contextId": "c1",
				"status": {"state": "working"}
			}
		});
		let artifact_snake = serde_json::json!({
			"artifact_update": {
				"taskId": "t1",
				"contextId": "c1",
				"artifact": {
					"artifactId": "a1",
					"parts": [{"text": "done"}]
				}
			}
		});

		let status: StreamResponse = serde_json::from_value(status_snake).unwrap();
		assert!(
			matches!(status, StreamResponse::StatusUpdate(_)),
			"status_update alias must deserialise to StatusUpdate variant"
		);

		let artifact: StreamResponse = serde_json::from_value(artifact_snake).unwrap();
		assert!(
			matches!(artifact, StreamResponse::ArtifactUpdate(_)),
			"artifact_update alias must deserialise to ArtifactUpdate variant"
		);

		// Serialisation always uses camelCase regardless of how the value
		// was deserialised.
		let json = serde_json::to_value(&status).unwrap();
		assert!(
			json.as_object().unwrap().contains_key("statusUpdate"),
			"serialised output must use camelCase statusUpdate key"
		);
	}
}
