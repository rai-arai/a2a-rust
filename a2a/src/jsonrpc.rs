// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-RPC 2.0 envelope types for the A2A protocol.
//!
//! The A2A protocol uses JSON-RPC 2.0 as its primary transport binding.
//! Every operation is a JSON-RPC request with a method name (e.g.
//! "`SendMessage`", "`GetTask`") and typed parameters. Responses carry
//! either a result or an error, identified by the request's ID.
//!
//! These envelope types are deliberately kept separate from the domain
//! types (Task, Message, etc.) because they're transport concerns. The
//! domain types don't know or care about JSON-RPC—they just serialise
//! to `serde_json::Value`, which gets wrapped in these envelopes for
//! transmission.
//!
//! The jsonrpc field is always "2.0" per the spec. We enforce this on
//! serialisation. Deserialisation accepts any value in the jsonrpc field
//! to be a lenient receiver—callers should not rely on this field being
//! validated. The id field uses `serde_json::Value` because JSON-RPC allows
//! string, number, or null IDs—we don't constrain callers to a specific type.

use serde::{Deserialize, Serialize};

use crate::error::A2AError;

/// A JSON-RPC 2.0 request envelope.
///
/// Callers construct this to send operations to an A2A agent. The
/// method field identifies the operation (e.g. "`SendMessage`") and
/// params carries the operation-specific data as a JSON value.
///
/// The id field correlates the request with its response. It can be
/// a string, number, or null—we use `serde_json::Value` to support
/// all three without constraining the caller's ID generation strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
	/// Protocol version—always "2.0".
	pub jsonrpc: String,

	/// The operation to invoke (e.g. "`SendMessage`", "`GetTask`").
	pub method: String,

	/// Operation-specific parameters.
	/// The structure depends on the method—see the operation
	/// modules for typed parameter structs.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub params: Option<serde_json::Value>,

	/// Request identifier for correlating with the response.
	/// Can be a string, number, or null. The server echoes it
	/// back in the response unchanged.
	pub id: serde_json::Value,
}

/// A JSON-RPC 2.0 response envelope.
///
/// Agents return this for every non-streaming operation. Contains
/// either a result (success) or an error (failure), never both.
/// The id matches the request that triggered this response.
///
/// The result and error fields are `pub(crate)` to enforce the
/// JSON-RPC invariant that exactly one of them is present. Use the
/// `success()` and `error()` constructors to create responses, and
/// the accessor methods (`result()`, `error()`, `is_success()`,
/// `into_result()`) to inspect them. This prevents constructing
/// an invalid response with both result and error set, or neither.
///
/// Deserialisation from the wire is unrestricted—the invariant
/// is a construction-time guarantee, not a parsing-time one. Wire
/// payloads from other implementations may technically violate it,
/// and we accept them without error to be a lenient receiver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
	/// Protocol version—always "2.0".
	pub jsonrpc: String,

	/// The operation result on success.
	/// Present when the operation succeeded, absent on error.
	/// Access via `result()` or `into_result()`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) result: Option<serde_json::Value>,

	/// The error on failure.
	/// Present when the operation failed, absent on success.
	/// Access via `error()` or `into_result()`.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) error: Option<A2AError>,

	/// The request ID this response correlates to.
	/// Echoed from the request unchanged.
	pub id: serde_json::Value,
}

impl JsonRpcRequest {
	/// Create a new request with the given method, params, and ID.
	/// Sets jsonrpc to "2.0" automatically.
	#[must_use]
	pub fn new(
		method: impl Into<String>,
		params: serde_json::Value,
		id: serde_json::Value,
	) -> Self {
		Self {
			jsonrpc: "2.0".into(),
			method: method.into(),
			params: Some(params),
			id,
		}
	}

	/// Create a new request with no params.
	/// Sets jsonrpc to "2.0" automatically. The params field will be
	/// omitted from the wire format entirely.
	#[must_use]
	pub fn without_params(method: impl Into<String>, id: serde_json::Value) -> Self {
		Self {
			jsonrpc: "2.0".into(),
			method: method.into(),
			params: None,
			id,
		}
	}
}

impl JsonRpcResponse {
	/// Create a success response with the given result and ID.
	///
	/// Sets jsonrpc to "2.0", populates the result field, and leaves
	/// the error field as None. This is one of only two ways to
	/// construct a `JsonRpcResponse` (the other being `error()`),
	/// ensuring the JSON-RPC invariant that exactly one of result
	/// or error is present.
	#[must_use]
	pub fn success(result: serde_json::Value, id: serde_json::Value) -> Self {
		Self {
			jsonrpc: "2.0".into(),
			result: Some(result),
			error: None,
			id,
		}
	}

	/// Create an error response from an `A2AError` and the request ID.
	///
	/// Sets jsonrpc to "2.0", populates the error field, and leaves
	/// the result field as None. The `A2AError` carries the numeric
	/// code, human-readable message, and optional structured data
	/// that describe the failure.
	#[must_use]
	pub fn error(error: A2AError, id: serde_json::Value) -> Self {
		Self {
			jsonrpc: "2.0".into(),
			result: None,
			error: Some(error),
			id,
		}
	}

