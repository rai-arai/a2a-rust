// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The core handler trait for implementing A2A agents.
//!
//! `A2AHandler` is the primary abstraction third-party developers build
//! against. Each method corresponds to one of the protocol's JSON-RPC
//! operations. Implementors provide the business logic; the dispatch
//! layer handles JSON-RPC envelope concerns (deserialisation, error
//! wrapping, method routing).
//!
//! The trait uses native async fn in traits (stable since Rust 1.75).
//! Streaming operations (`message_stream`, `resubscribe_task`) return a
//! `futures_core::Stream` of `StreamEvents`. This keeps the trait
//! runtime-agnostic—the HTTP layer (axum, actix, etc.) adapts the
//! stream into SSE or whatever transport it uses.
//!
//! Every method has a default implementation that returns
//! `UnsupportedOperation`. This lets agents implement only the methods
//! they support—a simple question-answering agent might only
//! implement `message_send`, ignoring task management and push
//! notifications entirely.
//!
//! The trait requires Send + Sync because multi-threaded runtimes
//! (tokio, async-std) need to move handler references across threads.
//! For single-threaded targets (wasm32), a non-Send variant can be
//! added later behind a cfg gate.

use std::pin::Pin;

use futures_core::Stream;

use crate::context::RequestContext;
use a2a::agent_card::AgentCard;
use a2a::error::A2AError;
use a2a::operation::{
	CancelTaskParams, DeleteTaskPushNotificationConfigParams, GetExtendedAgentCardParams,
	GetTaskParams, GetTaskPushNotificationConfigParams, ListTaskPushNotificationConfigsParams,
	ListTaskPushNotificationConfigsResponse, ListTasksParams, ListTasksResponse, SendMessageParams,
	SendMessageResult, SubscribeToTaskParams, TaskPushNotificationConfig,
};
use a2a::stream_event::StreamResponse;
use a2a::task::Task;

/// A boxed, Send-safe stream of A2A streaming responses.
///
/// Used as the return type for streaming operations (`SendStreamingMessage`,
/// `SubscribeToTask`). Boxed because trait methods cannot return
/// opaque impl types, and the Pin<Box<...>> overhead is negligible
/// compared to the network I/O these streams represent.
pub type EventStream<'a> =
	Pin<Box<dyn Stream<Item = Result<StreamResponse, A2AError>> + Send + 'a>>;

