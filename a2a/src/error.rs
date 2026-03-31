// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A2A protocol error codes and types.
//!
//! The protocol defines a set of domain-specific error codes on top of
//! the standard JSON-RPC 2.0 error range. These codes appear in the
//! "error" field of JSON-RPC responses when an operation fails.
//!
//! Standard JSON-RPC errors (-32700 to -32600) cover parse errors,
//! invalid requests, and method-not-found. The A2A-specific codes
//! (-32001 to -32005) cover domain situations like task not found,
//! unsupported content types, and missing capabilities.
//!
//! Error types here are pure data—they carry codes, messages, and
//! optional details. The actual JSON-RPC response envelope that wraps
//! these errors lives in the jsonrpc module.

use serde::{Deserialize, Serialize};

/// A2A-specific error codes.
///
/// These extend the standard JSON-RPC error code range with
/// domain-specific conditions. The numeric values are fixed by
/// the spec and must not change.
pub mod code {
	/// Standard JSON-RPC: the request payload could not be parsed as JSON.
	/// Returned before any method dispatch—the incoming bytes were not
	/// valid JSON and the request could not be interpreted at all.
	pub const PARSE_ERROR: i32 = -32700;

	/// Standard JSON-RPC: the request is not a valid Request object.
	/// Returned when the JSON parsed successfully but the resulting
	/// value does not conform to the JSON-RPC 2.0 Request structure
	/// (e.g. missing "jsonrpc" field, wrong type for "id", etc.).
	pub const INVALID_REQUEST: i32 = -32600;

	/// Standard JSON-RPC: internal error on the server.
	/// Returned when an unexpected condition prevents the server from
	/// fulfilling the request—for example, a serialisation failure
	/// when encoding the handler's return value into the response.
	pub const INTERNAL_ERROR: i32 = -32603;

	/// Standard JSON-RPC: invalid method parameters.
	/// Returned when the params field of a JSON-RPC request cannot
	/// be deserialised into the expected type for the method.
	pub const INVALID_PARAMS: i32 = -32602;

	/// Standard JSON-RPC: the method does not exist.
	/// Returned when the dispatcher receives a method name it
	/// doesn't recognise.
	pub const METHOD_NOT_FOUND: i32 = -32601;

	/// The referenced task does not exist.
	pub const TASK_NOT_FOUND: i32 = -32001;

	/// The task cannot be canceled in its current state.
	/// For example, a task that has already completed or failed
	/// cannot be canceled.
	pub const TASK_NOT_CANCELABLE: i32 = -32002;

	/// The agent does not support the content type in the request.
	/// Returned when a message contains parts with media types the
	/// agent's declared skills don't accept.
	pub const CONTENT_TYPE_NOT_SUPPORTED: i32 = -32003;

	/// The requested operation is not supported by this agent.
	/// Returned when a caller attempts streaming on an agent that
	/// doesn't declare streaming capability, for example.
	pub const UNSUPPORTED_OPERATION: i32 = -32004;

	/// Client-side transport error.
	/// Not part of the JSON-RPC or A2A spec—used by the client
	/// module to represent HTTP failures, connection errors, timeouts,
	/// and response parsing failures. Agents never return this code.
	/// The value is outside the JSON-RPC reserved range (-32768 to
	/// -32000) and the A2A domain range (-32001 to -32005) to avoid
	/// collisions with any spec-defined code.
	pub const TRANSPORT_ERROR: i32 = -1;

	/// Push notifications are not supported by this agent.
	/// Returned when a caller tries to create, get, or list push
	/// notification configs on an agent without that capability.
	pub const PUSH_NOTIFICATION_NOT_SUPPORTED: i32 = -32005;
}

/// An A2A protocol error with code, message, and optional details.
///
/// This is the domain-level error—it gets wrapped into a JSON-RPC
/// error response before being sent on the wire. The code identifies
/// the error category, the message is human-readable, and data carries
/// optional structured diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct A2AError {
	/// Numeric error code from the A2A or JSON-RPC spec.
	pub code: i32,

	/// Human-readable error description.
	pub message: String,

	/// Optional structured error details.
	/// The spec doesn't prescribe the format—agents include
	/// whatever diagnostics are useful (retry guidance, upstream
	/// error codes, validation failures, etc.).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub data: Option<serde_json::Value>,
}

impl A2AError {
	/// Create a `TaskNotFound` error for the given task ID.
	#[must_use]
	pub fn task_not_found(task_id: &str) -> Self {
		Self {
			code: code::TASK_NOT_FOUND,
			message: format!("task not found: {task_id}"),
			data: None,
		}
	}

