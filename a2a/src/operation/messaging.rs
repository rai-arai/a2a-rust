// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Messaging operation types for the A2A protocol.
//!
//! The messaging operations are the primary way callers interact with
//! agents. A caller sends a Message (containing Parts—text, files,
//! structured data) and receives either a Task (for deferred work) or
//! a Message (for immediate replies).
//!
//! `SendMessage` is the synchronous variant—the caller blocks until
//! the agent has a response. `SendStreamingMessage` uses SSE to push
//! interim status and artifact updates before the final result.
//!
//! The `SendMessageConfiguration` controls how the agent should process
//! the request: which output formats are acceptable, whether to block
//! until completion, how much conversation history to include, and
//! where to push notifications if the caller wants webhooks.

use serde::{Deserialize, Serialize};

use crate::message::Message;

use crate::task::Task;

use super::push_notification::TaskPushNotificationConfig;

/// Parameters for the `SendMessage` and `SendStreamingMessage` operations.
///
/// This is what goes in the "params" field of the JSON-RPC request
/// envelope. The message field carries the caller's input; everything
/// else is optional configuration for how the agent should handle it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageParams {
	/// The message to send to the agent.
	/// Contains the caller's input as one or more Parts (text, file,
	/// or structured data).
	pub message: Message,

	/// Optional configuration controlling agent behaviour for this
	/// request—output formats, blocking, history, push notifications.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub configuration: Option<SendMessageConfiguration>,

	/// Optional caller-defined metadata.
	/// The spec doesn't prescribe the structure—callers use this
	/// for tracing IDs, session context, or other pass-through data
	/// that the agent may echo back.
	/// Constrained to a JSON object—matching proto3 google.protobuf.Struct
	/// which never serialises as an array or scalar.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub metadata: Option<serde_json::Map<String, serde_json::Value>>,

	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,
}

/// Configuration for a `SendMessage` or `SendStreamingMessage` request.
///
/// Every field is optional—when omitted the agent uses its own
/// defaults. This lets callers progressively opt into more specific
/// control without requiring configuration for simple interactions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfiguration {
	/// MIME types the caller accepts for output.
	/// If the agent cannot produce any of these types it returns
	/// a `ContentTypeNotSupported` error. When omitted the agent
	/// chooses its preferred output format.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub accepted_output_modes: Vec<String>,

	/// Whether the agent should return immediately without waiting
	/// for task completion. When true the agent returns a Task in a
	/// non-terminal state immediately; the caller must poll or
	/// subscribe for updates. When false (the default) the agent may
	/// hold the connection open until work is complete.
	///
	/// Note: this is the semantic inverse of the pre-v1.0 `blocking`
	/// field—`return_immediately: true` corresponds to the old
	/// `blocking: false`. Omitted from the wire when false—proto3
	/// canonical form suppresses default-valued boolean fields.
	#[serde(default, skip_serializing_if = "crate::serde_helpers::is_false")]
	pub return_immediately: bool,

	/// Maximum number of conversation history messages to include
	/// in the response. Limits how much prior context the agent
	/// returns alongside the result. When omitted the agent uses
	/// its own default (often all available history).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub history_length: Option<i32>,

	/// Push notification configuration for this request.
	/// When provided the agent sends status updates to the configured
	/// webhook URL instead of (or in addition to) the caller polling
	/// for them. Uses the flat v1.0 `TaskPushNotificationConfig` type.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub task_push_notification_config: Option<TaskPushNotificationConfig>,
}

/// The result of a successful `SendMessage` operation.
///
/// An agent responds with either a Task (when the work is deferred
/// or still in progress) or a Message (when the agent can reply
/// immediately without creating a task). The caller must handle
/// both variants—simple question-answering agents tend to return
/// Messages, while long-running workflow agents return Tasks.
///
/// This follows the A2A v1.0 proto3 oneof wire format. The outer
/// JSON key is the camelCase variant name:
///
///   `{"task": {...}}`
///   `{"message": {...}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum SendMessageResult {
	/// The agent created or updated a Task to track deferred work.
	/// The Task's status indicates whether it's still in progress
	/// (submitted, working) or already finished (completed, failed).
	///
	/// Wire format: `{"task": {...}}`
	Task(Task),

	/// The agent replied immediately without creating a task.
	/// This is common for stateless, request-response agents that
	/// don't need the task lifecycle machinery.
	///
	/// Wire format: `{"message": {...}}`
	Message(Message),
}

