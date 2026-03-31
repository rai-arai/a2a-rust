// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The central coordination object in the A2A protocol.
//!
//! A task represents a unit of work being performed by an agent. When
//! a caller sends a message to an agent, the agent may create a task
//! to track the work. The task accumulates status updates, conversation
//! history, and output artifacts as the agent progresses.
//!
//! Tasks have a lifecycle governed by TaskState—they move from
//! submitted through working to a terminal state (completed, failed,
//! canceled, rejected). The inputRequired and authRequired states
//! represent pauses where the agent needs something from the caller
//! before it can continue.
//!
//! The contextId groups related tasks into a logical conversation.
//! Multiple tasks can share a contextId when they're part of the same
//! multi-step interaction. This lets agents maintain conversation
//! context across task boundaries.
//!
//! `TaskStatus` bundles the current state with an optional message (a
//! full Message object from the agent explaining the status) and an
//! optional RFC 3339 timestamp represented as `time::OffsetDateTime`.
//! Serialisation and deserialisation are handled by the internal
//! `serde_helpers::serde_rfc3339_option` module so the wire format
//! remains a plain quoted string compatible with all A2A implementations.

use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;
use crate::message::Message;
use crate::task_state::TaskState;

/// The current status of a task, including state and timing.
///
/// `TaskStatus` is a snapshot—it represents the task's state at
/// a specific point in time. During streaming, each status change
/// generates a `TaskStatusUpdateEvent` containing a new `TaskStatus`.
///
/// The message field is a full Message object (not a string) that
/// the agent can use to explain the status change. For example,
/// when transitioning to input-required, the agent might include
/// a message explaining what input is needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
	/// The current lifecycle state.
	pub state: TaskState,

	/// An optional message from the agent explaining this status.
	/// This is a full Message object, not a plain string. Agents
	/// use this to communicate progress or context beyond what
	/// the state enum conveys.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub message: Option<Message>,

	/// When this status was recorded, as an RFC 3339 timestamp.
	/// Typed as `time::OffsetDateTime` for correctness at the boundary—
	/// invalid timestamps are rejected at deserialisation time rather
	/// than discovered later when the caller tries to interpret them.
	/// Serialised as a quoted RFC 3339 string on the wire (e.g.
	/// `"2026-03-15T10:00:00Z"`) via the internal serde helper.
	/// Optional per the spec—some implementations may omit it.
	#[serde(
		default,
		with = "crate::serde_helpers::serde_rfc3339_option",
		skip_serializing_if = "Option::is_none"
	)]
	pub timestamp: Option<time::OffsetDateTime>,
}