	/// Create a `TaskNotCancelable` error for the given task ID.
	#[must_use]
	pub fn task_not_cancelable(task_id: &str) -> Self {
		Self {
			code: code::TASK_NOT_CANCELABLE,
			message: format!("task cannot be canceled: {task_id}"),
			data: None,
		}
	}

	/// Create a `ContentTypeNotSupported` error.
	#[must_use]
	pub fn content_type_not_supported(detail: &str) -> Self {
		Self {
			code: code::CONTENT_TYPE_NOT_SUPPORTED,
			message: format!("content type not supported: {detail}"),
			data: None,
		}
	}

	/// Create an `UnsupportedOperation` error.
	#[must_use]
	pub fn unsupported_operation(operation: &str) -> Self {
		Self {
			code: code::UNSUPPORTED_OPERATION,
			message: format!("operation not supported: {operation}"),
			data: None,
		}
	}

	/// Create a `PushNotificationNotSupported` error.
	#[must_use]
	pub fn push_notification_not_supported() -> Self {
		Self {
			code: code::PUSH_NOTIFICATION_NOT_SUPPORTED,
			message: "push notifications not supported".into(),
			data: None,
		}
	}

	/// Create an `InvalidParams` error for malformed request parameters.
	///
	/// Returned when the JSON-RPC params field cannot be deserialised
	/// into the expected type for the method. The detail string is
	/// moved into the `data` field rather than embedded in the message
	/// so that internal serde error messages are not exposed verbatim
	/// in the generic wire response, while remaining accessible to
	/// callers that inspect structured diagnostics.
	///
	/// ```
	/// # use a2a::A2AError;
	/// let err = A2AError::invalid_params("missing field `id`");
	/// assert_eq!(err.code, -32602);
	/// assert_eq!(err.message, "invalid parameters");
	/// assert_eq!(err.data.unwrap().as_str().unwrap(), "missing field `id`");
	/// ```
	#[must_use]
	pub fn invalid_params(detail: impl std::fmt::Display) -> Self {
		Self {
			code: code::INVALID_PARAMS,
			message: "invalid parameters".into(),
			data: Some(serde_json::Value::String(format!("{detail}"))),
		}
	}

	/// Create a `MethodNotFound` error for an unrecognised JSON-RPC method.
	///
	/// Returned when the dispatcher receives a method name it doesn't
	/// recognise. The method name is included in the message so the
	/// caller can identify typos or version mismatches (e.g. calling
	/// a method added in a newer spec version against an older agent).
	///
	/// ```
	/// # use a2a::A2AError;
	/// let err = A2AError::method_not_found("tasks/nonexistent");
	/// assert_eq!(err.code, -32601);
	/// assert!(err.message.contains("tasks/nonexistent"));
	/// ```
	#[must_use]
	pub fn method_not_found(method: &str) -> Self {
		Self {
			code: code::METHOD_NOT_FOUND,
			message: format!("unknown method: {method}"),
			data: None,
		}
	}

	/// Create a transport-level error for client-side failures.
	///
	/// Not part of the JSON-RPC or A2A spec—used by the client
	/// module to represent HTTP failures, connection errors, timeouts,
	/// and response parsing failures. Agents never return this code;
	/// it exists solely for client-side error reporting.
	///
	/// ```
	/// # use a2a::A2AError;
	/// let err = A2AError::transport("connection refused");
	/// assert_eq!(err.code, a2a::error::code::TRANSPORT_ERROR);
	/// assert!(err.message.contains("connection refused"));
	/// ```
	#[must_use]
	pub fn transport(detail: impl std::fmt::Display) -> Self {
		Self {
			code: code::TRANSPORT_ERROR,
			message: format!("transport error: {detail}"),
			data: None,
		}
	}

	/// Create a `ParseError` for a request body that could not be decoded as JSON.
	///
	/// Returned before any method dispatch when the raw bytes received over
	/// the wire are not valid JSON. The detail string is moved into the `data`
	/// field rather than embedded in the message, so that `serde_json` internals
	/// are not surfaced in the generic wire response while remaining available
	/// to structured diagnostic consumers.
	///
	/// ```
	/// # use a2a::A2AError;
	/// let err = A2AError::parse_error("unexpected token at line 1 column 1");
	/// assert_eq!(err.code, -32700);
	/// assert_eq!(err.message, "parse error");
	/// assert!(err.data.unwrap().as_str().unwrap().contains("unexpected token"));
	/// ```
	#[must_use]
	pub fn parse_error(detail: impl std::fmt::Display) -> Self {
		Self {
			code: code::PARSE_ERROR,
			message: "parse error".into(),
			data: Some(serde_json::Value::String(format!("{detail}"))),
		}
	}