impl SendMessageParams {
	/// Create send message parameters with the given message.
	///
	/// Initialises configuration and metadata to None. Use
	/// `with_configuration()` and `with_metadata()` to add optional
	/// fields. This is the most common construction path—a caller
	/// sending a message to an agent with default behaviour:
	///
	/// ```
	/// # use a2a::{SendMessageParams, Message, Role};
	/// let params = SendMessageParams::new(
	///     Message::text("msg-1", Role::User, "Hello, agent!"),
	/// );
	/// ```
	#[must_use]
	pub fn new(message: Message) -> Self {
		Self {
			message,
			configuration: None,
			metadata: None,
			tenant: None,
		}
	}

	/// Attach configuration controlling agent behaviour.
	///
	/// The configuration specifies output format preferences, whether
	/// the agent should block until completion, how much conversation
	/// history to return, and optional push notification setup. When
	/// omitted, the agent uses its own defaults for all of these.
	#[must_use]
	pub fn with_configuration(mut self, configuration: SendMessageConfiguration) -> Self {
		self.configuration = Some(configuration);
		self
	}

	/// Attach caller-defined metadata to this request.
	///
	/// The spec doesn't prescribe the structure—callers use this
	/// for tracing IDs, session context, billing tags, or other
	/// pass-through data that the agent may echo back in the response.
	/// The parameter type is `serde_json::Map<String, serde_json::Value>`
	/// so the type system enforces the proto3 google.protobuf.Struct
	/// constraint—arrays and scalars cannot be passed at compile time.
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}

	/// Scope this message to a specific tenant.
	///
	/// In multi-tenant deployments the agent may serve multiple
	/// isolated tenants. Providing a tenant identifier routes the
	/// message to the correct tenant's context. When omitted the
	/// agent uses its default tenant resolution strategy.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

impl SendMessageConfiguration {
	/// Create a new empty configuration with all fields set to None.
	///
	/// Every field is optional—when omitted the agent uses its own
	/// defaults. Use the `with_*` chainable setters to opt into
	/// specific behaviour:
	///
	/// ```
	/// # use a2a::SendMessageConfiguration;
	/// let config = SendMessageConfiguration::new()
	///     .with_accepted_output_modes(vec!["text/plain".into()])
	///     .with_return_immediately(true);
	/// ```
	#[must_use]
	pub fn new() -> Self {
		Self {
			accepted_output_modes: Vec::new(),
			return_immediately: false,
			history_length: None,
			task_push_notification_config: None,
		}
	}

	/// Declare which output media types the caller accepts.
	///
	/// If the agent cannot produce any of these types it returns
	/// a `ContentTypeNotSupported` error. When omitted the agent
	/// chooses its preferred output format from its declared
	/// `default_output_modes`.
	#[must_use]
	pub fn with_accepted_output_modes(mut self, modes: Vec<String>) -> Self {
		self.accepted_output_modes = modes;
		self
	}

	/// Control whether the agent should return immediately without
	/// waiting for task completion.
	///
	/// When true the agent returns a Task in a non-terminal state
	/// right away; the caller must poll or subscribe for updates.
	/// When false (or omitted) the agent may hold the connection
	/// open until work is complete. Setting this to true is the
	/// semantic equivalent of the pre-v1.0 `blocking: false`.
	#[must_use]
	pub fn with_return_immediately(mut self, return_immediately: bool) -> Self {
		self.return_immediately = return_immediately;
		self
	}

	/// Limit how many conversation history messages the agent returns.
	///
	/// Controls how much prior context the agent includes alongside
	/// the result. Useful for bandwidth-constrained callers or when
	/// the conversation history is very large. When omitted the agent
	/// uses its own default (often all available history).
	#[must_use]
	pub fn with_history_length(mut self, length: i32) -> Self {
		self.history_length = Some(length);
		self
	}

	/// Configure push notification delivery for this request.
	///
	/// When provided the agent sends status updates to the configured
	/// webhook URL instead of (or in addition to) the caller polling
	/// for them. This is essential for long-running tasks where the
	/// caller doesn't want to hold an SSE connection open
	/// indefinitely. Uses the flat v1.0 `TaskPushNotificationConfig`.
	#[must_use]
	pub fn with_task_push_notification_config(
		mut self,
		config: TaskPushNotificationConfig,
	) -> Self {
		self.task_push_notification_config = Some(config);
		self
	}
}