/// A unit of work being performed by an agent.
///
/// Tasks are the main coordination object between callers and agents.
/// They accumulate state (status), conversation history (messages
/// exchanged during the task), and outputs (artifacts) as the agent
/// works.
///
/// The contextId groups related tasks into a logical conversation.
/// It is optional—the proto has no REQUIRED annotation on contextId.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
	/// Unique identifier for this task.
	/// Assigned by the agent when the task is created. Callers use
	/// this ID for subsequent operations (get, cancel, subscribe).
	pub id: String,

	/// The conversation this task belongs to.
	///
	/// Groups related tasks into a logical conversation. Modelled as a
	/// plain `String` to match proto3 semantics where a `string` field
	/// defaults to `""` and is never truly absent. This is consistent
	/// with `TaskStatusUpdateEvent.context_id` and
	/// `TaskArtifactUpdateEvent.context_id` which are also `String`.
	/// Omitted from the wire format when empty.
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub context_id: String,

	/// The current status of this task.
	pub status: TaskStatus,

	/// Output artifacts produced by the agent.
	/// Populated as the agent generates results. Each artifact
	/// has a unique ID within the task and contains one or more
	/// content parts. Omitted from the wire format when empty.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub artifacts: Vec<Artifact>,

	/// Conversation history associated with this task.
	/// Contains the messages exchanged between requester and agent
	/// during this task's lifecycle. Subject to historyLength limits
	/// on requests—may not contain the full conversation. Omitted
	/// from the wire format when empty.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub history: Vec<Message>,

	/// Arbitrary key-value metadata attached to this task.
	/// Passed through by the protocol without interpretation.
	/// Constrained to a JSON object—matching proto3 google.protobuf.Struct
	/// which never serialises as an array or scalar.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TaskStatus {
	/// Create a new task status with the given state.
	///
	/// Initialises the optional message and timestamp fields to
	/// None. Use `with_message()` and `with_timestamp()` to
	/// populate them when needed—for example, when transitioning
	/// to a failed state with a diagnostic message, or when
	/// recording precise timing for audit trails.
	///
	/// ```
	/// # use a2a::{TaskStatus, TaskState};
	/// # use time::macros::datetime;
	/// let status = TaskStatus::new(TaskState::Working)
	///     .with_timestamp(datetime!(2026-03-15 10:00:00 UTC));
	/// assert!(status.timestamp.is_some());
	/// ```
	#[must_use]
	pub fn new(state: TaskState) -> Self {
		Self {
			state,
			message: None,
			timestamp: None,
		}
	}

	/// Create a status representing a newly submitted task.
	///
	/// Submitted means the agent accepted the request but has not
	/// started working yet. Agents return this when they queue the
	/// request for later processing. Message and timestamp are
	/// left as None—use `with_timestamp()` to record when the
	/// submission was received.
	#[must_use]
	pub fn submitted() -> Self {
		Self::new(TaskState::Submitted)
	}

	/// Create a status representing a task actively being worked on.
	///
	/// Working means the agent is processing the request. Streaming
	/// connections will receive status and artifact updates while
	/// the task stays in this state. Message and timestamp are left
	/// as None—use `with_timestamp()` to record when work began.
	#[must_use]
	pub fn working() -> Self {
		Self::new(TaskState::Working)
	}

	/// Create a status representing a successfully completed task.
	///
	/// Completed is a terminal state—artifacts are available and
	/// the task will not change again. SSE streams close after
	/// emitting this status. Message and timestamp are left as
	/// None—use `with_timestamp()` to record completion time.
	#[must_use]
	pub fn completed() -> Self {
		Self::new(TaskState::Completed)
	}

	/// Create a status representing a task that encountered a
	/// fatal error, with a diagnostic message explaining what
	/// went wrong.
	///
	/// Failed is a terminal state—the agent gave up and will not
	/// retry. The message carries the failure reason as a full
	/// Message object so callers can display it or log it. This
	/// constructor requires a message because a bare "failed" status
	/// with no explanation is rarely useful in practice.
	#[must_use]
	pub fn failed(message: Message) -> Self {
		Self::new(TaskState::Failed).with_message(message)
	}

	/// Create a status representing a task canceled by the caller.
	///
	/// Canceled is a terminal state—the agent acknowledged the
	/// cancellation and stopped working. Message and timestamp are
	/// left as None—use `with_message()` to include a confirmation
	/// or `with_timestamp()` to record when cancellation occurred.
	#[must_use]
	pub fn canceled() -> Self {
		Self::new(TaskState::Canceled)
	}

	/// Create a status representing a task rejected by the agent.
	///
	/// Rejected is a terminal state—the agent decided not to
	/// process this request, possibly because the input falls
	/// outside its declared skills or capabilities. Message and
	/// timestamp are left as None—use `with_message()` to
	/// explain the rejection reason.
	#[must_use]
	pub fn rejected() -> Self {
		Self::new(TaskState::Rejected)
	}

	/// Create a status indicating the agent needs additional input
	/// from the caller before it can continue, with a message
	/// explaining what input is needed.
	///
	/// The caller should respond with another `SendMessage` referencing
	/// this task's ID and providing the requested information. This
	/// is the A2A equivalent of a "human in the loop" pause.
	/// The message is required because a bare "input-required" status
	/// with no explanation leaves the caller guessing what to provide.
	#[must_use]
	pub fn input_required(message: Message) -> Self {
		Self::new(TaskState::InputRequired).with_message(message)
	}

	/// Create a status indicating the task requires authentication
	/// before it can proceed, with a message explaining the
	/// authentication requirement.
	///
	/// The caller should authenticate using one of the agent's
	/// declared security schemes and retry. The message is required
	/// because the caller needs to know which authentication method
	/// to use or why their current credentials were insufficient.
	#[must_use]
	pub fn auth_required(message: Message) -> Self {
		Self::new(TaskState::AuthRequired).with_message(message)
	}

	/// Attach a message from the agent explaining this status.
	///
	/// The message is a full Message object (not a plain string)
	/// that the agent uses to communicate context beyond what the
	/// state enum conveys. For example, when transitioning to
	/// input-required, the message might explain what specific
	/// input is needed. When transitioning to failed, the message
	/// carries diagnostic information about the failure.
	#[must_use]
	pub fn with_message(mut self, message: Message) -> Self {
		self.message = Some(message);
		self
	}

	/// Record when this status was set, as an RFC 3339 timestamp.
	///
	/// Accepts any `time::OffsetDateTime`, including non-UTC offsets.
	/// The value is normalised to UTC before storing to match proto3
	/// `google.protobuf.Timestamp` semantics, which is always UTC.
	/// This ensures the serialised RFC 3339 string always ends with
	/// `Z` regardless of the caller's local offset, producing a
	/// consistent wire format across all agents.
	///
	/// ```
	/// # use a2a::{TaskStatus, TaskState};
	/// # use time::macros::datetime;
	/// let status = TaskStatus::new(TaskState::Working)
	///     .with_timestamp(datetime!(2026-03-15 10:00:00 UTC));
	/// assert!(status.timestamp.is_some());
	/// ```
	#[must_use]
	pub fn with_timestamp(mut self, timestamp: time::OffsetDateTime) -> Self {
		self.timestamp = Some(timestamp.to_offset(time::UtcOffset::UTC));
		self
	}
}

