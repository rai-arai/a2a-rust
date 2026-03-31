// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-RPC dispatch layer for the A2A protocol.
//!
//! The dispatcher sits between the transport (HTTP, WebSocket, etc.)
//! and the handler trait. It receives raw `JsonRpcRequest` envelopes,
//! routes them to the correct handler method based on the "method"
//! field, deserialises the params into typed structs, and wraps the
//! handler's result into a `JsonRpcResponse` envelope.
//!
//! This separation means handler implementations never touch JSON-RPC
//! concerns—they work with typed Rust structs and return typed
//! results. The dispatcher handles all the envelope plumbing.
//!
//! Streaming operations (`SendStreamingMessage`, `SubscribeToTask`) are
//! handled separately from request-response operations because they
//! return a Stream rather than a single value. The dispatch function
//! returns a `DispatchResult` enum that the transport layer matches on
//! to decide whether to send a single response or start an SSE stream.

use crate::error::A2AError;
use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use crate::operation;

use super::context::RequestContext;
use super::handler::{A2AHandler, EventStream};

/// The result of dispatching a JSON-RPC request.
///
/// The transport layer (axum, actix, etc.) matches on this to decide
/// how to send the response. Single responses become regular HTTP
/// responses; streams become SSE event streams.
///
/// Errors during dispatch (unknown method, malformed params) are
/// already wrapped into JsonRpcResponse—the transport layer just
/// sends them. Handler-level errors are also wrapped—the transport
/// never needs to construct error responses itself.
pub enum DispatchResult<'a> {
	/// A single JSON-RPC response (success or error).
	/// Covers all request-response operations: `SendMessage`,
	/// `GetTask`, `ListTasks`, `CancelTask`, and all push notification
	/// operations.
	Response(JsonRpcResponse),

	/// A stream of events for SSE-based operations.
	/// Covers `SendStreamingMessage` and `SubscribeToTask`. The transport
	/// layer converts each `StreamEvent` into an SSE data frame.
	/// The id is the request ID from the original JSON-RPC request,
	/// needed for the final response or error framing.
	Stream {
		id: serde_json::Value,
		events: EventStream<'a>,
	},
}

/// Whether a method name corresponds to a streaming operation.
///
/// Used by transport integrations to set up the response format
/// (SSE vs regular JSON) before dispatch completes. The method
/// names are checked against the two streaming operations defined
/// in the A2A spec: `SendStreamingMessage` and `SubscribeToTask`.
#[must_use]
pub fn is_streaming_method(method: &str) -> bool {
	matches!(
		method,
		operation::METHOD_SEND_STREAMING_MESSAGE | operation::METHOD_SUBSCRIBE_TO_TASK
	)
}

/// Dispatch a JSON-RPC request to the appropriate handler method.
///
/// This is the main entry point for the server feature. The transport
/// layer calls this with each incoming request and handles the
/// `DispatchResult` appropriately (single HTTP response or SSE stream).
///
/// Unknown methods and deserialisation failures are returned as
/// JSON-RPC error responses—the transport layer never needs to
/// handle dispatch errors specially.
pub async fn dispatch<'a>(
	handler: &'a impl A2AHandler,
	context: &'a RequestContext,
	request: JsonRpcRequest,
) -> DispatchResult<'a> {
	let id = request.id.clone();
	let raw_params = request.params;

	match request.method.as_str() {
		operation::METHOD_SEND_MESSAGE => {
			dispatch_request_response(&id, raw_params, |params| {
				handler.message_send(context, params)
			})
			.await
		}
		operation::METHOD_SEND_STREAMING_MESSAGE => {
			dispatch_stream(&id, raw_params, |params| {
				handler.message_stream(context, params)
			})
			.await
		}
		operation::METHOD_GET_TASK => {
			dispatch_request_response(&id, raw_params, |params| handler.get_task(context, params))
				.await
		}
		operation::METHOD_LIST_TASKS => {
			dispatch_request_response(&id, raw_params, |params| {
				handler.list_tasks(context, params)
			})
			.await
		}
		operation::METHOD_CANCEL_TASK => {
			dispatch_request_response(&id, raw_params, |params| {
				handler.cancel_task(context, params)
			})
			.await
		}
		operation::METHOD_SUBSCRIBE_TO_TASK => {
			dispatch_stream(&id, raw_params, |params| {
				handler.resubscribe_task(context, params)
			})
			.await
		}
		_ => dispatch_push_notification(handler, context, &request.method, &id, raw_params).await,
	}
}