impl Default for SendMessageConfiguration {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::part::Part;
	use crate::role::Role;

	// A minimal send message request needs only the message field.
	// Configuration, metadata, and tenant are optional—callers
	// shouldn't need boilerplate to send a simple text message.
	#[test]
	fn minimal_params_serialise() {
		let params = SendMessageParams {
			message: Message {
				message_id: "msg-min".into(),
				role: Role::User,
				parts: vec![Part::text("hello")],
				metadata: None,
				context_id: None,
				task_id: None,
				reference_task_ids: vec![],
				extensions: vec![],
			},
			configuration: None,
			metadata: None,
			tenant: None,
		};

		let json = serde_json::to_value(&params).unwrap();
		assert!(json["message"].is_object());
		assert!(
			!json.as_object().unwrap().contains_key("configuration"),
			"configuration must be absent when None"
		);
		assert!(
			!json.as_object().unwrap().contains_key("metadata"),
			"metadata must be absent when None"
		);
		assert!(
			!json.as_object().unwrap().contains_key("tenant"),
			"tenant must be absent when None"
		);
	}

	// The configuration object controls agent behaviour. All fields
	// are optional and only appear on the wire when explicitly set.
	// This test verifies that set fields serialise correctly with
	// camelCase keys and that unset fields are omitted, not null.
	#[test]
	fn configuration_serialises_camel_case() {
		let config = SendMessageConfiguration {
			accepted_output_modes: vec!["text/plain".into(), "application/json".into()],
			return_immediately: true,
			history_length: Some(10),
			task_push_notification_config: None,
		};

		let json = serde_json::to_value(&config).unwrap();
		assert_eq!(json["acceptedOutputModes"][0], "text/plain");
		assert_eq!(json["returnImmediately"], true);
		assert_eq!(json["historyLength"], 10);
		assert!(
			!json
				.as_object()
				.unwrap()
				.contains_key("taskPushNotificationConfig"),
			"task push notification config must be absent when None"
		);
		assert!(
			!json.as_object().unwrap().contains_key("blocking"),
			"old 'blocking' field must not appear"
		);
	}

	// The result of message/send is an externally-tagged enum—the
	// outer JSON key is "task" or "message", matching the A2A v1.0
	// oneof wire format. The Task object is nested under "task" and
	// the Message object is nested under "message".
	#[test]
	fn result_deserialises_task_variant() {
		let json = serde_json::json!({
			"task": {
				"id": "task-1",
				"contextId": "ctx-1",
				"status": {
					"state": "submitted",
					"timestamp": "2026-03-12T10:00:00Z"
				}
			}
		});

		let result: SendMessageResult = serde_json::from_value(json).unwrap();
		match result {
			SendMessageResult::Task(task) => assert_eq!(task.id, "task-1"),
			SendMessageResult::Message(_) => panic!("expected Task variant"),
		}
	}

	// When an agent replies immediately without creating a task,
	// the result is wrapped as {"message": {...}}. This is the common
	// path for lightweight, stateless agents that answer questions directly.
	#[test]
	fn result_deserialises_message_variant() {
		let json = serde_json::json!({
			"message": {
				"messageId": "reply-1",
				"role": "agent",
				"parts": [{"text": "hello back"}]
			}
		});

		let result: SendMessageResult = serde_json::from_value(json).unwrap();
		match result {
			SendMessageResult::Message(msg) => assert_eq!(msg.role, Role::Agent),
			SendMessageResult::Task(_) => panic!("expected Message variant"),
		}
	}

	// SendMessageResult round-trips through serde preserving the
	// externally-tagged wire format. The Task variant serialises as
	// {"task": {...}} and the Message variant as {"message": {...}}.
	#[test]
	fn result_round_trips_both_variants() {
		let task_result = SendMessageResult::Task(crate::task::Task {
			id: "rt-task-1".into(),
			context_id: "rt-ctx-1".into(),
			status: crate::task::TaskStatus {
				state: crate::task_state::TaskState::Submitted,
				message: None,
				timestamp: None,
			},
			artifacts: vec![],
			history: vec![],
			metadata: None,
		});

		let msg_result = SendMessageResult::Message(Message {
			message_id: "rt-msg-1".into(),
			role: Role::Agent,
			parts: vec![Part::text("hello")],
			context_id: None,
			task_id: None,
			reference_task_ids: vec![],
			metadata: None,
			extensions: vec![],
		});

		for result in [task_result, msg_result] {
			let json = serde_json::to_string(&result).unwrap();
			let back: SendMessageResult = serde_json::from_str(&json).unwrap();
			assert_eq!(back, result);
		}
	}

