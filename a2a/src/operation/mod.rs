// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Operation module—typed request and response structures for the
//! A2A protocol's JSON-RPC methods.
//!
//! The A2A protocol defines 11 JSON-RPC methods grouped into four
//! domains: messaging, task management, push notifications, and
//! discovery. Each domain has its own submodule containing the typed
//! parameter and result structures that flow through those methods.
//!
//! The method name constants below are the exact strings that appear
//! in the "method" field of JSON-RPC requests. They are defined here
//! rather than scattered across submodules because they form the
//! complete method table an agent or client needs to dispatch against.
//! All method names are `PascalCase`, matching the proto3 RPC names.

pub mod discovery;
pub mod messaging;
pub mod push_notification;
pub mod task_management;

pub use discovery::*;
pub use messaging::*;
pub use push_notification::*;
pub use task_management::*;

/// JSON-RPC method name for sending a message to an agent.
/// The agent processes the message and returns either a Task
/// (for deferred work) or a Message (for immediate replies).
pub const METHOD_SEND_MESSAGE: &str = "SendMessage";

/// JSON-RPC method name for sending a message with SSE streaming.
/// The agent streams back status updates, artifact updates, and
/// eventually the final Task or Message as server-sent events.
pub const METHOD_SEND_STREAMING_MESSAGE: &str = "SendStreamingMessage";

/// JSON-RPC method name for retrieving a task by its identifier.
/// Returns the full Task object including status, history, and
/// artifacts up to the requested history length.
pub const METHOD_GET_TASK: &str = "GetTask";

/// JSON-RPC method name for listing tasks with optional filters.
/// Returns a paginated collection of tasks matching the supplied
/// context, status, and timing constraints.
pub const METHOD_LIST_TASKS: &str = "ListTasks";

/// JSON-RPC method name for canceling a running task.
/// Only tasks in non-terminal states can be canceled—attempting
/// to cancel a completed or failed task returns `TaskNotCancelable`.
pub const METHOD_CANCEL_TASK: &str = "CancelTask";

/// JSON-RPC method name for re-subscribing to a task's SSE stream.
/// Used to reconnect after a dropped connection without resending
/// the original message. Returns the same SSE event stream as
/// `SendStreamingMessage`.
pub const METHOD_SUBSCRIBE_TO_TASK: &str = "SubscribeToTask";

/// JSON-RPC method name for creating or updating a push notification
/// configuration on a task. The agent will POST status updates to
/// the configured webhook URL.
pub const METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG: &str = "CreateTaskPushNotificationConfig";

/// JSON-RPC method name for retrieving a task's push notification
/// configuration.
pub const METHOD_GET_TASK_PUSH_NOTIFICATION_CONFIG: &str = "GetTaskPushNotificationConfig";

/// JSON-RPC method name for listing all push notification configs
/// associated with a task.
pub const METHOD_LIST_TASK_PUSH_NOTIFICATION_CONFIGS: &str = "ListTaskPushNotificationConfigs";

/// JSON-RPC method name for deleting a push notification config
/// from a task.
pub const METHOD_DELETE_TASK_PUSH_NOTIFICATION_CONFIG: &str = "DeleteTaskPushNotificationConfig";

/// JSON-RPC method name for retrieving the agent's extended card.
/// Unlike the public agent card at /.well-known/agent.json, the
/// extended card may include additional capabilities, skills, or
/// configuration that require authentication to access.
pub const METHOD_GET_EXTENDED_AGENT_CARD: &str = "GetExtendedAgentCard";

#[cfg(test)]
mod tests {
	use super::*;

	// Method name strings are wire-format identifiers—they must
	// match the spec exactly or interoperability breaks. This test
	// pins every method name to catch accidental typos or renames.
	// All names are PascalCase, matching the proto3 RPC names.
	#[test]
	fn method_constants_match_spec() {
		assert_eq!(METHOD_SEND_MESSAGE, "SendMessage");
		assert_eq!(METHOD_SEND_STREAMING_MESSAGE, "SendStreamingMessage");
		assert_eq!(METHOD_GET_TASK, "GetTask");
		assert_eq!(METHOD_LIST_TASKS, "ListTasks");
		assert_eq!(METHOD_CANCEL_TASK, "CancelTask");
		assert_eq!(METHOD_SUBSCRIBE_TO_TASK, "SubscribeToTask");
		assert_eq!(
			METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG,
			"CreateTaskPushNotificationConfig"
		);
		assert_eq!(
			METHOD_GET_TASK_PUSH_NOTIFICATION_CONFIG,
			"GetTaskPushNotificationConfig"
		);
		assert_eq!(
			METHOD_LIST_TASK_PUSH_NOTIFICATION_CONFIGS,
			"ListTaskPushNotificationConfigs"
		);
		assert_eq!(
			METHOD_DELETE_TASK_PUSH_NOTIFICATION_CONFIG,
			"DeleteTaskPushNotificationConfig"
		);
		assert_eq!(METHOD_GET_EXTENDED_AGENT_CARD, "GetExtendedAgentCard");
	}

	// There are exactly 11 method constants (SendMessage,
	// SendStreamingMessage, GetTask, ListTasks, CancelTask,
	// SubscribeToTask, and 4 push notification operations plus
	// GetExtendedAgentCard). This count guards against accidentally
	// dropping or duplicating methods.
	#[test]
	fn all_methods_accounted_for() {
		let methods = [
			METHOD_SEND_MESSAGE,
			METHOD_SEND_STREAMING_MESSAGE,
			METHOD_GET_TASK,
			METHOD_LIST_TASKS,
			METHOD_CANCEL_TASK,
			METHOD_SUBSCRIBE_TO_TASK,
			METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG,
			METHOD_GET_TASK_PUSH_NOTIFICATION_CONFIG,
			METHOD_LIST_TASK_PUSH_NOTIFICATION_CONFIGS,
			METHOD_DELETE_TASK_PUSH_NOTIFICATION_CONFIG,
			METHOD_GET_EXTENDED_AGENT_CARD,
		];
		assert_eq!(methods.len(), 11);

		// Every method name must be unique—duplicates would cause
		// dispatch ambiguity.
		let mut unique = std::collections::HashSet::new();
		for m in &methods {
			assert!(unique.insert(m), "duplicate method: {m}");
		}
	}
}
