// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The A2A JSON-RPC client.
//!
//! `A2AClient` wraps a `reqwest::Client` and an agent's JSON-RPC endpoint
//! URL. It provides typed methods for every A2A operation, handling
//! JSON-RPC envelope construction, request serialisation, response
//! deserialisation, and error extraction.
//!
//! The client does not perform agent discovery—callers should use
//! `discover_agent()` first to get the `AgentCard`, then construct the
//! client with the agent's endpoint URL from the card.
//!
//! Request IDs are generated as random UUIDs to avoid collisions in
//! concurrent use. The client is stateless—it holds no conversation
//! or task state. Callers manage their own task IDs and context IDs.

use std::sync::atomic::{AtomicU64, Ordering};

use a2a::agent_card::AgentCard;
use a2a::error::A2AError;
use a2a::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use a2a::message::Message;
use a2a::operation::{
	self, CancelTaskParams, DeleteTaskPushNotificationConfigParams, GetExtendedAgentCardParams,
	GetTaskParams, GetTaskPushNotificationConfigParams, ListTaskPushNotificationConfigsParams,
	ListTaskPushNotificationConfigsResponse, ListTasksParams, ListTasksResponse, SendMessageParams,
	SendMessageResult, SubscribeToTaskParams, TaskPushNotificationConfig,
};
use a2a::role::Role;
use a2a::task::Task;

/// Categorise a reqwest error without including the request URL.
///
/// `reqwest::Error`'s `Display` can include the full URL, which may
/// contain sensitive path components or query parameters. This function
/// describes the error category (timeout, connection, TLS, etc.)
/// without forwarding the URL to callers who might surface it in
/// their own error responses.
fn categorise_http_error(error: &reqwest::Error) -> String {
	if error.is_timeout() {
		"request timed out".into()
	} else if error.is_connect() {
		"connection failed".into()
	} else if error.is_request() {
		"request construction failed".into()
	} else if error.is_redirect() {
		"too many redirects".into()
	} else if error.is_body() {
		"request body error".into()
	} else if error.is_decode() {
		"response decoding failed".into()
	} else {
		"HTTP request failed".into()
	}
}

use crate::sse;

/// A stream of typed A2A events from a streaming operation.
///
/// Each item is either a successfully parsed `StreamResponse` or an
/// `A2AError` representing a parse failure or transport error. The
/// stream terminates when the server closes the SSE connection.
pub type ClientEventStream = std::pin::Pin<
	Box<
		dyn futures_core::Stream<
				Item = Result<a2a::stream_event::StreamResponse, a2a::error::A2AError>,
			> + Send,
	>,
>;

/// An HTTP client for calling a remote A2A agent.
///
/// Wraps a `reqwest::Client` and the agent's JSON-RPC endpoint URL.
/// All methods construct JSON-RPC request envelopes, send them via
/// HTTP POST, and parse the response envelopes. Streaming methods
/// return a Stream of events instead of a single response.
///
/// The client is cheaply cloneable (`reqwest::Client` uses an Arc
/// internally, and the URL is shared via Arc).
#[derive(Clone)]
pub struct A2AClient {
	/// The underlying HTTP client. Shared across clones.
	http: reqwest::Client,

	/// The agent's JSON-RPC endpoint URL.
	/// All requests are POST'd to this URL.
	endpoint: String,

	/// Monotonically increasing request ID counter.
	/// Shared across clones via Arc<AtomicU64> so each request
	/// gets a unique ID even when the client is used concurrently.
	next_id: std::sync::Arc<AtomicU64>,
}

impl A2AClient {
	/// Create a new client for the given agent endpoint URL.
	///
	/// The endpoint should be the agent's JSON-RPC URL (typically
	/// the base URL, though some agents use a different path).
	/// The `reqwest::Client` is provided by the caller so they can
	/// configure timeouts, TLS, proxies, etc.
	#[must_use]
	pub fn new(http: reqwest::Client, endpoint: impl Into<String>) -> Self {
		Self {
			http,
			endpoint: endpoint.into(),
			next_id: std::sync::Arc::new(AtomicU64::new(1)),
		}
	}