	/// Borrow the result value, if this is a success response.
	///
	/// Returns None for error responses. The returned value is the
	/// raw JSON—callers typically deserialise it into the expected
	/// operation result type (e.g. Task, Message, `TaskPushNotificationConfig`).
	#[must_use]
	pub fn result(&self) -> Option<&serde_json::Value> {
		self.result.as_ref()
	}

	/// Borrow the error, if this is an error response.
	///
	/// Returns None for success responses. The `A2AError` carries the
	/// numeric code (e.g. -32001 for task not found), a human-readable
	/// message, and optional structured diagnostic data.
	#[must_use]
	pub fn a2a_error(&self) -> Option<&A2AError> {
		self.error.as_ref()
	}

	/// Whether this response represents a successful operation.
	///
	/// A response is successful when it carries a result value.
	/// This is a convenience predicate for branching without
	/// destructuring—use `into_result()` when you need to
	/// consume the response and extract the value or error.
	#[must_use]
	pub fn is_success(&self) -> bool {
		self.result.is_some()
	}

	/// Consume the response into a Result, extracting either the
	/// success value or the `A2AError`.
	///
	/// Converts the JSON-RPC response envelope into Rust's standard
	/// Result type for ergonomic error handling with `?`. If both
	/// result and error are somehow present (possible from lenient
	/// wire deserialisation), the result takes precedence. If neither
	/// is present, returns an `A2AError` with a transport error code.
	///
	/// # Errors
	///
	/// Returns `Err` if the response contains a JSON-RPC error, or if
	/// the response contains neither a result nor an error.
	pub fn into_result(self) -> Result<serde_json::Value, A2AError> {
		if let Some(result) = self.result {
			Ok(result)
		} else if let Some(error) = self.error {
			Err(error)
		} else {
			Err(A2AError::transport(
				"response contains neither result nor error",
			))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::error;

	// A request must carry jsonrpc "2.0", the method name, params,
	// and an ID. This is the exact structure every A2A agent expects
	// to receive—getting any field wrong means the request is
	// rejected before the method even dispatches.
	#[test]
	fn request_wire_format() {
		let req = JsonRpcRequest::new(
			"SendMessage",
			serde_json::json!({"message": {"role": "user", "parts": []}}),
			serde_json::json!("req-1"),
		);

		let json = serde_json::to_value(&req).unwrap();
		assert_eq!(json["jsonrpc"], "2.0");
		assert_eq!(json["method"], "SendMessage");
		assert_eq!(json["id"], "req-1");
		assert!(json["params"].is_object());
	}

	// A success response carries the result and echoes the request ID.
	// The error field must be absent (not null) on success—the spec
	// requires exactly one of result or error to be present.
	#[test]
	fn success_response_wire_format() {
		let resp = JsonRpcResponse::success(
			serde_json::json!({"id": "task-1", "status": {"state": "submitted"}}),
			serde_json::json!("req-1"),
		);

		let json = serde_json::to_value(&resp).unwrap();
		assert_eq!(json["jsonrpc"], "2.0");
		assert_eq!(json["id"], "req-1");
		assert!(json["result"].is_object());
		assert!(
			!json.as_object().unwrap().contains_key("error"),
			"error must be absent on success"
		);
	}

	// An error response carries the A2AError and echoes the request ID.
	// The result field must be absent (not null) on error.
	#[test]
	fn error_response_wire_format() {
		let resp = JsonRpcResponse::error(
			A2AError::task_not_found("task-99"),
			serde_json::json!("req-2"),
		);

		let json = serde_json::to_value(&resp).unwrap();
		assert_eq!(json["jsonrpc"], "2.0");
		assert_eq!(json["id"], "req-2");
		assert_eq!(json["error"]["code"], error::code::TASK_NOT_FOUND);
		assert!(
			json["error"]["message"]
				.as_str()
				.unwrap()
				.contains("task-99")
		);
		assert!(
			!json.as_object().unwrap().contains_key("result"),
			"result must be absent on error"
		);
	}

	// JSON-RPC allows numeric IDs. This is common in automated
	// clients that use monotonically increasing integers. The ID
	// must round-trip as a number, not be coerced to a string.
	#[test]
	fn numeric_id_round_trips() {
		let req = JsonRpcRequest::new(
			"GetTask",
			serde_json::json!({"id": "task-1"}),
			serde_json::json!(42),
		);

		let json = serde_json::to_string(&req).unwrap();
		let back: JsonRpcRequest = serde_json::from_str(&json).unwrap();
		assert_eq!(back.id, serde_json::json!(42));
	}

	// The params field is optional—some operations (like
	// GetExtendedAgentCard) take no parameters. When None, params
	// must be omitted from the wire format entirely.
	#[test]
	fn request_without_params() {
		let req =
			JsonRpcRequest::without_params("GetExtendedAgentCard", serde_json::json!("req-3"));

		let json = serde_json::to_value(&req).unwrap();
		assert!(
			!json.as_object().unwrap().contains_key("params"),
			"params must be absent when None"
		);
	}

	// Round-trip both request and response through serde to confirm
	// the full envelope structure survives serialisation intact.
	// This is the fundamental correctness property for the transport
	// layer—if envelopes don't round-trip, nothing works.
	#[test]
	fn full_request_response_round_trip() {
		let req = JsonRpcRequest::new(
			"CancelTask",
			serde_json::json!({"id": "task-5"}),
			serde_json::json!("cancel-req"),
		);

		let req_json = serde_json::to_string(&req).unwrap();
		let req_back: JsonRpcRequest = serde_json::from_str(&req_json).unwrap();
		assert_eq!(req_back, req);

		let resp = JsonRpcResponse::success(
			serde_json::json!({"id": "task-5", "status": {"state": "canceled", "timestamp": "2026-03-12T18:00:00Z"}}),
			serde_json::json!("cancel-req"),
		);

		let resp_json = serde_json::to_string(&resp).unwrap();
		let resp_back: JsonRpcResponse = serde_json::from_str(&resp_json).unwrap();
		assert_eq!(resp_back, resp);
	}

	// Deserialise a request from wire JSON as it would arrive from
	// another A2A client implementation. This validates the inbound
	// parsing path with the exact structure the spec defines.
	#[test]
	fn deserialises_external_request() {
		let json = r#"{
            "jsonrpc": "2.0",
            "method": "SendMessage",
            "params": {
                "message": {
                    "role": "user",
                    "parts": [{"text": "hello"}]
                }
            },
            "id": "ext-1"
        }"#;

		let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
		assert_eq!(req.jsonrpc, "2.0");
		assert_eq!(req.method, "SendMessage");
		assert_eq!(req.id, serde_json::json!("ext-1"));
		assert!(req.params.is_some());
	}

	// The result() accessor returns a reference to the result value
	// on success responses and None on error responses.
	#[test]
	fn result_accessor_returns_value_on_success() {
		let response = JsonRpcResponse::success(
			serde_json::json!({"id": "task-1"}),
			serde_json::json!("request-1"),
		);

		assert!(response.result().is_some());
		assert_eq!(response.result().unwrap()["id"], "task-1");
		assert!(response.a2a_error().is_none());
	}

	// The a2a_error() accessor returns a reference to the error
	// on error responses and None on success responses.
	#[test]
	fn error_accessor_returns_error_on_failure() {
		let response = JsonRpcResponse::error(
			A2AError::task_not_found("task-missing"),
			serde_json::json!("request-2"),
		);

		assert!(response.a2a_error().is_some());
		assert_eq!(
			response.a2a_error().unwrap().code,
			error::code::TASK_NOT_FOUND
		);
		assert!(response.result().is_none());
	}

	// is_success() is true for success responses and false for error
	// responses. This is a convenience predicate for branching
	// without destructuring the response.
	#[test]
	fn is_success_reflects_response_type() {
		let success =
			JsonRpcResponse::success(serde_json::json!({}), serde_json::json!("request-3"));
		assert!(success.is_success());

		let failure = JsonRpcResponse::error(
			A2AError::task_not_found("task-1"),
			serde_json::json!("request-4"),
		);
		assert!(!failure.is_success());
	}

	// into_result() converts a success response into Ok(value),
	// consuming the response envelope.
	#[test]
	fn into_result_returns_ok_on_success() {
		let response = JsonRpcResponse::success(
			serde_json::json!({"status": "completed"}),
			serde_json::json!("request-5"),
		);

		let result = response.into_result();
		assert!(result.is_ok());
		assert_eq!(result.unwrap()["status"], "completed");
	}

	// into_result() converts an error response into Err(A2AError),
	// consuming the response envelope.
	#[test]
	fn into_result_returns_err_on_failure() {
		let response = JsonRpcResponse::error(
			A2AError::unsupported_operation("SendStreamingMessage"),
			serde_json::json!("request-6"),
		);

		let result = response.into_result();
		assert!(result.is_err());
		let error = result.unwrap_err();
		assert_eq!(error.code, error::code::UNSUPPORTED_OPERATION);
	}

	// into_result() returns a transport error when the response
	// contains neither result nor error. This can happen with
	// malformed wire payloads from lenient deserialisation.
	#[test]
	fn into_result_returns_transport_error_when_empty() {
		let response: JsonRpcResponse =
			serde_json::from_str(r#"{"jsonrpc": "2.0", "id": "request-7"}"#).unwrap();

		let result = response.into_result();
		assert!(result.is_err());
		let error = result.unwrap_err();
		assert_eq!(error.code, error::code::TRANSPORT_ERROR);
		assert!(error.message.contains("neither result nor error"));
	}
}