	/// Create an internal error for unexpected server-side failures.
	///
	/// Used when the server encounters an unexpected condition that
	/// prevents it from completing the request—most commonly a
	/// serialisation failure when encoding the handler's return
	/// value into the JSON-RPC response. The detail string is moved
	/// into the `data` field so that internal implementation details
	/// are not surfaced in the generic wire response, while remaining
	/// available to operators inspecting structured diagnostics.
	///
	/// ```
	/// # use a2a::A2AError;
	/// let err = A2AError::internal_error("failed to serialise response");
	/// assert_eq!(err.code, -32603);
	/// assert_eq!(err.message, "internal error");
	/// assert!(err.data.unwrap().as_str().unwrap().contains("failed to serialise response"));
	/// ```
	#[must_use]
	pub fn internal_error(detail: impl std::fmt::Display) -> Self {
		Self {
			code: code::INTERNAL_ERROR,
			message: "internal error".into(),
			data: Some(serde_json::Value::String(format!("{detail}"))),
		}
	}

	/// Attach structured diagnostic data to this error.
	///
	/// The data field carries optional machine-readable details that
	/// help the caller diagnose the failure—retry guidance, upstream
	/// error codes, validation breakdown, etc. The spec doesn't
	/// prescribe a format, so agents include whatever is useful for
	/// their specific error scenarios.
	///
	/// ```
	/// # use a2a::A2AError;
	/// let err = A2AError::task_not_found("task-1")
	///     .with_data(serde_json::json!({"searched": ["active", "archived"]}));
	/// assert!(err.data.is_some());
	/// ```
	#[must_use]
	pub fn with_data(mut self, data: serde_json::Value) -> Self {
		self.data = Some(data);
		self
	}
}

impl std::fmt::Display for A2AError {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "A2A error {}: {}", self.code, self.message)
	}
}

impl std::error::Error for A2AError {}

#[cfg(test)]
mod tests {
	use super::*;

	// Error codes are fixed by the spec. If they drift, every
	// other A2A implementation will misinterpret our errors. This
	// test pins the exact numeric values as a regression guard.
	#[test]
	fn error_codes_match_spec() {
		// Standard JSON-RPC infrastructure codes—defined by the
		// JSON-RPC 2.0 specification and shared across all compliant
		// implementations. These must never change.
		assert_eq!(code::PARSE_ERROR, -32700);
		assert_eq!(code::INVALID_REQUEST, -32600);
		assert_eq!(code::INTERNAL_ERROR, -32603);
		assert_eq!(code::METHOD_NOT_FOUND, -32601);
		assert_eq!(code::INVALID_PARAMS, -32602);

		// A2A domain-specific codes—defined by the A2A protocol
		// specification on top of the JSON-RPC range. These identify
		// A2A-specific failure conditions and must match the spec.
		assert_eq!(code::TASK_NOT_FOUND, -32001);
		assert_eq!(code::TASK_NOT_CANCELABLE, -32002);
		assert_eq!(code::CONTENT_TYPE_NOT_SUPPORTED, -32003);
		assert_eq!(code::UNSUPPORTED_OPERATION, -32004);
		assert_eq!(code::PUSH_NOTIFICATION_NOT_SUPPORTED, -32005);
	}

	// Factory methods should produce errors with the correct code
	// and a message that includes the context (task ID, operation
	// name, etc.). This ensures error messages are actionable when
	// debugging interop issues between agents.
	#[test]
	fn factory_methods_produce_correct_codes() {
		let err = A2AError::task_not_found("task-42");
		assert_eq!(err.code, code::TASK_NOT_FOUND);
		assert!(err.message.contains("task-42"));

		let err = A2AError::unsupported_operation("SendStreamingMessage");
		assert_eq!(err.code, code::UNSUPPORTED_OPERATION);
		assert!(err.message.contains("SendStreamingMessage"));
	}

	// Errors must round-trip through serde because they're embedded
	// in JSON-RPC response payloads. The optional data field should
	// be omitted when None and preserved when present.
	#[test]
	fn error_round_trips_with_data() {
		let err = A2AError {
			code: code::TASK_NOT_FOUND,
			message: "task not found: task-1".into(),
			data: Some(serde_json::json!({"searched": ["active", "archived"]})),
		};

		let json = serde_json::to_string(&err).unwrap();
		let back: A2AError = serde_json::from_str(&json).unwrap();
		assert_eq!(back, err);
	}