	/// Create a client from a discovered agent card.
	///
	/// Searches the card's `supported_interfaces` list for the first
	/// entry whose `protocol_binding` contains "jsonrpc" (case-insensitive).
	/// Returns `Err` if no JSON-RPC interface is found—callers must handle
	/// this when working with agents that only advertise non-JSON-RPC bindings.
	///
	/// This is the natural follow-up after `discover_agent()`—fetch
	/// the card, inspect the agent's capabilities, then create a
	/// client for the agent's JSON-RPC endpoint:
	///
	/// ```no_run
	/// # use a2a_client::{A2AClient, discover_agent};
	/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
	/// let http = reqwest::Client::new();
	/// let card = discover_agent(&http, "https://agent.example.com").await?;
	/// let client = A2AClient::from_agent_card(http, &card)?;
	/// # Ok(())
	/// # }
	/// ```
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the agent card contains no JSON-RPC interface.
	pub fn from_agent_card(http: reqwest::Client, card: &AgentCard) -> Result<Self, A2AError> {
		let endpoint = card
			.supported_interfaces
			.iter()
			.find(|iface| {
				iface
					.protocol_binding
					.to_ascii_lowercase()
					.contains("jsonrpc")
			})
			.ok_or_else(|| A2AError::transport("agent card has no JSON-RPC interface"))?
			.url
			.clone();
		Ok(Self::new(http, endpoint))
	}

	/// Generate the next unique request ID.
	fn next_request_id(&self) -> serde_json::Value {
		let id = self.next_id.fetch_add(1, Ordering::Relaxed);
		serde_json::Value::Number(id.into())
	}

	/// Send a JSON-RPC request and parse the response.
	///
	/// Handles the full request-response cycle: serialise the
	/// request envelope, POST it to the endpoint, parse the
	/// response envelope, and extract either the result or error.
	async fn rpc_call<P: serde::Serialize, R: serde::de::DeserializeOwned>(
		&self,
		method: &str,
		params: &P,
	) -> Result<R, A2AError> {
		let params_value = serde_json::to_value(params)
			.map_err(|e| A2AError::transport(format!("failed to serialise params: {e}")))?;

		let id = self.next_request_id();
		let request = JsonRpcRequest::new(method, params_value, id);

		let response = self
			.http
			.post(&self.endpoint)
			.json(&request)
			.send()
			.await
			.map_err(|error| A2AError::transport(categorise_http_error(&error)))?;

		if !response.status().is_success() {
			return Err(A2AError::transport(format!(
				"HTTP {} from agent",
				response.status()
			)));
		}

		let rpc_response: JsonRpcResponse = response
			.json()
			.await
			.map_err(|e| A2AError::transport(format!("failed to parse JSON-RPC response: {e}")))?;

		let result_value = rpc_response.into_result()?;

		serde_json::from_value(result_value)
			.map_err(|e| A2AError::transport(format!("failed to deserialise result: {e}")))
	}

	/// Send a JSON-RPC request and return an SSE event stream.
	///
	/// Used for streaming operations (`SendStreamingMessage`, `SubscribeToTask`).
	/// The HTTP response is expected to be an SSE stream rather than
	/// a single JSON-RPC response.
	async fn rpc_stream<P: serde::Serialize>(
		&self,
		method: &str,
		params: &P,
	) -> Result<ClientEventStream, A2AError> {
		let params_value = serde_json::to_value(params)
			.map_err(|e| A2AError::transport(format!("failed to serialise params: {e}")))?;

		let id = self.next_request_id();
		let request = JsonRpcRequest::new(method, params_value, id);

		let response = self
			.http
			.post(&self.endpoint)
			.json(&request)
			.send()
			.await
			.map_err(|error| A2AError::transport(categorise_http_error(&error)))?;

		if !response.status().is_success() {
			return Err(A2AError::transport(format!(
				"HTTP {} from agent",
				response.status()
			)));
		}

		Ok(sse::into_event_stream(response))
	}

