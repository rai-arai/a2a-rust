// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Conversation messages in the A2A protocol.
//!
//! A message represents a single turn in a conversation between a
//! requester (the calling agent or human operator) and a responder
//! (the receiving agent). Messages carry one or more Parts as their
//! content, plus optional context that ties the message into a broader
//! conversation or task.
//!
//! The contextId groups related messages into a logical conversation
//! that may span multiple tasks. The taskId ties a message to a
//! specific task. These are both optional because the first message
//! in a conversation has neither—the agent assigns them.
//!
//! The referenceTaskIds field lets a message reference tasks from
//! prior conversations. This is how agents carry context forward:
//! "I'm following up on tasks X and Y we discussed earlier."
//!
//! Extensions is a list of extension URIs that apply to this message.
//! Core protocol processing ignores extensions it doesn't recognise.

use serde::{Deserialize, Serialize};

use crate::part::Part;
use crate::role::Role;

/// A single conversation turn between requester and responder.
///
/// Messages are the primary input and output of the protocol.
/// The `SendMessage` operation accepts a Message and returns either
/// a Task (if the agent creates or continues a task) or a Message
/// (if the agent responds directly without task tracking).
///
/// Required fields: `message_id`, role, parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
	/// Unique identifier for this message.
	/// Assigned by the sender. Required per the spec.
	pub message_id: String,

	/// Who sent this message—the requester ("user" in spec terms)
	/// or the responder ("agent"). Determines how the message is
	/// interpreted during task execution and streaming.
	pub role: Role,

	/// The content of this message, as one or more typed parts.
	/// A message must have at least one part. Multiple parts allow
	/// mixing content types (text explanation + data payload + file).
	pub parts: Vec<Part>,

	/// The conversation this message belongs to.
	/// Groups related messages across potentially multiple tasks.
	/// The first message in a conversation typically omits this —
	/// the agent assigns a contextId and returns it on the task.
	/// Subsequent messages in the same conversation include it to
	/// maintain continuity.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub context_id: Option<String>,

	/// The task this message is associated with.
	/// Present when continuing an existing task (e.g. providing
	/// additional input for an inputRequired task).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub task_id: Option<String>,

	/// References to tasks from prior conversations.
	/// Allows agents to carry context forward across conversation
	/// boundaries. For example, "continue the analysis from task X."
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub reference_task_ids: Vec<String>,

	/// Arbitrary key-value metadata attached to this message.
	/// Passed through by the protocol without interpretation.
	/// Useful for tracing, routing, or domain-specific annotations.
	/// Constrained to a JSON object—matching proto3 google.protobuf.Struct
	/// which never serialises as an array or scalar.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub metadata: Option<serde_json::Map<String, serde_json::Value>>,

	/// Protocol extension URIs attached to this message.
	/// Each entry identifies an extension by its URI. Unknown
	/// extensions are preserved but ignored during core protocol
	/// processing.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub extensions: Vec<String>,
}

impl Message {
	/// Create a new message with the given ID, role, and parts.
	///
	/// Initialises optional fields (`context_id`, `task_id`, metadata) to
	/// None and collection fields (`reference_task_ids`, extensions) to
	/// empty Vecs. Use the `with_*` chainable setters to populate them:
	///
	/// ```
	/// # use a2a::{Message, Role, Part};
	/// let message = Message::new("msg-1", Role::User, vec![
	///     Part::text("hello"),
	/// ])
	/// .with_context_id("ctx-abc")
	/// .with_metadata(
	///     serde_json::json!({"trace": "t-1"}).as_object().unwrap().clone()
	/// );
	/// ```
	#[must_use]
	pub fn new(message_id: impl Into<String>, role: Role, parts: Vec<Part>) -> Self {
		Self {
			message_id: message_id.into(),
			role,
			parts,
			context_id: None,
			task_id: None,
			reference_task_ids: Vec::new(),
			metadata: None,
			extensions: Vec::new(),
		}
	}

	/// Create a text message containing a single text part.
	///
	/// This is the most common construction path—a plain text
	/// exchange between requester and responder. Equivalent to
	/// calling `Message::new()` with a single text `Part` but
	/// without the boilerplate of constructing the part manually.
	///
	/// ```
	/// # use a2a::{Message, Role};
	/// let request = Message::text("msg-1", Role::User, "What is the weather?");
	/// let reply = Message::text("msg-2", Role::Agent, "Sunny, 23°C.");
	/// ```
	#[must_use]
	pub fn text(message_id: impl Into<String>, role: Role, text: impl Into<String>) -> Self {
		Self::new(message_id, role, vec![Part::text(text)])
	}

