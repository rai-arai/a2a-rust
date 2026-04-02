// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Server-side handler trait and JSON-RPC dispatch for A2A agents.
//!
//! This crate provides the core abstractions for building A2A agents:
//!
//!   `A2AHandler`—the trait that agents implement, with one async method
//!   per protocol operation
//!   `dispatch`—the JSON-RPC routing and deserialisation layer
//!   `RequestContext`—per-request metadata (auth tokens, trace IDs)
//!
//! The handler trait is runtime-agnostic. It uses native async fn in
//! traits and returns `futures_core::Stream` for streaming operations.
//! The dispatch layer is pure async—it doesn't spawn tasks, open
//! connections, or do any I/O. The transport layer (axum, actix, etc.)
//! drives the futures and streams returned by dispatch.

pub mod context;
pub mod dispatch;
pub mod handler;

pub use context::{ApiKey, BearerToken, CorrelationId, Extensions, RequestContext};
pub use dispatch::{DispatchResult, dispatch, is_streaming_method};
pub use handler::{A2AHandler, EventStream};

#[cfg(test)]
mod tests {
	use crate::context::RequestContext;
	use crate::*;
	use a2a::error;
	use a2a::jsonrpc::JsonRpcRequest;
	use a2a::message::Message;
	use a2a::operation;
	use a2a::operation::discovery::GetExtendedAgentCardParams;
	use a2a::operation::push_notification::{
		DeleteTaskPushNotificationConfigParams, GetTaskPushNotificationConfigParams,
		ListTaskPushNotificationConfigsParams, ListTaskPushNotificationConfigsResponse,
		TaskPushNotificationConfig,
	};
	use a2a::operation::task_management::{
		ListTasksParams, ListTasksResponse, SubscribeToTaskParams,
	};
	use a2a::operation::{SendMessageParams, SendMessageResult};
	use a2a::role::Role;
	use a2a::stream_event::{StreamResponse, TaskStatusUpdateEvent};
	use a2a::task::{Task, TaskStatus};

	use a2a::agent_card::{
		AgentCapabilities, AgentCard, AgentCardRequired, AgentInterface, AgentSkill,
	};
	use a2a::task_state::TaskState;
	use time::macros::datetime;

	// A handler with all default implementations rejects every
	// operation with UnsupportedOperation or PushNotificationNotSupported.
	// This is the baseline—agents override only what they support.
	struct DefaultHandler;
	impl A2AHandler for DefaultHandler {}

	// An echo handler that implements message_send by returning
	// the caller's message text back as an agent message. This
	// demonstrates the minimal viable agent—a stateless handler
	// that responds immediately without creating a task.
	struct EchoHandler;
	impl A2AHandler for EchoHandler {
		fn message_send(
			&self,
			_context: &RequestContext,
			params: SendMessageParams,
		) -> impl std::future::Future<Output = Result<SendMessageResult, a2a::error::A2AError>> + Send + '_
		{
			async move {
				let reply = Message::text(
					format!("reply-to-{}", params.message.message_id),
					Role::Agent,
					format!("echo: {}", params.message.parts.first().unwrap()),
				);
				Ok(SendMessageResult::Message(reply))
			}
		}
	}

	// A task-based handler that creates a task and returns it.
	// This demonstrates the deferred work pattern—the agent
	// accepts the request and returns a task handle that the caller
	// can poll or subscribe to for updates.
	struct TaskHandler;
	impl A2AHandler for TaskHandler {
		fn message_send(
			&self,
			_context: &RequestContext,
			params: SendMessageParams,
		) -> impl std::future::Future<Output = Result<SendMessageResult, a2a::error::A2AError>> + Send + '_
		{
			async move {
				let task = Task::new(
					"task-created-1",
					params
						.message
						.context_id
						.unwrap_or_else(|| "context-default".into()),
					TaskStatus::submitted().with_timestamp(datetime!(2026-03-16 12:00:00 UTC)),
				);
				Ok(SendMessageResult::Task(task))
			}
		}