/// Dispatch push notification and extended card operations.
async fn dispatch_push_notification<'a>(
	handler: &'a impl A2AHandler,
	context: &'a RequestContext,
	method: &str,
	id: &serde_json::Value,
	raw_params: Option<serde_json::Value>,
) -> DispatchResult<'a> {
	match method {
		operation::METHOD_CREATE_TASK_PUSH_NOTIFICATION_CONFIG => {
			dispatch_request_response(id, raw_params, |params| {
				handler.set_push_notification_config(context, params)
			})
			.await
		}
		operation::METHOD_GET_TASK_PUSH_NOTIFICATION_CONFIG => {
			dispatch_request_response(id, raw_params, |params| {
				handler.get_push_notification_config(context, params)
			})
			.await
		}
		operation::METHOD_LIST_TASK_PUSH_NOTIFICATION_CONFIGS => {
			dispatch_request_response(id, raw_params, |params| {
				handler.list_push_notification_configs(context, params)
			})
			.await
		}
		operation::METHOD_DELETE_TASK_PUSH_NOTIFICATION_CONFIG => {
			dispatch_request_response(id, raw_params, |params| {
				handler.delete_push_notification_config(context, params)
			})
			.await
		}
		operation::METHOD_GET_EXTENDED_AGENT_CARD => {
			dispatch_request_response(id, raw_params, |params| {
				handler.get_extended_agent_card(context, params)
			})
			.await
		}
		_ => DispatchResult::Response(JsonRpcResponse::error(
			A2AError::method_not_found(method),
			id.clone(),
		)),
	}
}

/// Dispatch a request-response operation.
///
/// Deserialises the params from the JSON-RPC request, calls the
/// handler function, and wraps the result into a `JsonRpcResponse`.
/// Deserialisation failures become JSON-RPC invalid-params errors.
async fn dispatch_request_response<Params, Result, HandlerFn, HandlerFuture>(
	id: &serde_json::Value,
	raw_params: Option<serde_json::Value>,
	handler_fn: HandlerFn,
) -> DispatchResult<'static>
where
	Params: serde::de::DeserializeOwned,
	Result: serde::Serialize,
	HandlerFn: FnOnce(Params) -> HandlerFuture,
	HandlerFuture: std::future::Future<Output = std::result::Result<Result, A2AError>>,
{
	let params_value = raw_params.unwrap_or(serde_json::Value::Object(serde_json::Map::default()));
	let typed_params: Params = match serde_json::from_value(params_value) {
		Ok(parsed) => parsed,
		Err(error) => {
			return DispatchResult::Response(JsonRpcResponse::error(
				A2AError::invalid_params(error),
				id.clone(),
			));
		}
	};

	match handler_fn(typed_params).await {
		Ok(result) => match serde_json::to_value(result) {
			Ok(result_value) => {
				DispatchResult::Response(JsonRpcResponse::success(result_value, id.clone()))
			}
			Err(error) => DispatchResult::Response(JsonRpcResponse::error(
				A2AError::internal_error(error),
				id.clone(),
			)),
		},
		Err(err) => DispatchResult::Response(JsonRpcResponse::error(err, id.clone())),
	}
}

/// Dispatch a streaming operation.
///
/// Deserialises the params and calls the handler function, which
/// returns a Stream instead of a single value. Deserialisation
/// failures are returned as a single-response error.
async fn dispatch_stream<'a, Params, HandlerFn, HandlerFuture>(
	id: &serde_json::Value,
	raw_params: Option<serde_json::Value>,
	handler_fn: HandlerFn,
) -> DispatchResult<'a>
where
	Params: serde::de::DeserializeOwned,
	HandlerFn: FnOnce(Params) -> HandlerFuture,
	HandlerFuture: std::future::Future<Output = std::result::Result<EventStream<'a>, A2AError>>,
{
	let params_value = raw_params.unwrap_or(serde_json::Value::Object(serde_json::Map::default()));
	let typed_params: Params = match serde_json::from_value(params_value) {
		Ok(parsed) => parsed,
		Err(error) => {
			return DispatchResult::Response(JsonRpcResponse::error(
				A2AError::invalid_params(error),
				id.clone(),
			));
		}
	};

	match handler_fn(typed_params).await {
		Ok(events) => DispatchResult::Stream {
			id: id.clone(),
			events,
		},
		Err(err) => DispatchResult::Response(JsonRpcResponse::error(err, id.clone())),
	}
}