	/// Attach a conversation context ID to this message.
	///
	/// The context ID groups related messages across potentially
	/// multiple tasks into a logical conversation. Typically omitted
	/// on the first message—the agent assigns a context ID and
	/// returns it on the task. Subsequent messages in the same
	/// conversation include it to maintain continuity.
	#[must_use]
	pub fn with_context_id(mut self, context_id: impl Into<String>) -> Self {
		self.context_id = Some(context_id.into());
		self
	}

	/// Associate this message with an existing task.
	///
	/// Used when continuing an existing task—for example, providing
	/// additional input in response to an input-required status, or
	/// sending follow-up instructions to a working task.
	#[must_use]
	pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
		self.task_id = Some(task_id.into());
		self
	}

	/// Reference tasks from prior conversations.
	///
	/// Lets agents carry context forward across conversation
	/// boundaries. For example, "continue the analysis from task X"
	/// would include task X's ID here so the receiving agent can
	/// retrieve the prior context.
	#[must_use]
	pub fn with_reference_task_ids(mut self, ids: Vec<String>) -> Self {
		self.reference_task_ids = ids;
		self
	}

	/// Attach arbitrary metadata to this message.
	///
	/// Metadata is passed through by the protocol without
	/// interpretation. Common uses include tracing IDs, routing
	/// hints, and domain-specific annotations that don't fit into
	/// the message parts themselves. The parameter type is
	/// `serde_json::Map<String, serde_json::Value>` so the type
	/// system enforces the proto3 google.protobuf.Struct
	/// constraint—arrays and scalars cannot be passed at compile time.
	///
	/// ```
	/// # use a2a::{Message, Role};
	/// let message = Message::text("m-1", Role::User, "hi")
	///     .with_metadata(
	///         serde_json::json!({"trace": "t-1"}).as_object().unwrap().clone()
	///     );
	/// ```
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}

	/// Declare protocol extension URIs that apply to this message.
	///
	/// Each entry is a URI identifying an extension specification.
	/// Receiving agents preserve unknown extensions but ignore them
	/// during core protocol processing. Extensions allow the protocol
	/// to evolve without breaking existing implementations.
	#[must_use]
	pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
		self.extensions = extensions;
		self
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// A minimal message has the required fields: message_id, role,
	// and parts. All optional fields must be omitted from the wire
	// format to keep payloads compact.
	#[test]
	fn minimal_message_omits_optional_fields() {
		let message = Message {
			message_id: "msg-1".into(),
			role: Role::User,
			parts: vec![Part::text("hello agent")],
			context_id: None,
			task_id: None,
			reference_task_ids: vec![],
			metadata: None,
			extensions: vec![],
		};
		let json = serde_json::to_value(&message).unwrap();
		let object = json.as_object().unwrap();

		// Required fields must be present.
		assert_eq!(object["messageId"], "msg-1");
		assert_eq!(object["role"], "user");
		assert!(object["parts"].is_array());

		// The kind field is no longer part of Message—must be absent.
		assert!(!object.contains_key("kind"));

		// Optional fields must be absent.
		assert!(!object.contains_key("contextId"));
		assert!(!object.contains_key("taskId"));
		assert!(!object.contains_key("referenceTaskIds"));
		assert!(!object.contains_key("metadata"));
		assert!(!object.contains_key("extensions"));
	}

	// A fully populated message exercises every field. This tests
	// that all camelCase renames are correct and that complex nested
	// structures survive the round trip intact.
	#[test]
	fn full_message_round_trips() {
		let message = Message {
			message_id: "msg-001".into(),
			role: Role::Agent,
			parts: vec![
				Part::text("here is the analysis").with_metadata(
					serde_json::json!({"confidence": 0.95})
						.as_object()
						.unwrap()
						.clone(),
				),
			],
			context_id: Some("ctx-abc".into()),
			task_id: Some("task-123".into()),
			reference_task_ids: vec!["task-100".into(), "task-101".into()],
			metadata: Some(
				serde_json::json!({"trace_id": "t-xyz"})
					.as_object()
					.unwrap()
					.clone(),
			),
			extensions: vec!["ext:custom".into()],
		};

		let json = serde_json::to_string(&message).unwrap();
		let back: Message = serde_json::from_str(&json).unwrap();
		assert_eq!(back, message);
	}

	// Deserialise a message from wire JSON produced by an external
	// implementation. This tests the inbound path with the exact
	// field names the spec defines.
	#[test]
	fn deserialises_from_external_wire_json() {
		let json = r#"{
            "messageId": "ext-msg-1",
            "role": "user",
            "parts": [{"text": "what is the weather?"}],
            "contextId": "conv-99",
            "referenceTaskIds": ["old-task-1"]
        }"#;

		let message: Message = serde_json::from_str(json).unwrap();
		assert_eq!(message.message_id, "ext-msg-1");
		assert_eq!(message.role, Role::User);
		assert_eq!(message.parts.len(), 1);
		assert_eq!(message.context_id.unwrap(), "conv-99");
		assert_eq!(message.reference_task_ids, vec!["old-task-1"]);

		// Fields not present in the JSON should deserialise to defaults.
		assert!(message.task_id.is_none());
		assert!(message.metadata.is_none());
		assert!(message.extensions.is_empty());
	}

	// Message::new leaves all optional fields as None. This is the
	// primary construction path—verify it produces wire-compatible
	// output identical to struct literal construction.
	#[test]
	fn new_sets_required_fields_and_defaults() {
		let message = Message::new("message-new-1", Role::User, vec![Part::text("hello")]);

		assert_eq!(message.message_id, "message-new-1");
		assert_eq!(message.role, Role::User);
		assert_eq!(message.parts.len(), 1);
		assert!(message.context_id.is_none());
		assert!(message.task_id.is_none());
		assert!(message.reference_task_ids.is_empty());
		assert!(message.metadata.is_none());
		assert!(message.extensions.is_empty());
	}

	// Message::text is a shorthand for a single text part. Verify it
	// produces the same result as constructing the Part manually.
	#[test]
	fn text_creates_single_text_part() {
		let message = Message::text("message-text-1", Role::Agent, "hello world");

		assert_eq!(message.message_id, "message-text-1");
		assert_eq!(message.role, Role::Agent);
		assert_eq!(message.parts.len(), 1);
		let part = &message.parts[0];
		assert!(
			matches!(&part.content, crate::part::PartContent::Text(t) if t == "hello world"),
			"expected text content 'hello world'"
		);
		assert!(part.metadata.is_none());
	}

	// Chainable setters must each populate exactly one optional field
	// without disturbing the others. Building a fully populated message
	// through the builder should produce output identical to struct
	// literal construction.
	#[test]
	fn chainable_setters_populate_optional_fields() {
		let message = Message::text("message-chained-1", Role::User, "chained")
			.with_context_id("context-1")
			.with_task_id("task-1")
			.with_reference_task_ids(vec!["reference-1".into(), "reference-2".into()])
			.with_metadata(
				serde_json::json!({"key": "value"})
					.as_object()
					.unwrap()
					.clone(),
			)
			.with_extensions(vec!["ext:custom-extension".into()]);

		assert_eq!(message.context_id.as_deref(), Some("context-1"));
		assert_eq!(message.task_id.as_deref(), Some("task-1"));
		assert_eq!(
			message.reference_task_ids,
			vec!["reference-1".to_string(), "reference-2".to_string()]
		);
		assert_eq!(message.metadata.unwrap()["key"], "value");
		assert_eq!(message.extensions, vec!["ext:custom-extension".to_string()]);
	}

	// A message built with the builder must produce identical wire
	// output to one built with struct literals. This guards against
	// the builder accidentally setting defaults differently from the
	// struct default.
	#[test]
	fn builder_produces_same_wire_format_as_struct_literal() {
		let from_builder = Message::text("message-equivalence", Role::Agent, "same")
			.with_context_id("context-equivalence");

		let from_literal = Message {
			message_id: "message-equivalence".into(),
			role: Role::Agent,
			parts: vec![Part::text("same")],
			context_id: Some("context-equivalence".into()),
			task_id: None,
			reference_task_ids: vec![],
			metadata: None,
			extensions: vec![],
		};

		let builder_json = serde_json::to_value(&from_builder).unwrap();
		let literal_json = serde_json::to_value(&from_literal).unwrap();
		assert_eq!(builder_json, literal_json);
	}
}