impl Task {
	/// Create a new task with the given ID, context ID, and status.
	///
	/// Pass an empty string if no context is available, or use
	/// `with_context_id()` to set it after construction. Initialises
	/// artifacts and history as empty vecs (omitted from the wire format
	/// when empty) and metadata as None. Use the `with_*` chainable
	/// setters to populate optional fields as needed.
	///
	/// ```
	/// # use a2a::{Task, TaskStatus, TaskState};
	/// let task = Task::new("task-1", "ctx-abc", TaskStatus::new(TaskState::Submitted))
	///     .with_metadata(
	///         serde_json::json!({"priority": "high"}).as_object().unwrap().clone()
	///     );
	/// ```
	#[must_use]
	pub fn new(id: impl Into<String>, context_id: impl Into<String>, status: TaskStatus) -> Self {
		Self {
			id: id.into(),
			context_id: context_id.into(),
			status,
			artifacts: Vec::new(),
			history: Vec::new(),
			metadata: None,
		}
	}

	/// Set the conversation context this task belongs to.
	///
	/// Used when the context ID is not known at construction time—for
	/// example when deserialising a task from the wire and then
	/// attaching it to a known conversation context, or when the
	/// agent assigns a context ID after initial submission.
	#[must_use]
	pub fn with_context_id(mut self, context_id: impl Into<String>) -> Self {
		self.context_id = context_id.into();
		self
	}

	/// Attach output artifacts produced by the agent.
	///
	/// Artifacts represent the tangible outputs of the task—rendered
	/// pages, generated files, computed results. Each artifact has a
	/// unique ID within the task and contains one or more content parts.
	/// Typically populated as the agent generates results during the
	/// working state.
	#[must_use]
	pub fn with_artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
		self.artifacts = artifacts;
		self
	}

	/// Include conversation history with this task.
	///
	/// Contains the messages exchanged between requester and agent
	/// during this task's lifecycle. The amount of history returned
	/// is subject to the historyLength limit set in the request's
	/// SendMessageConfiguration—callers control how much prior
	/// context they want back.
	#[must_use]
	pub fn with_history(mut self, history: Vec<Message>) -> Self {
		self.history = history;
		self
	}

	/// Attach arbitrary metadata to this task.
	///
	/// Metadata is passed through by the protocol without
	/// interpretation. Common uses include tracing IDs, priority
	/// annotations, billing context, or domain-specific state that
	/// doesn't fit into the protocol's structured fields. The parameter
	/// type is `serde_json::Map<String, serde_json::Value>` so the type
	/// system enforces the proto3 google.protobuf.Struct constraint—
	/// arrays and scalars cannot be passed at compile time.
	///
	/// ```
	/// # use a2a::{Task, TaskStatus, TaskState};
	/// let task = Task::new("t-1", "ctx-1", TaskStatus::submitted())
	///     .with_metadata(
	///         serde_json::json!({"priority": "high"}).as_object().unwrap().clone()
	///     );
	/// ```
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}
}

#[cfg(test)]
mod tests {
	use time::macros::datetime;

	use super::*;
	use crate::part::Part;
	use crate::role::Role;