	// When data is None, it must be absent from the wire format
	// (not serialised as null). This keeps error payloads compact
	// for the common case where no extra diagnostics are needed.
	#[test]
	fn error_omits_none_data() {
		let err = A2AError::task_not_found("task-1");
		let json = serde_json::to_value(&err).unwrap();
		assert!(!json.as_object().unwrap().contains_key("data"));
	}

	// The Display impl is used in logging and error chains.
	// It should include both the code and message for quick
	// identification when scanning logs.
	#[test]
	fn display_includes_code_and_message() {
		let err = A2AError::task_not_found("task-99");
		let display = format!("{err}");
		assert!(display.contains("-32001"));
		assert!(display.contains("task-99"));
	}

	// invalid_params() is the constructor the dispatch layer uses
	// when serde deserialisation of request params fails. The error
	// must carry the INVALID_PARAMS code with a generic message—the
	// detail is moved into the data field so that internal serde
	// error text is not surfaced verbatim in the wire response.
	#[test]
	fn invalid_params_produces_correct_code_and_message() {
		let err = A2AError::invalid_params("missing field `id`");
		assert_eq!(err.code, code::INVALID_PARAMS);
		assert_eq!(
			err.message, "invalid parameters",
			"message must be the generic category string, not the raw serde detail"
		);
		assert_eq!(
			err.data.as_ref().unwrap().as_str().unwrap(),
			"missing field `id`",
			"serde detail must be placed in the data field"
		);
	}

	// method_not_found() is used when the dispatcher receives an
	// unrecognised JSON-RPC method name. The error must carry the
	// METHOD_NOT_FOUND code and include the offending method name
	// so the caller can spot typos or version mismatches.
	#[test]
	fn method_not_found_produces_correct_code_and_message() {
		let err = A2AError::method_not_found("tasks/nonexistent");
		assert_eq!(err.code, code::METHOD_NOT_FOUND);
		assert!(
			err.message.contains("tasks/nonexistent"),
			"message should include the unknown method name"
		);
		assert!(err.data.is_none());
	}

	// transport() wraps client-side failures (HTTP errors, timeouts,
	// connection drops) into A2AError. It uses the TRANSPORT_ERROR
	// code which is client-only—agents never return it. The message
	// must carry the "transport error:" prefix so error logs are
	// categorically distinct from other A2AError messages.
	#[test]
	fn transport_produces_correct_code_and_message() {
		let err = A2AError::transport("connection refused");
		assert_eq!(err.code, code::TRANSPORT_ERROR);
		assert!(
			err.message.starts_with("transport error:"),
			"message must begin with the 'transport error:' category prefix"
		);
		assert!(
			err.message.contains("connection refused"),
			"message should include the transport failure detail"
		);
		assert!(err.data.is_none());
	}

	// with_data() attaches structured diagnostics to any error.
	// The data should be preserved through serialisation and not
	// interfere with the code or message fields.
	#[test]
	fn with_data_attaches_structured_diagnostics() {
		let err = A2AError::task_not_found("task-1")
			.with_data(serde_json::json!({"searched": ["active", "archived"]}));

		assert_eq!(err.code, code::TASK_NOT_FOUND);
		assert!(err.message.contains("task-1"));
		let data = err.data.unwrap();
		assert_eq!(data["searched"][0], "active");
		assert_eq!(data["searched"][1], "archived");
	}

	// with_data() should work with any error constructor, not just
	// task_not_found. Verify it chains correctly with the new
	// constructors. Note that invalid_params already populates data
	// with the serde detail string—with_data() overwrites it, which
	// is the correct behaviour for callers that want to replace the
	// auto-populated diagnostic with structured data.
	#[test]
	fn with_data_chains_with_all_constructors() {
		let diag = serde_json::json!({"hint": "check spelling"});

		let err = A2AError::invalid_params("bad field").with_data(diag.clone());
		assert_eq!(err.code, code::INVALID_PARAMS);
		assert_eq!(err.data, Some(diag.clone()));

		let err = A2AError::method_not_found("bad/method").with_data(diag.clone());
		assert_eq!(err.code, code::METHOD_NOT_FOUND);
		assert_eq!(err.data, Some(diag.clone()));

		let err = A2AError::transport("timeout").with_data(diag.clone());
		assert_eq!(err.code, code::TRANSPORT_ERROR);
		assert_eq!(err.data, Some(diag));
	}
}