	// The Task variant serialises as {"task": {...}}—the outer key
	// is "task", not the Task's own "id" field at the top level.
	#[test]
	fn result_task_variant_wire_key() {
		let result = SendMessageResult::Task(crate::task::Task {
			id: "wire-task-1".into(),
			context_id: "wire-ctx-1".into(),
			status: crate::task::TaskStatus {
				state: crate::task_state::TaskState::Submitted,
				message: None,
				timestamp: None,
			},
			artifacts: vec![],
			history: vec![],
			metadata: None,
		});

		let json = serde_json::to_value(&result).unwrap();
		assert!(
			json.as_object().unwrap().contains_key("task"),
			"outer key must be 'task'"
		);
		assert_eq!(json["task"]["id"], "wire-task-1");
	}

	// The Message variant serialises as {"message": {...}}—the outer
	// key is "message", not the Message's own "messageId" field at
	// the top level.
	#[test]
	fn result_message_variant_wire_key() {
		let result = SendMessageResult::Message(Message {
			message_id: "wire-msg-1".into(),
			role: Role::Agent,
			parts: vec![Part::text("done")],
			context_id: None,
			task_id: None,
			reference_task_ids: vec![],
			metadata: None,
			extensions: vec![],
		});

		let json = serde_json::to_value(&result).unwrap();
		assert!(
			json.as_object().unwrap().contains_key("message"),
			"outer key must be 'message'"
		);
		assert_eq!(json["message"]["messageId"], "wire-msg-1");
	}

	// Full round-trip of SendMessageParams through serde to confirm
	// nothing is lost or mangled during serialisation. This is the
	// fundamental correctness property—if params don't round-trip,
	// the caller and agent will disagree on the request contents.
	#[test]
	fn params_round_trip() {
		let params = SendMessageParams {
			message: Message {
				message_id: "msg-1".into(),
				role: Role::User,
				parts: vec![Part::text("test")],
				metadata: None,
				context_id: Some("ctx-1".into()),
				task_id: None,
				reference_task_ids: vec![],
				extensions: vec![],
			},
			configuration: Some(SendMessageConfiguration {
				accepted_output_modes: vec!["text/plain".into()],
				return_immediately: false,
				history_length: None,
				task_push_notification_config: None,
			}),
			metadata: Some(
				serde_json::json!({"trace_id": "abc-123"})
					.as_object()
					.unwrap()
					.clone(),
			),
			tenant: None,
		};

		let json = serde_json::to_string(&params).unwrap();
		let back: SendMessageParams = serde_json::from_str(&json).unwrap();
		assert_eq!(back, params);
	}

	// SendMessageParams::new creates params with only the message
	// set, leaving configuration and metadata as None. This is the
	// common path for simple message sends without special config.
	#[test]
	fn params_new_sets_message_only() {
		let params =
			SendMessageParams::new(Message::text("message-builder-1", Role::User, "hello"));

		assert_eq!(params.message.message_id, "message-builder-1");
		assert!(params.configuration.is_none());
		assert!(params.metadata.is_none());
	}

	// SendMessageParams chainable setters populate configuration
	// and metadata independently. The resulting params must produce
	// the same wire format as struct literal construction.
	#[test]
	fn params_setters_populate_optional_fields() {
		let params = SendMessageParams::new(Message::text(
			"message-configured-1",
			Role::User,
			"configured",
		))
		.with_configuration(
			SendMessageConfiguration::new()
				.with_return_immediately(true)
				.with_history_length(5),
		)
		.with_metadata(
			serde_json::json!({"trace_id": "trace-1"})
				.as_object()
				.unwrap()
				.clone(),
		);

		let configuration = params.configuration.as_ref().unwrap();
		assert!(configuration.return_immediately);
		assert_eq!(configuration.history_length, Some(5));
		assert!(configuration.accepted_output_modes.is_empty());
		assert!(configuration.task_push_notification_config.is_none());
		assert_eq!(params.metadata.as_ref().unwrap()["trace_id"], "trace-1");
	}