	// A minimal task has id, context_id, and status. Empty artifacts
	// and history must be omitted from the wire format to keep
	// responses compact.
	#[test]
	fn minimal_task_omits_optional_fields() {
		let task = Task {
			id: "task-1".into(),
			context_id: "ctx-1".into(),
			status: TaskStatus {
				state: TaskState::Submitted,
				message: None,
				timestamp: Some(datetime!(2026-03-12 16:00:00 UTC)),
			},
			artifacts: vec![],
			history: vec![],
			metadata: None,
		};
		let json = serde_json::to_value(&task).unwrap();
		let object = json.as_object().unwrap();

		// The kind field is no longer part of Task—must be absent.
		assert!(!object.contains_key("kind"));
		assert_eq!(object["id"], "task-1");
		assert_eq!(object["contextId"], "ctx-1");
		assert_eq!(object["status"]["state"], "submitted");
		// RFC 3339 serialisation of UTC midnight: the time crate emits
		// +00:00 for an explicit UTC offset, not Z. Accept either form.
		let timestamp_wire = object["status"]["timestamp"].as_str().unwrap();
		assert!(
			timestamp_wire.starts_with("2026-03-12T16:00:00"),
			"unexpected timestamp format: {timestamp_wire}"
		);

		// Empty vecs and None metadata must be absent.
		assert!(!object.contains_key("artifacts"));
		assert!(!object.contains_key("history"));
		assert!(!object.contains_key("metadata"));
	}

	// A task in the failed state should include a status message
	// explaining the failure. This tests the error reporting path
	// that agents use when something goes wrong during execution.
	#[test]
	fn failed_task_with_status_message() {
		let task = Task {
			id: "task-err".into(),
			context_id: "ctx-1".into(),
			status: TaskStatus {
				state: TaskState::Failed,
				message: Some(Message {
					message_id: "status-msg-1".into(),
					role: Role::Agent,
					parts: vec![Part::text("could not reach upstream service")],
					context_id: None,
					task_id: None,
					reference_task_ids: vec![],
					metadata: None,
					extensions: vec![],
				}),
				timestamp: Some(datetime!(2026-03-12 16:10:00 UTC)),
			},
			artifacts: vec![],
			history: vec![],
			metadata: None,
		};

		let json = serde_json::to_string(&task).unwrap();
		let back: Task = serde_json::from_str(&json).unwrap();
		assert_eq!(back, task);
		assert_eq!(back.status.state, TaskState::Failed);
		assert!(back.status.message.is_some());
	}

	// A completed task with artifacts and history exercises the full
	// structure. This represents the typical end state of a successful
	// task—the agent has produced outputs and the conversation
	// history records how it got there.
	#[test]
	fn completed_task_with_artifacts_and_history() {
		let task = Task {
			id: "task-done".into(),
			context_id: "ctx-abc".into(),
			status: TaskStatus {
				state: TaskState::Completed,
				message: None,
				timestamp: Some(datetime!(2026-03-12 16:20:00 UTC)),
			},
			artifacts: vec![Artifact {
				artifact_id: "art-1".into(),
				name: Some("rendered page".into()),
				description: None,
				parts: vec![Part::text("<html>content</html>")],
				metadata: None,
				extensions: vec![],
			}],
			history: vec![
				Message {
					message_id: "msg-1".into(),
					role: Role::User,
					parts: vec![Part::text("render https://example.com")],
					context_id: None,
					task_id: None,
					reference_task_ids: vec![],
					metadata: None,
					extensions: vec![],
				},
				Message {
					message_id: "msg-2".into(),
					role: Role::Agent,
					parts: vec![Part::text("page rendered successfully")],
					context_id: None,
					task_id: None,
					reference_task_ids: vec![],
					metadata: None,
					extensions: vec![],
				},
			],
			metadata: None,
		};

		let json = serde_json::to_string(&task).unwrap();
		let back: Task = serde_json::from_str(&json).unwrap();
		assert_eq!(back, task);
		assert_eq!(back.artifacts.len(), 1);
		assert_eq!(back.history.len(), 2);
	}

	// TaskStatus round-trip with all fields populated. The timestamp
	// must survive the serde round-trip as an equal OffsetDateTime—
	// sub-second precision is preserved because time 0.3 retains
	// nanoseconds in its internal representation.
	#[test]
	fn task_status_round_trips() {
		// Construct with sub-second precision to verify it survives the
		// serialise-then-deserialise cycle without truncation. The time
		// crate's RFC 3339 formatter emits fractional seconds when the
		// nanosecond component is non-zero.
		let timestamp = datetime!(2026-03-12 16:05:30 UTC)
			.replace_nanosecond(123_000_000)
			.expect("123 ms is a valid nanosecond value");
		let status = TaskStatus {
			state: TaskState::Working,
			message: None,
			timestamp: Some(timestamp),
		};

		let json = serde_json::to_string(&status).unwrap();
		let back: TaskStatus = serde_json::from_str(&json).unwrap();
		assert_eq!(back, status);
		assert_eq!(back.timestamp.unwrap(), timestamp);
	}