	/// Send a message to the agent (`SendMessage`).
	///
	/// The agent processes the message and returns either a Task
	/// (for deferred work) or a Message (for immediate replies).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn message_send(
		&self,
		params: &SendMessageParams,
	) -> Result<SendMessageResult, A2AError> {
		self.rpc_call(operation::METHOD_SEND_MESSAGE, params).await
	}

	/// Send a message with SSE streaming (`SendStreamingMessage`).
	///
	/// Returns a stream of events as the agent processes the message.
	/// The stream emits status updates, artifact updates, and
	/// eventually the final result.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the server returns
	/// a non-success status code before the stream begins.
	pub async fn message_stream(
		&self,
		params: &SendMessageParams,
	) -> Result<ClientEventStream, A2AError> {
		self.rpc_stream(operation::METHOD_SEND_STREAMING_MESSAGE, params)
			.await
	}

	/// Retrieve a task by ID (`GetTask`).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn get_task(&self, params: &GetTaskParams) -> Result<Task, A2AError> {
		self.rpc_call(operation::METHOD_GET_TASK, params).await
	}

	/// List tasks with optional filters (`ListTasks`).
	///
	/// Returns a paginated response containing tasks that match the
	/// supplied filter criteria. All filter fields are optional—pass
	/// `&ListTasksParams::new()` to retrieve the first page of all
	/// tasks without filtering.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn list_tasks(
		&self,
		params: &ListTasksParams,
	) -> Result<ListTasksResponse, A2AError> {
		self.rpc_call(operation::METHOD_LIST_TASKS, params).await
	}

	/// Cancel a running task (`CancelTask`).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn cancel_task(&self, params: &CancelTaskParams) -> Result<Task, A2AError> {
		self.rpc_call(operation::METHOD_CANCEL_TASK, params).await
	}

	/// Resubscribe to a task's event stream (`SubscribeToTask`).
	///
	/// Reconnects to an existing task's SSE stream after a dropped
	/// connection. Returns the same type of stream as `message_stream`.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the server returns
	/// a non-success status code before the stream begins.
	pub async fn resubscribe_task(
		&self,
		params: &SubscribeToTaskParams,
	) -> Result<ClientEventStream, A2AError> {
		self.rpc_stream(operation::METHOD_SUBSCRIBE_TO_TASK, params)
			.await
	}

	/// Set a push notification config on a task
	/// (`CreateTaskPushNotificationConfig`).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn set_push_notification_config(
		&self,
		params: &TaskPushNotificationConfig,
	) -> Result<TaskPushNotificationConfig, A2AError> {
		self.rpc_call(
			operation::METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG,
			params,
		)
		.await
	}

	/// Get a push notification config for a task
	/// (`GetTaskPushNotificationConfig`).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn get_push_notification_config(
		&self,
		params: &GetTaskPushNotificationConfigParams,
	) -> Result<TaskPushNotificationConfig, A2AError> {
		self.rpc_call(operation::METHOD_GET_TASK_PUSH_NOTIFICATION_CONFIG, params)
			.await
	}

	/// List push notification configs for a task
	/// (`ListTaskPushNotificationConfigs`).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn list_push_notification_configs(
		&self,
		params: &ListTaskPushNotificationConfigsParams,
	) -> Result<ListTaskPushNotificationConfigsResponse, A2AError> {
		self.rpc_call(
			operation::METHOD_LIST_TASK_PUSH_NOTIFICATION_CONFIGS,
			params,
		)
		.await
	}

	/// Delete a push notification config from a task
	/// (`DeleteTaskPushNotificationConfig`).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn delete_push_notification_config(
		&self,
		params: &DeleteTaskPushNotificationConfigParams,
	) -> Result<(), A2AError> {
		self.rpc_call(
			operation::METHOD_DELETE_TASK_PUSH_NOTIFICATION_CONFIG,
			params,
		)
		.await
	}

	/// Get the agent's extended card (`GetExtendedAgentCard`).
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn get_extended_agent_card(
		&self,
		params: &GetExtendedAgentCardParams,
	) -> Result<AgentCard, A2AError> {
		self.rpc_call(operation::METHOD_GET_EXTENDED_AGENT_CARD, params)
			.await
	}

	/// Send a plain text message to the agent and get the result.
	///
	/// This is the simplest way to interact with an A2A agent—just
	/// provide a message ID and the text. The message is sent as a
	/// user role with a single text part, no configuration, and no
	/// metadata. For more control, use `message_send()` directly
	/// with a manually constructed `SendMessageParams`.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn send_text(
		&self,
		message_id: impl Into<String>,
		text: impl Into<String>,
	) -> Result<SendMessageResult, A2AError> {
		let params = SendMessageParams::new(Message::text(message_id, Role::User, text));
		self.message_send(&params).await
	}

	/// Send a plain text message and stream the response.
	///
	/// Like `send_text()` but returns an SSE event stream instead
	/// of a single result. Use this when you want real-time status
	/// updates and artifact delivery as the agent works.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the server returns
	/// a non-success status code before the stream begins.
	pub async fn stream_text(
		&self,
		message_id: impl Into<String>,
		text: impl Into<String>,
	) -> Result<ClientEventStream, A2AError> {
		let params = SendMessageParams::new(Message::text(message_id, Role::User, text));
		self.message_stream(&params).await
	}

	/// Retrieve a task by its ID.
	///
	/// Convenience wrapper around `get_task()` that constructs the
	/// `GetTaskParams` for you. Returns the task without conversation
	/// history—use `get_task()` directly with `GetTaskParams` to
	/// request history with a specific length limit, or chain
	/// `with_tenant()` for multi-tenant deployments.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn get_task_by_id(&self, task_id: impl Into<String>) -> Result<Task, A2AError> {
		let params = GetTaskParams::new(task_id);
		self.get_task(&params).await
	}

	/// List all tasks belonging to a context.
	///
	/// Convenience wrapper around `list_tasks()` that constructs the
	/// `ListTasksParams` with only the `context_id` filter set. Returns
	/// the first page of all tasks in that context. For paginated access,
	/// filtering by status, or controlling page size, use `list_tasks()`
	/// directly with a manually constructed `ListTasksParams`.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn list_tasks_by_context(
		&self,
		context_id: impl Into<String>,
	) -> Result<ListTasksResponse, A2AError> {
		let params = ListTasksParams::new().with_context_id(context_id);
		self.list_tasks(&params).await
	}

	/// Cancel a task by its ID.
	///
	/// Convenience wrapper around `cancel_task()` that constructs
	/// the `CancelTaskParams` for you. Returns the updated task with
	/// its status set to canceled. For cancellations that need
	/// metadata or a tenant, use `cancel_task()` directly with a
	/// manually constructed `CancelTaskParams`.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the agent returns
	/// a JSON-RPC error response.
	pub async fn cancel_task_by_id(&self, task_id: impl Into<String>) -> Result<Task, A2AError> {
		let params = CancelTaskParams::new(task_id);
		self.cancel_task(&params).await
	}

	/// Resubscribe to a task's SSE event stream by its ID.
	///
	/// Convenience wrapper around `resubscribe_task()` that constructs
	/// the `SubscribeToTaskParams` for you. This is the typical recovery
	/// path after a dropped SSE connection—the caller reconnects to
	/// the same task without resending the original message. The agent
	/// replays any events the caller may have missed, then continues
	/// streaming live updates.
	///
	/// For resubscribe requests that need a tenant identifier, use
	/// `resubscribe_task()` directly with a manually constructed
	/// `SubscribeToTaskParams` and chain `with_tenant()`.
	///
	/// # Errors
	///
	/// Returns an [`A2AError`] if the HTTP request fails or the server returns
	/// a non-success status code before the stream begins.
	pub async fn resubscribe_by_id(
		&self,
		task_id: impl Into<String>,
	) -> Result<ClientEventStream, A2AError> {
		let params = SubscribeToTaskParams::new(task_id);
		self.resubscribe_task(&params).await
	}
}