	// SendMessageConfiguration::new creates an empty config with all
	// fields at their zero values. This is the starting point for the
	// builder pattern—callers chain setters to opt into specific behaviour.
	#[test]
	fn configuration_new_starts_empty() {
		let configuration = SendMessageConfiguration::new();

		assert!(configuration.accepted_output_modes.is_empty());
		assert!(!configuration.return_immediately);
		assert!(configuration.history_length.is_none());
		assert!(configuration.task_push_notification_config.is_none());
	}

	// SendMessageConfiguration::default() must produce the same
	// result as SendMessageConfiguration::new(). This is a contract
	// that the Default trait impl delegates correctly.
	#[test]
	fn configuration_default_matches_new() {
		let from_new = SendMessageConfiguration::new();
		let from_default = SendMessageConfiguration::default();
		assert_eq!(from_new, from_default);
	}

	// SendMessageConfiguration chainable setters must each populate
	// exactly one field. A fully configured configuration built
	// through the builder should produce identical wire output to
	// struct literal construction.
	#[test]
	fn configuration_setters_populate_all_fields() {
		let configuration = SendMessageConfiguration::new()
			.with_accepted_output_modes(vec!["text/plain".into(), "application/json".into()])
			.with_return_immediately(false)
			.with_history_length(20);

		assert_eq!(
			configuration.accepted_output_modes,
			vec!["text/plain".to_string(), "application/json".to_string()]
		);
		assert!(!configuration.return_immediately);
		assert_eq!(configuration.history_length, Some(20));
		assert!(configuration.task_push_notification_config.is_none());

		// Round-trip to verify wire compatibility.
		let json = serde_json::to_string(&configuration).unwrap();
		let back: SendMessageConfiguration = serde_json::from_str(&json).unwrap();
		assert_eq!(back, configuration);
	}

	// SendMessageParams::new creates params with only the message set,
	// leaving configuration, metadata, and tenant as None. Verify
	// that tenant is absent from the wire format by default.
	#[test]
	fn params_new_tenant_defaults_to_none() {
		let params = SendMessageParams::new(Message::text("message-tenant-1", Role::User, "hello"));
		assert!(params.tenant.is_none());
		let json = serde_json::to_value(&params).unwrap();
		assert!(
			!json.as_object().unwrap().contains_key("tenant"),
			"tenant must be absent from the wire format when None"
		);
	}

	// with_tenant() scopes the message send to a specific tenant.
	// Verify the setter populates the tenant field and that the
	// resulting wire format includes the tenant key.
	#[test]
	fn params_with_tenant_populates_field() {
		let params = SendMessageParams::new(Message::text("message-tenant-2", Role::User, "hello"))
			.with_tenant("org-abc");
		assert_eq!(params.tenant, Some("org-abc".into()));
		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["tenant"], "org-abc");
	}

	// with_task_push_notification_config() wires a flat v1.0
	// TaskPushNotificationConfig into the message configuration.
	// The JSON key must be taskPushNotificationConfig (camelCase).
	#[test]
	fn configuration_with_task_push_notification_config() {
		use crate::operation::push_notification::TaskPushNotificationConfig;

		let push_config =
			TaskPushNotificationConfig::new("task-cfg-1", "https://hooks.example.com/w");
		let configuration =
			SendMessageConfiguration::new().with_task_push_notification_config(push_config.clone());

		let json = serde_json::to_value(&configuration).unwrap();
		assert!(
			json.get("taskPushNotificationConfig").is_some(),
			"must use taskPushNotificationConfig key"
		);
		assert!(
			json.get("pushNotificationConfig").is_none(),
			"old pushNotificationConfig key must not appear"
		);
		assert_eq!(
			configuration
				.task_push_notification_config
				.as_ref()
				.unwrap(),
			&push_config
		);
	}

	// The returnImmediately field uses camelCase on the wire and the
	// old 'blocking' key must never appear in serialised output.
	#[test]
	fn configuration_return_immediately_wire_key() {
		let configuration = SendMessageConfiguration::new().with_return_immediately(true);
		let json = serde_json::to_value(&configuration).unwrap();
		assert_eq!(json["returnImmediately"], true);
		assert!(
			json.get("blocking").is_none(),
			"blocking key from pre-v1.0 must not appear"
		);
	}
}