		fn message_stream(
			&self,
			_context: &RequestContext,
			params: SendMessageParams,
		) -> impl std::future::Future<Output = Result<EventStream<'_>, a2a::error::A2AError>> + Send + '_
		{
			async move {
				let context_id = params
					.message
					.context_id
					.unwrap_or_else(|| "context-stream".into());

				let events = vec![
					Ok(StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
						"task-stream-1",
						context_id.clone(),
						TaskStatus::working().with_timestamp(datetime!(2026-03-16 12:00:01 UTC)),
					))),
					Ok(StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
						"task-stream-1",
						context_id,
						TaskStatus::completed().with_timestamp(datetime!(2026-03-16 12:00:02 UTC)),
					))),
				];

				Ok(Box::pin(futures_util::stream::iter(events)) as EventStream<'_>)
			}
		}

		/// Handle SubscribeToTask by replaying the task's event
		/// history and then streaming any remaining live updates.
		///
		/// In this test handler the "replay" is a single completed
		/// status StreamResponse—a real agent would replay all events
		/// the caller missed since their last seen event, then continue
		/// with live delivery. The important thing is that resubscribe
		/// returns the same EventStream type as message_stream,
		/// demonstrating that the transport layer handles both identically.
		fn resubscribe_task(
			&self,
			_context: &RequestContext,
			params: SubscribeToTaskParams,
		) -> impl std::future::Future<Output = Result<EventStream<'_>, a2a::error::A2AError>> + Send + '_
		{
			async move {
				// Simulate replaying the completed status for a task that
				// finished while the caller was disconnected. The caller
				// gets the completed status and knows the task is done
				// without having to call GetTask separately.
				let events = vec![Ok(StreamResponse::StatusUpdate(
					TaskStatusUpdateEvent::new(
						params.id,
						"context-resubscribed",
						TaskStatus::completed().with_timestamp(datetime!(2026-03-16 12:05:00 UTC)),
					),
				))];

				Ok(Box::pin(futures_util::stream::iter(events)) as EventStream<'_>)
			}
		}
	}

	// Dispatching a SendMessage to a default handler returns an
	// UnsupportedOperation error. This verifies the full pipeline:
	// JSON-RPC envelope → params deserialisation → handler call →
	// error wrapping → JSON-RPC response envelope.
	#[tokio::test]
	async fn default_handler_rejects_message_send() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SEND_MESSAGE,
			serde_json::json!({
				"message": {
					"messageId": "test-msg-1",
					"role": "user",
					"parts": [{"text": "hello"}]
				}
			}),
			serde_json::json!("req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!("req-1"));
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::UNSUPPORTED_OPERATION);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Unknown methods must return a METHOD_NOT_FOUND error per the
	// JSON-RPC 2.0 spec. The dispatcher handles this before the
	// handler is ever called.
	#[tokio::test]
	async fn unknown_method_returns_method_not_found() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			"nonexistent/method",
			serde_json::json!({}),
			serde_json::json!("req-2"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!("req-2"));
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::METHOD_NOT_FOUND);
				assert!(err.message.contains("nonexistent/method"));
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Malformed params must return INVALID_PARAMS, not crash.
	// This test sends a string where an object is expected—the
	// serde deserialisation fails and the dispatcher wraps the
	// error into a proper JSON-RPC response.
	#[tokio::test]
	async fn malformed_params_returns_invalid_params() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_GET_TASK,
			serde_json::json!("not an object"),
			serde_json::json!("req-3"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!("req-3"));
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::INVALID_PARAMS);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Push notification operations default to PushNotificationNotSupported
	// rather than UnsupportedOperation. This is a distinct error code
	// because the spec treats push notification support as a separate
	// capability from general operation support.
	#[tokio::test]
	async fn default_handler_rejects_push_notifications() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG,
			serde_json::json!({
				"taskId": "task-1",
				"url": "https://example.com/hook"
			}),
			serde_json::json!("req-4"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::PUSH_NOTIFICATION_NOT_SUPPORTED);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// SendStreamingMessage should return a DispatchResult::Stream variant
	// (or a Response if the handler rejects it). With the default
	// handler, it returns an error response because the default
	// implementation returns Err(UnsupportedOperation).
	#[tokio::test]
	async fn default_handler_rejects_message_stream() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SEND_STREAMING_MESSAGE,
			serde_json::json!({
				"message": {
					"messageId": "test-msg-5",
					"role": "user",
					"parts": [{"text": "stream this"}]
				}
			}),
			serde_json::json!("req-5"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::UNSUPPORTED_OPERATION);
			}
			DispatchResult::Stream { .. } => {
				panic!("default handler should not return a stream")
			}
		}
	}

	// The request ID must be echoed back in the response unchanged,
	// regardless of the response type (success or error). This is
	// a fundamental JSON-RPC requirement for request correlation.
	#[tokio::test]
	async fn request_id_echoed_in_error_response() {
		let handler = DefaultHandler;
		let request =
			JsonRpcRequest::new("bad/method", serde_json::json!({}), serde_json::json!(42));

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!(42));
			}
			DispatchResult::Stream { .. } => panic!("expected Response"),
		}
	}

	// A handler that returns a Message result demonstrates the
	// stateless request-response path. The echo handler receives
	// a text message and replies with the same text prefixed by
	// "echo:". The full pipeline is exercised: JSON-RPC envelope →
	// params deserialisation → handler call → result serialisation →
	// JSON-RPC success response.
	#[tokio::test]
	async fn echo_handler_returns_message_result() {
		let handler = EchoHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SEND_MESSAGE,
			serde_json::json!({
				"message": {
					"messageId": "echo-input-1",
					"role": "user",
					"parts": [{"text": "hello agent"}]
				}
			}),
			serde_json::json!("echo-req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!("echo-req-1"));
				assert!(resp.is_success());

				let send_result: SendMessageResult =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				match send_result {
					SendMessageResult::Message(message) => {
						assert_eq!(message.role, Role::Agent);
						assert_eq!(message.message_id, "reply-to-echo-input-1");
					}
					_ => panic!("expected Message variant"),
				}
			}
			DispatchResult::Stream { .. } => panic!("expected Response"),
		}
	}

	// A handler that returns a Task result demonstrates the deferred
	// work pattern. The task handler creates a task in the submitted
	// state and returns it. Callers would then poll or subscribe to
	// track the task's progress.
	#[tokio::test]
	async fn task_handler_returns_task_result() {
		let handler = TaskHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SEND_MESSAGE,
			serde_json::json!({
				"message": {
					"messageId": "task-input-1",
					"role": "user",
					"parts": [{"text": "do something complex"}],
					"contextId": "context-task-1"
				}
			}),
			serde_json::json!("task-req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(resp.is_success());

				let send_result: SendMessageResult =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				match send_result {
					SendMessageResult::Task(task) => {
						assert_eq!(task.id, "task-created-1");
						assert_eq!(task.context_id, "context-task-1");
						assert_eq!(task.status.state, TaskState::Submitted);
					}
					_ => panic!("expected Task variant"),
				}
			}
			DispatchResult::Stream { .. } => panic!("expected Response"),
		}
	}

	// A streaming handler returns a DispatchResult::Stream containing
	// a stream of StreamResponse values. This tests the full streaming
	// dispatch path: JSON-RPC envelope → params → handler → stream.
	// The task handler emits two status updates (working → completed)
	// to simulate a task lifecycle.
	#[tokio::test]
	async fn task_handler_returns_event_stream() {
		use futures_util::StreamExt;

		let handler = TaskHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SEND_STREAMING_MESSAGE,
			serde_json::json!({
				"message": {
					"messageId": "stream-input-1",
					"role": "user",
					"parts": [{"text": "stream this"}],
					"contextId": "context-stream-1"
				}
			}),
			serde_json::json!("stream-req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Stream { id, mut events } => {
				assert_eq!(id, serde_json::json!("stream-req-1"));

				// First event: working status.
				let first = events.next().await.unwrap().unwrap();
				match first {
					StreamResponse::StatusUpdate(event) => {
						assert_eq!(event.task_id, "task-stream-1");
						assert_eq!(event.status.state, TaskState::Working);
					}
					_ => panic!("expected StatusUpdate event"),
				}

				// Second event: completed status.
				let second = events.next().await.unwrap().unwrap();
				match second {
					StreamResponse::StatusUpdate(event) => {
						assert_eq!(event.status.state, TaskState::Completed);
					}
					_ => panic!("expected StatusUpdate event"),
				}

				// Stream should be exhausted.
				assert!(events.next().await.is_none());
			}
			DispatchResult::Response(_) => panic!("expected Stream"),
		}
	}

	// A handler that implements all four push notification CRUD
	// operations. This demonstrates the pattern for agents that
	// support webhook-based status delivery—the agent stores
	// configs per task and returns them on get/list/delete.
	//
	// The implementation is intentionally minimal (no actual
	// storage) because we're testing the dispatch pipeline, not
	// the agent's business logic. Each method returns a
	// deterministic response derived from the input parameters.
	struct PushNotificationHandler;
	impl A2AHandler for PushNotificationHandler {
		fn set_push_notification_config(
			&self,
			_context: &RequestContext,
			params: TaskPushNotificationConfig,
		) -> impl std::future::Future<
			Output = Result<TaskPushNotificationConfig, a2a::error::A2AError>,
		> + Send
		+ '_ {
			async move {
				// Echo the config back with a server-assigned ID if
				// the caller didn't provide one. This mirrors the
				// typical server behaviour of accepting the caller's
				// config and returning the persisted version.
				let task_id_str = params.task_id.as_deref().unwrap_or("unknown");
				let id = params
					.id
					.unwrap_or_else(|| format!("server-assigned-{task_id_str}"));
				let mut config = TaskPushNotificationConfig::for_url(&params.url).with_id(id);
				if let Some(tid) = params.task_id {
					config = config.with_task_id(tid);
				}
				Ok(config)
			}
		}

		fn get_push_notification_config(
			&self,
			_context: &RequestContext,
			params: GetTaskPushNotificationConfigParams,
		) -> impl std::future::Future<
			Output = Result<TaskPushNotificationConfig, a2a::error::A2AError>,
		> + Send
		+ '_ {
			async move {
				// Return a synthetic config for the requested task.
				// A real agent would look this up from storage by
				// task ID and config ID.
				Ok(TaskPushNotificationConfig::new(
					&params.task_id,
					"https://example.com/stored-hook",
				)
				.with_id(&params.id))
			}
		}

		fn list_push_notification_configs(
			&self,
			_context: &RequestContext,
			params: ListTaskPushNotificationConfigsParams,
		) -> impl std::future::Future<
			Output = Result<ListTaskPushNotificationConfigsResponse, a2a::error::A2AError>,
		> + Send
		+ '_ {
			async move {
				// Return two synthetic configs to demonstrate that
				// a task can have multiple notification endpoints.
				let config_a =
					TaskPushNotificationConfig::new(&params.task_id, "https://example.com/hook-a")
						.with_id("cfg-a");
				let config_b =
					TaskPushNotificationConfig::new(&params.task_id, "https://example.com/hook-b")
						.with_id("cfg-b");
				Ok(ListTaskPushNotificationConfigsResponse {
					configs: vec![config_a, config_b],
					next_page_token: String::new(),
				})
			}
		}

		fn delete_push_notification_config(
			&self,
			_context: &RequestContext,
			_params: DeleteTaskPushNotificationConfigParams,
		) -> impl std::future::Future<Output = Result<(), a2a::error::A2AError>> + Send + '_ {
			async move {
				// Deletion returns unit on success. A real agent
				// would remove the config from storage and stop
				// delivering notifications to its webhook URL.
				Ok(())
			}
		}
	}

	// Setting a push notification config through the dispatch pipeline
	// exercises the full path: JSON-RPC envelope →
	// TaskPushNotificationConfig deserialisation → handler call →
	// response serialisation. The handler assigns a server-side config
	// ID when the caller omits one. The v1.0 wire format is flat—no
	// nested pushNotificationConfig wrapper.
	#[tokio::test]
	async fn set_push_notification_config_through_dispatch() {
		let handler = PushNotificationHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG,
			serde_json::json!({
				"taskId": "task-push-1",
				"url": "https://caller.example.com/webhook"
			}),
			serde_json::json!("push-req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(resp.is_success());
				let config: TaskPushNotificationConfig =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				assert_eq!(config.task_id.as_deref(), Some("task-push-1"));
				assert_eq!(config.url, "https://caller.example.com/webhook");
				assert_eq!(
					config.id.as_deref(),
					Some("server-assigned-task-push-1"),
					"handler should assign an ID when caller omits one"
				);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Setting a config that already has a caller-provided ID should
	// preserve it rather than overwriting with a server-assigned one.
	// This tests the conditional logic in the handler's set impl.
	#[tokio::test]
	async fn set_push_notification_config_preserves_caller_id() {
		let handler = PushNotificationHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG,
			serde_json::json!({
				"taskId": "task-push-2",
				"url": "https://caller.example.com/webhook",
				"id": "caller-cfg-1"
			}),
			serde_json::json!("push-req-2"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				let config: TaskPushNotificationConfig =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				assert_eq!(
					config.id.as_deref(),
					Some("caller-cfg-1"),
					"caller-provided ID must be preserved"
				);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Getting a push notification config returns the stored config
	// for the given task. The v1.0 params use taskId and id (both
	// required) rather than the old optional pushNotificationConfigId.
	#[tokio::test]
	async fn get_push_notification_config_through_dispatch() {
		let handler = PushNotificationHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_GET_TASK_PUSH_NOTIFICATION_CONFIG,
			serde_json::json!({
				"taskId": "task-push-3",
				"id": "specific-cfg"
			}),
			serde_json::json!("push-req-3"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(resp.is_success());
				let config: TaskPushNotificationConfig =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				assert_eq!(config.task_id.as_deref(), Some("task-push-3"));
				assert_eq!(config.id.as_deref(), Some("specific-cfg"));
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Listing push notification configs returns a
	// ListTaskPushNotificationConfigsResponse (with pagination support)
	// rather than a bare Vec. The handler returns two configs to verify
	// the dispatch pipeline correctly serialises the response wrapper.
	#[tokio::test]
	async fn list_push_notification_configs_through_dispatch() {
		let handler = PushNotificationHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_LIST_TASK_PUSH_NOTIFICATION_CONFIGS,
			serde_json::json!({
				"taskId": "task-push-4"
			}),
			serde_json::json!("push-req-4"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(resp.is_success());
				let response: ListTaskPushNotificationConfigsResponse =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				assert_eq!(response.configs.len(), 2);
				assert_eq!(response.configs[0].task_id.as_deref(), Some("task-push-4"));
				assert_eq!(response.configs[1].task_id.as_deref(), Some("task-push-4"));
				assert_eq!(response.configs[0].id.as_deref(), Some("cfg-a"));
				assert_eq!(response.configs[1].id.as_deref(), Some("cfg-b"));
				assert!(
					!response.has_next_page(),
					"no pagination cursor when all results fit one page"
				);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Deleting a push notification config returns a null result
	// on success (the handler returns ()). The dispatch pipeline
	// must serialise unit as JSON null and wrap it in a success
	// response envelope.
	#[tokio::test]
	async fn delete_push_notification_config_through_dispatch() {
		let handler = PushNotificationHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_DELETE_TASK_PUSH_NOTIFICATION_CONFIG,
			serde_json::json!({
				"taskId": "task-push-5",
				"id": "cfg-to-delete"
			}),
			serde_json::json!("push-req-5"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(resp.is_success());
				let value = resp.into_result().unwrap();
				assert!(value.is_null(), "delete returns null on success");
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// The request ID must be passed through to the stream result
	// so the transport layer can include it in error framing or
	// final response envelopes.
	#[tokio::test]
	async fn stream_dispatch_preserves_request_id() {
		let handler = TaskHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SEND_STREAMING_MESSAGE,
			serde_json::json!({
				"message": {
					"messageId": "stream-id-test-1",
					"role": "user",
					"parts": [{"text": "test"}]
				}
			}),
			serde_json::json!(999),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Stream { id, .. } => {
				assert_eq!(id, serde_json::json!(999));
			}
			DispatchResult::Response(_) => panic!("expected Stream"),
		}
	}

	// The default handler rejects SubscribeToTask with
	// UnsupportedOperation, consistent with how it rejects all
	// other streaming operations. This ensures agents that don't
	// implement resubscribe return a proper error rather than
	// silently succeeding with an empty stream.
	#[tokio::test]
	async fn default_handler_rejects_resubscribe() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SUBSCRIBE_TO_TASK,
			serde_json::json!({
				"id": "task-resub-1"
			}),
			serde_json::json!("resub-req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::UNSUPPORTED_OPERATION);
				assert!(
					err.message.contains("SubscribeToTask"),
					"error should mention the operation name"
				);
			}
			DispatchResult::Stream { .. } => {
				panic!("default handler should not return a stream for resubscribe")
			}
		}
	}

	// Resubscribing to a task through the dispatch pipeline
	// exercises the full streaming path for SubscribeToTask:
	// JSON-RPC envelope → SubscribeToTaskParams deserialisation →
	// handler call → EventStream. The handler replays the task's
	// final completed event, simulating the typical reconnection
	// scenario where the caller missed the completion while
	// disconnected.
	#[tokio::test]
	async fn resubscribe_returns_event_stream() {
		use futures_util::StreamExt;

		let handler = TaskHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SUBSCRIBE_TO_TASK,
			serde_json::json!({
				"id": "task-resub-2"
			}),
			serde_json::json!("resub-req-2"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Stream { id, mut events } => {
				assert_eq!(id, serde_json::json!("resub-req-2"));

				// The handler replays a single completed status event
				// for the resubscribed task. The task ID should match
				// the one from the request params.
				let event = events.next().await.unwrap().unwrap();
				match event {
					StreamResponse::StatusUpdate(update) => {
						assert_eq!(update.task_id, "task-resub-2");
						assert_eq!(update.status.state, TaskState::Completed);
					}
					_ => panic!("expected StatusUpdate event from resubscribe"),
				}

				// Stream should be exhausted after the single replayed
				// status—the task already completed so there are no
				// further live updates to deliver.
				assert!(events.next().await.is_none());
			}
			DispatchResult::Response(_) => panic!("expected Stream for resubscribe"),
		}
	}

	// The resubscribe dispatch must pass the request ID through to
	// the DispatchResult::Stream variant, just like SendStreamingMessage
	// does. The transport layer needs this ID for error framing and
	// final response envelopes on the SSE connection.
	#[tokio::test]
	async fn resubscribe_preserves_request_id() {
		let handler = TaskHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_SUBSCRIBE_TO_TASK,
			serde_json::json!({
				"id": "task-resub-3"
			}),
			serde_json::json!(7777),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Stream { id, .. } => {
				assert_eq!(id, serde_json::json!(7777));
			}
			DispatchResult::Response(_) => panic!("expected Stream for resubscribe"),
		}
	}

	// A handler that implements GetExtendedAgentCard.
	// The extended card includes additional skills and security
	// schemes that the public card omits—this models the common
	// pattern where sensitive or premium capabilities are only
	// revealed to authenticated callers.
	//
	// The handler returns a static card with one extra skill
	// ("confidential-analysis") and a security scheme definition.
	// A real agent would check the caller's credentials from the
	// transport layer before deciding which extra fields to include.
	struct ExtendedCardHandler;
	impl A2AHandler for ExtendedCardHandler {
		fn get_extended_agent_card(
			&self,
			_context: &RequestContext,
			_params: GetExtendedAgentCardParams,
		) -> impl std::future::Future<Output = Result<AgentCard, a2a::error::A2AError>> + Send + '_
		{
			async move {
				let card = AgentCard::new(AgentCardRequired {
					name: "Extended Agent".into(),
					description: "Agent with authenticated extended card".into(),
					supported_interfaces: vec![AgentInterface::new(
						"https://agent.example.com",
						"JSONRPC",
						"1.0",
					)],
					version: "2.0".into(),
					capabilities: AgentCapabilities::default()
						.with_streaming(true)
						.with_push_notifications(true)
						.with_extended_agent_card(true),
					skills: vec![
						AgentSkill::new(
							"public-skill",
							"Public Skill",
							"Available to all callers",
							vec!["general".into()],
						),
						AgentSkill::new(
							"confidential-analysis",
							"Confidential Analysis",
							"Only visible to authenticated callers",
							vec!["premium".into(), "analysis".into()],
						),
					],
					default_input_modes: vec!["text/plain".into()],
					default_output_modes: vec!["text/plain".into()],
				});

				Ok(card)
			}
		}
	}

	// The default handler rejects GetExtendedAgentCard with
	// UnsupportedOperation. Agents that don't offer an extended
	// card (the majority) should return this error so callers know
	// to fall back to the public card from /.well-known/agent.json.
	#[tokio::test]
	async fn default_handler_rejects_extended_card() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_GET_EXTENDED_AGENT_CARD,
			serde_json::json!({}),
			serde_json::json!("ext-req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::UNSUPPORTED_OPERATION);
				assert!(
					err.message.contains("GetExtendedAgentCard"),
					"error should mention the operation name"
				);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Fetching the extended card through the dispatch pipeline
	// exercises the full path: JSON-RPC envelope →
	// GetExtendedCardParams deserialisation → handler call →
	// AgentCard serialisation → JSON-RPC success response. The
	// returned card should include the extra skills and capabilities
	// that the public card omits.
	#[tokio::test]
	async fn extended_card_returns_full_agent_card() {
		let handler = ExtendedCardHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_GET_EXTENDED_AGENT_CARD,
			serde_json::json!({}),
			serde_json::json!("ext-req-2"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(resp.is_success());
				let card: AgentCard = serde_json::from_value(resp.into_result().unwrap()).unwrap();
				assert_eq!(card.name, "Extended Agent");
				assert_eq!(card.version, "2.0");
				assert_eq!(card.capabilities.extended_agent_card, Some(true));

				// The extended card should have both the public and
				// confidential skills.
				assert_eq!(card.skills.len(), 2);
				assert_eq!(card.skills[0].id, "public-skill");
				assert_eq!(card.skills[1].id, "confidential-analysis");

				// Capabilities should reflect the full feature set.
				assert_eq!(card.capabilities.streaming, Some(true));
				assert_eq!(card.capabilities.push_notifications, Some(true));
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// The extended card request accepts optional metadata for
	// caller-defined context. Verify that the dispatch pipeline
	// correctly deserialises params with metadata without
	// interfering with the handler's response.
	#[tokio::test]
	async fn extended_card_accepts_metadata_in_params() {
		let handler = ExtendedCardHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_GET_EXTENDED_AGENT_CARD,
			serde_json::json!({
				"metadata": {"trace_id": "abc-123", "session": "xyz"}
			}),
			serde_json::json!("ext-req-3"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(resp.is_success());
				let card: AgentCard = serde_json::from_value(resp.into_result().unwrap()).unwrap();
				assert_eq!(card.name, "Extended Agent");
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// The default handler rejects ListTasks with UnsupportedOperation.
	// Agents that do not maintain a task store (stateless or single-task
	// agents) rely on this default so callers receive a clear error
	// rather than an empty list that might be misinterpreted as "no tasks".
	#[tokio::test]
	async fn default_handler_rejects_list_tasks() {
		let handler = DefaultHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_LIST_TASKS,
			serde_json::json!({}),
			serde_json::json!("list-req-1"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!("list-req-1"));
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::UNSUPPORTED_OPERATION);
				assert!(
					err.message.contains("ListTasks"),
					"error should mention the operation name"
				);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// A handler that implements list_tasks returns a paginated task
	// list. This exercises the full dispatch path for ListTasks:
	// JSON-RPC envelope → ListTasksParams deserialisation → handler
	// call → ListTasksResponse serialisation → JSON-RPC success response.
	//
	// The implementation returns a fixed list of two tasks filtered by
	// context_id, simulating the most common listing pattern.
	struct ListTasksHandler;
	impl A2AHandler for ListTasksHandler {
		fn list_tasks(
			&self,
			_context: &RequestContext,
			params: ListTasksParams,
		) -> impl std::future::Future<Output = Result<ListTasksResponse, a2a::error::A2AError>> + Send + '_
		{
			async move {
				// Return two synthetic tasks in the requested context.
				// A real agent would query its task store and apply all
				// the filter and pagination parameters.
				let context = params.context_id.unwrap_or_else(|| "ctx-default".into());
				let task_a = a2a::task::Task::new(
					"listed-task-a",
					context.clone(),
					a2a::task::TaskStatus::submitted()
						.with_timestamp(datetime!(2026-03-24 08:00:00 UTC)),
				);
				let task_b = a2a::task::Task::new(
					"listed-task-b",
					context,
					a2a::task::TaskStatus::completed()
						.with_timestamp(datetime!(2026-03-24 09:00:00 UTC)),
				);
				Ok(ListTasksResponse {
					tasks: vec![task_a, task_b],
					page_size: 50,
					total_size: 2,
					next_page_token: String::new(),
				})
			}
		}
	}

	// Dispatching ListTasks with a context_id filter returns the
	// tasks belonging to that context. The response must include
	// the tasks and an empty next_page_token, indicating this
	// is the only page.
	#[tokio::test]
	async fn list_tasks_dispatch_returns_task_list() {
		let handler = ListTasksHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_LIST_TASKS,
			serde_json::json!({ "contextId": "ctx-list-dispatch-1" }),
			serde_json::json!("list-req-2"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!("list-req-2"));
				assert!(resp.is_success());

				let list_response: ListTasksResponse =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				assert_eq!(list_response.tasks.len(), 2);
				assert_eq!(list_response.tasks[0].id, "listed-task-a");
				assert_eq!(list_response.tasks[0].context_id, "ctx-list-dispatch-1");
				assert_eq!(list_response.tasks[1].id, "listed-task-b");
				assert!(
					list_response.next_page_token.is_empty(),
					"single page result must have an empty continuation token"
				);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// Every method constant defined in the operation module must have
	// a corresponding dispatch arm. A missing arm would silently return
	// METHOD_NOT_FOUND for a valid operation, breaking the server for
	// that method. This test sends each of the 11 method names through
	// dispatch with empty params and asserts the error is NOT
	// METHOD_NOT_FOUND (it may be INVALID_PARAMS or UNSUPPORTED_OPERATION,
	// both of which prove the method was recognised and routed correctly).
	// Any new method constant added to operation/mod.rs without a matching
	// dispatch arm will cause this test to fail immediately.
	#[tokio::test]
	async fn all_method_constants_have_dispatch_arms() {
		let handler = DefaultHandler;
		let methods = [
			operation::METHOD_SEND_MESSAGE,
			operation::METHOD_SEND_STREAMING_MESSAGE,
			operation::METHOD_GET_TASK,
			operation::METHOD_LIST_TASKS,
			operation::METHOD_CANCEL_TASK,
			operation::METHOD_SUBSCRIBE_TO_TASK,
			operation::METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG,
			operation::METHOD_GET_TASK_PUSH_NOTIFICATION_CONFIG,
			operation::METHOD_LIST_TASK_PUSH_NOTIFICATION_CONFIGS,
			operation::METHOD_DELETE_TASK_PUSH_NOTIFICATION_CONFIG,
			operation::METHOD_GET_EXTENDED_AGENT_CARD,
		];

		for method in methods {
			let request =
				JsonRpcRequest::new(method, serde_json::json!({}), serde_json::json!("test"));
			let ctx = RequestContext::default();
			let result = dispatch(&handler, &ctx, request).await;
			match result {
				DispatchResult::Response(response) => {
					if let Some(err) = response.a2a_error() {
						assert_ne!(
							err.code,
							error::code::METHOD_NOT_FOUND,
							"method {method} has no dispatch arm"
						);
					}
				}
				DispatchResult::Stream { .. } => {
					// Streaming methods that successfully dispatched—this is fine.
				}
			}
		}
	}

	// ListTasks with no params must succeed—an empty params object is
	// valid and requests all tasks without filtering. Malformed params
	// (a non-object JSON value) must still return INVALID_PARAMS.
	#[tokio::test]
	async fn list_tasks_malformed_params_returns_invalid_params() {
		let handler = ListTasksHandler;
		let request = JsonRpcRequest::new(
			operation::METHOD_LIST_TASKS,
			serde_json::json!("not an object"),
			serde_json::json!("list-req-3"),
		);

		let ctx = RequestContext::default();
		let result = dispatch(&handler, &ctx, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert_eq!(resp.id, serde_json::json!("list-req-3"));
				let err = resp.a2a_error().expect("expected error").clone();
				assert_eq!(err.code, error::code::INVALID_PARAMS);
			}
			DispatchResult::Stream { .. } => panic!("expected Response, got Stream"),
		}
	}

	// A handler that reads a BearerToken from the context and reflects
	// it in the reply text. This is used to verify end-to-end that a
	// RequestContext with extension values is delivered intact to the
	// handler by the dispatch layer.
	//
	// The distinction from the axum router test is important: this test
	// exercises the dispatch layer in isolation—no HTTP, no headers—so
	// it confirms the RequestContext flows correctly from the dispatch
	// call down into the handler method without any transport involvement.
	struct ContextCapturingHandler;
	impl A2AHandler for ContextCapturingHandler {
		fn message_send(
			&self,
			context: &RequestContext,
			_params: SendMessageParams,
		) -> impl std::future::Future<Output = Result<SendMessageResult, a2a::error::A2AError>> + Send + '_
		{
			use crate::context::BearerToken;
			use a2a::role::Role;
			let token = context
				.extensions()
				.get::<BearerToken>()
				.map(|t| t.0.clone())
				.unwrap_or_else(|| "absent".into());
			async move {
				let reply = a2a::message::Message::text("ctx-reply", Role::Agent, token);
				Ok(SendMessageResult::Message(reply))
			}
		}
	}

	// Context values inserted before calling dispatch must reach the
	// handler unchanged. The dispatch layer must pass the context
	// through by reference without copying, dropping, or replacing
	// any entries in the extension map.
	//
	// The test constructs a RequestContext containing a BearerToken,
	// calls dispatch, and asserts that the token value the handler
	// read matches what was inserted before the call.
	#[tokio::test]
	async fn context_extension_flows_through_dispatch_to_handler() {
		use crate::context::BearerToken;

		let handler = ContextCapturingHandler;
		let mut context = RequestContext::default();
		context
			.extensions_mut()
			.insert(BearerToken("dispatch-test-token".into()));

		let request = JsonRpcRequest::new(
			operation::METHOD_SEND_MESSAGE,
			serde_json::json!({
				"message": {
					"messageId": "ctx-test-msg-1",
					"role": "user",
					"parts": [{"text": "check context"}]
				}
			}),
			serde_json::json!("ctx-req-1"),
		);

		let result = dispatch(&handler, &context, request).await;
		match result {
			DispatchResult::Response(resp) => {
				assert!(
					resp.is_success(),
					"dispatch with a valid context must produce a success response"
				);
				let send_result: SendMessageResult =
					serde_json::from_value(resp.into_result().unwrap()).unwrap();
				match send_result {
					SendMessageResult::Message(message) => {
						// The handler echoes the token value as the reply text.
						// "absent" means the context's extension map was empty
						// when the handler ran—i.e., the token was not delivered.
						assert_eq!(
							message.parts.first().unwrap().to_string(),
							"dispatch-test-token",
							"BearerToken inserted before dispatch must reach the handler"
						);
					}
					_ => panic!("expected Message variant"),
				}
			}
			DispatchResult::Stream { .. } => panic!("expected Response"),
		}
	}
}