/// The handler trait for A2A agent implementations.
///
/// Each method maps to a JSON-RPC operation. All methods are async
/// and return Result—the dispatcher translates Ok values into
/// JSON-RPC success responses and Err values into error responses.
///
/// Default implementations return `UnsupportedOperation` for every
/// method. Override only the operations your agent supports. The
/// agent card's capabilities declaration should match which methods
/// are actually implemented.
pub trait A2AHandler: Send + Sync {
	/// Handle a `SendMessage` request.
	///
	/// The agent processes the incoming message and returns either a
	/// Task (for deferred or tracked work) or a Message (for immediate
	/// stateless replies). This is the most fundamental operation —
	/// almost every agent implements it.
	fn message_send(
		&self,
		context: &RequestContext,
		params: SendMessageParams,
	) -> impl std::future::Future<Output = Result<SendMessageResult, A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::unsupported_operation("SendMessage")) }
	}

	/// Handle a `SendStreamingMessage` request.
	///
	/// Like `message_send` but returns a stream of events instead of a
	/// single result. The stream emits `TaskStatusUpdateEvents` and
	/// `TaskArtifactUpdateEvents` as the agent works, followed by a
	/// final Task or Message event when processing completes.
	fn message_stream(
		&self,
		context: &RequestContext,
		params: SendMessageParams,
	) -> impl std::future::Future<Output = Result<EventStream<'_>, A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::unsupported_operation("SendStreamingMessage")) }
	}

	/// Handle a `GetTask` request.
	///
	/// Retrieves a task by its identifier, optionally including
	/// conversation history up to the requested length.
	fn get_task(
		&self,
		context: &RequestContext,
		params: GetTaskParams,
	) -> impl std::future::Future<Output = Result<Task, A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::unsupported_operation("GetTask")) }
	}

	/// Handle a `ListTasks` request.
	///
	/// Returns a paginated list of tasks matching the supplied filter
	/// criteria (context, status, timestamp). The default implementation
	/// returns `UnsupportedOperation` so agents that do not expose task
	/// listing—for example stateless or single-task agents—can ignore
	/// this operation entirely.
	fn list_tasks(
		&self,
		context: &RequestContext,
		params: ListTasksParams,
	) -> impl std::future::Future<Output = Result<ListTasksResponse, A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::unsupported_operation("ListTasks")) }
	}

	/// Handle a `CancelTask` request.
	///
	/// Requests cancellation of a running task. Returns the updated
	/// Task with its status set to canceled, or an error if the task
	/// is in a non-cancelable state.
	fn cancel_task(
		&self,
		context: &RequestContext,
		params: CancelTaskParams,
	) -> impl std::future::Future<Output = Result<Task, A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::unsupported_operation("CancelTask")) }
	}

	/// Handle a `SubscribeToTask` request.
	///
	/// Reconnects to an existing task's event stream after a dropped
	/// connection. Returns the same type of stream as `SendStreamingMessage`.
	fn resubscribe_task(
		&self,
		context: &RequestContext,
		params: SubscribeToTaskParams,
	) -> impl std::future::Future<Output = Result<EventStream<'_>, A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::unsupported_operation("SubscribeToTask")) }
	}

	/// Handle a `CreateTaskPushNotificationConfig` request.
	///
	/// Creates or updates a push notification configuration on a task.
	/// Returns the saved config, which may include server-assigned
	/// fields like a config ID.
	fn set_push_notification_config(
		&self,
		context: &RequestContext,
		params: TaskPushNotificationConfig,
	) -> impl std::future::Future<Output = Result<TaskPushNotificationConfig, A2AError>> + Send + '_
	{
		let _ = (context, params);
		async { Err(A2AError::push_notification_not_supported()) }
	}

	/// Handle a `GetTaskPushNotificationConfig` request.
	///
	/// Retrieves a push notification configuration for a task.
	fn get_push_notification_config(
		&self,
		context: &RequestContext,
		params: GetTaskPushNotificationConfigParams,
	) -> impl std::future::Future<Output = Result<TaskPushNotificationConfig, A2AError>> + Send + '_
	{
		let _ = (context, params);
		async { Err(A2AError::push_notification_not_supported()) }
	}

	/// Handle a `ListTaskPushNotificationConfigs` request.
	///
	/// Lists all push notification configurations associated with
	/// a task. Returns a paginated response.
	fn list_push_notification_configs(
		&self,
		context: &RequestContext,
		params: ListTaskPushNotificationConfigsParams,
	) -> impl std::future::Future<Output = Result<ListTaskPushNotificationConfigsResponse, A2AError>>
	+ Send
	+ '_ {
		let _ = (context, params);
		async { Err(A2AError::push_notification_not_supported()) }
	}

	/// Handle a `DeleteTaskPushNotificationConfig` request.
	///
	/// Removes a push notification configuration from a task.
	/// After deletion the agent stops sending notifications to
	/// the config's webhook URL.
	fn delete_push_notification_config(
		&self,
		context: &RequestContext,
		params: DeleteTaskPushNotificationConfigParams,
	) -> impl std::future::Future<Output = Result<(), A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::push_notification_not_supported()) }
	}

	/// Handle a `GetExtendedAgentCard` request.
	///
	/// Returns the agent's extended card, which may include
	/// capabilities, skills, or configuration that require
	/// authentication to access. The base agent card (served
	/// at /.well-known/agent.json) is a public subset of this.
	fn get_extended_agent_card(
		&self,
		context: &RequestContext,
		params: GetExtendedAgentCardParams,
	) -> impl std::future::Future<Output = Result<AgentCard, A2AError>> + Send + '_ {
		let _ = (context, params);
		async { Err(A2AError::unsupported_operation("GetExtendedAgentCard")) }
	}
}