	// TaskStatus with no timestamp—the spec says timestamp is
	// optional, so we must handle its absence gracefully.
	#[test]
	fn task_status_without_timestamp() {
		let json = r#"{"state": "working"}"#;
		let status: TaskStatus = serde_json::from_str(json).unwrap();
		assert_eq!(status.state, TaskState::Working);
		assert!(status.timestamp.is_none());
		assert!(status.message.is_none());
	}

	// Deserialise a task from wire JSON with the exact field names
	// the spec defines. Validates the inbound path for tasks
	// produced by other A2A implementations.
	#[test]
	fn deserialises_from_spec_wire_json() {
		let json = r#"{
            "id": "remote-task-1",
            "contextId": "remote-ctx",
            "status": {
                "state": "input-required",
                "timestamp": "2026-03-12T17:00:00Z"
            }
        }"#;

		let task: Task = serde_json::from_str(json).unwrap();
		assert_eq!(task.id, "remote-task-1");
		assert_eq!(task.context_id, "remote-ctx");
		assert_eq!(task.status.state, TaskState::InputRequired);
		assert!(task.artifacts.is_empty());
		assert!(task.history.is_empty());
	}

	// TaskStatus::new creates a status with only the state set.
	// Message and timestamp must be None until explicitly populated
	// via the chainable setters.
	#[test]
	fn task_status_new_sets_state_only() {
		let status = TaskStatus::new(TaskState::Working);

		assert_eq!(status.state, TaskState::Working);
		assert!(status.message.is_none());
		assert!(status.timestamp.is_none());
	}

	// The convenience constructors for stateless transitions (submitted,
	// working, completed, canceled, rejected) produce a status with the
	// correct state and no message or timestamp. These are the quick-fire
	// constructors agents use when they don't need to explain the transition.
	#[test]
	fn stateless_convenience_constructors() {
		let submitted = TaskStatus::submitted();
		assert_eq!(submitted.state, TaskState::Submitted);
		assert!(submitted.message.is_none());
		assert!(submitted.timestamp.is_none());

		let working = TaskStatus::working();
		assert_eq!(working.state, TaskState::Working);
		assert!(working.message.is_none());

		let completed = TaskStatus::completed();
		assert_eq!(completed.state, TaskState::Completed);
		assert!(completed.message.is_none());

		let canceled = TaskStatus::canceled();
		assert_eq!(canceled.state, TaskState::Canceled);
		assert!(canceled.message.is_none());

		let rejected = TaskStatus::rejected();
		assert_eq!(rejected.state, TaskState::Rejected);
		assert!(rejected.message.is_none());
	}

	// The convenience constructors for message-bearing transitions (failed,
	// input_required, auth_required) require a message explaining the
	// transition. The message must be preserved in the resulting status.
	#[test]
	fn message_bearing_convenience_constructors() {
		let failed = TaskStatus::failed(Message::text(
			"failure-message-1",
			Role::Agent,
			"upstream service timed out",
		));
		assert_eq!(failed.state, TaskState::Failed);
		assert_eq!(
			failed.message.as_ref().unwrap().message_id,
			"failure-message-1"
		);

		let input_required = TaskStatus::input_required(Message::text(
			"input-message-1",
			Role::Agent,
			"please provide your API key",
		));
		assert_eq!(input_required.state, TaskState::InputRequired);
		assert_eq!(
			input_required.message.as_ref().unwrap().message_id,
			"input-message-1"
		);

		let auth_required = TaskStatus::auth_required(Message::text(
			"auth-message-1",
			Role::Agent,
			"bearer token expired, please re-authenticate",
		));
		assert_eq!(auth_required.state, TaskState::AuthRequired);
		assert_eq!(
			auth_required.message.as_ref().unwrap().message_id,
			"auth-message-1"
		);
	}

	// Convenience constructors must chain with with_timestamp() to
	// produce the same wire format as manual construction. This
	// verifies that the builder pattern composes correctly when
	// starting from a convenience constructor.
	#[test]
	fn convenience_constructors_chain_with_timestamp() {
		let timestamp = datetime!(2026-03-16 10:00:00 UTC);
		let status = TaskStatus::completed().with_timestamp(timestamp);

		assert_eq!(status.state, TaskState::Completed);
		assert_eq!(status.timestamp, Some(timestamp));

		// Must produce the same wire output as manual construction.
		let manual = TaskStatus {
			state: TaskState::Completed,
			message: None,
			timestamp: Some(timestamp),
		};
		let status_json = serde_json::to_value(&status).unwrap();
		let manual_json = serde_json::to_value(&manual).unwrap();
		assert_eq!(status_json, manual_json);
	}

	// The spec marks state as REQUIRED on TaskStatus. A JSON object
	// missing the state field must be rejected at parse time rather
	// than deserialising with a default value, because a status
	// without a state is meaningless and would mask interop bugs
	// where a remote agent omits the field by mistake.
	#[test]
	fn task_status_rejects_missing_state() {
		let json = r#"{"timestamp": "2026-03-12T16:00:00Z"}"#;
		let result = serde_json::from_str::<TaskStatus>(json);
		assert!(
			result.is_err(),
			"TaskStatus without state must fail to deserialise"
		);
	}

	// The failed() convenience constructor chains with with_timestamp()
	// to produce a fully populated failure status. This is the most
	// common real-world usage—agent encounters an error, records
	// the failure message, and stamps the time.
	#[test]
	fn failed_with_timestamp_round_trips() {
		let status = TaskStatus::failed(Message::text(
			"failure-roundtrip-1",
			Role::Agent,
			"database connection lost",
		))
		.with_timestamp(datetime!(2026-03-16 11:30:00 UTC));

		assert_eq!(status.state, TaskState::Failed);
		assert!(status.message.is_some());
		assert!(status.timestamp.is_some());

		let json = serde_json::to_string(&status).unwrap();
		let back: TaskStatus = serde_json::from_str(&json).unwrap();
		assert_eq!(back, status);
	}

	// TaskStatus chainable setters populate the optional fields
	// independently. A status with both a message and a timestamp
	// must round-trip cleanly through serde.
	#[test]
	fn task_status_setters_populate_fields() {
		let timestamp = datetime!(2026-03-15 12:00:00 UTC);
		let status = TaskStatus::new(TaskState::Failed)
			.with_message(Message::text(
				"status-message-1",
				Role::Agent,
				"out of memory",
			))
			.with_timestamp(timestamp);

		assert_eq!(status.state, TaskState::Failed);
		assert_eq!(
			status.message.as_ref().unwrap().message_id,
			"status-message-1"
		);
		assert_eq!(status.timestamp, Some(timestamp));

		// Round-trip to verify wire compatibility.
		let json = serde_json::to_string(&status).unwrap();
		let back: TaskStatus = serde_json::from_str(&json).unwrap();
		assert_eq!(back, status);
	}

	// Task::new initialises artifacts and history as empty vecs and
	// leaves metadata as None. Verify the constructor produces
	// wire-compatible output.
	#[test]
	fn task_new_sets_required_fields_and_defaults() {
		let task = Task::new(
			"task-new-1",
			"context-new-1",
			TaskStatus::new(TaskState::Submitted),
		);

		assert_eq!(task.id, "task-new-1");
		assert_eq!(task.context_id, "context-new-1");
		assert_eq!(task.status.state, TaskState::Submitted);
		assert!(task.artifacts.is_empty());
		assert!(task.history.is_empty());
		assert!(task.metadata.is_none());
	}

	// Task chainable setters must populate each optional field
	// independently. A fully populated task built via the builder
	// should round-trip through serde identically to struct literal
	// construction.
	#[test]
	fn task_setters_populate_optional_fields() {
		let task = Task::new(
			"task-built-1",
			"context-built-1",
			TaskStatus::new(TaskState::Completed)
				.with_timestamp(datetime!(2026-03-15 13:00:00 UTC)),
		)
		.with_artifacts(vec![Artifact {
			artifact_id: "artifact-1".into(),
			name: Some("output".into()),
			description: None,
			parts: vec![Part::text("result")],
			metadata: None,
			extensions: vec![],
		}])
		.with_history(vec![Message::text(
			"history-message-1",
			Role::User,
			"do the thing",
		)])
		.with_metadata(
			serde_json::json!({"priority": "high"})
				.as_object()
				.unwrap()
				.clone(),
		);

		assert_eq!(task.artifacts.len(), 1);
		assert_eq!(task.history.len(), 1);
		assert_eq!(task.metadata.as_ref().unwrap()["priority"], "high");

		// Round-trip to verify wire compatibility.
		let json = serde_json::to_string(&task).unwrap();
		let back: Task = serde_json::from_str(&json).unwrap();
		assert_eq!(back, task);
	}
}
