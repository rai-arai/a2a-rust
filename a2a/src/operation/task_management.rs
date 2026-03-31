// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Task management operation types for the A2A protocol.
//!
//! Once a task exists (created by `SendMessage` or `SendStreamingMessage`),
//! callers use these operations to query, monitor, and control it.
//!
//! `GetTask` retrieves a task by ID, optionally with conversation
//! history. `ListTasks` returns a paginated collection of tasks
//! filtered by context, status, or timing. `CancelTask` requests
//! that the agent stop work on a task—the agent may not honor this
//! immediately if the task is in a non-cancelable state. `SubscribeToTask`
//! reconnects to a task's SSE event stream after a dropped connection,
//! without resending the original message.
//!
//! All request types carry a task ID as their primary identifier.
//! The optional tenant field scopes the request to a specific
//! tenant in multi-tenant deployments. When absent the agent uses
//! its default tenant resolution strategy.

use serde::{Deserialize, Serialize};

use crate::task::Task;
use crate::task_state::TaskState;

/// Parameters for the `GetTask` operation.
///
/// Retrieves a single task by its identifier. The optional
/// `history_length` controls how many conversation messages are
/// included in the response—useful for limiting payload size
/// when the caller only needs the current status. The optional
/// tenant field scopes the lookup to a specific tenant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskParams {
	/// The unique identifier of the task to retrieve.
	pub id: String,

	/// Maximum number of conversation history messages to include
	/// in the response. When omitted, the agent returns its default
	/// amount of history (often all available messages).
	#[serde(skip_serializing_if = "Option::is_none")]
	pub history_length: Option<i32>,

	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,
}

impl GetTaskParams {
	/// Create params to retrieve a task by its ID.
	///
	/// History length and tenant default to None, producing the
	/// most compact wire representation. Chain `with_history_length()`
	/// to request conversation history or `with_tenant()` to scope
	/// the request to a specific tenant in multi-tenant deployments.
	///
	/// ```
	/// # use a2a::GetTaskParams;
	/// let params = GetTaskParams::new("task-1");
	/// assert_eq!(params.id, "task-1");
	///
	/// // Request the last 10 messages of conversation history.
	/// let params = GetTaskParams::new("task-1").with_history_length(10);
	/// assert_eq!(params.history_length, Some(10));
	/// ```
	#[must_use]
	pub fn new(id: impl Into<String>) -> Self {
		Self {
			id: id.into(),
			history_length: None,
			tenant: None,
		}
	}

	/// Request a specific number of conversation history messages.
	///
	/// The agent includes up to this many messages in the response's
	/// history field. When omitted, the agent returns its default
	/// amount (often all messages). Set to 0 to explicitly request
	/// no history, which keeps the response payload small when only
	/// the task's current status matters.
	#[must_use]
	pub fn with_history_length(mut self, length: i32) -> Self {
		self.history_length = Some(length);
		self
	}

	/// Scope this request to a specific tenant.
	///
	/// In multi-tenant deployments the agent may serve multiple
	/// isolated tenants. Providing a tenant identifier routes the
	/// lookup to the correct tenant's task store. When omitted the
	/// agent uses its default tenant resolution strategy.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

/// Parameters for the `CancelTask` operation.
///
/// Requests cancellation of a running task. The agent transitions
/// the task to the Canceled state if the task is cancelable. Tasks
/// in terminal states (completed, failed, already canceled) cannot
/// be canceled—the agent returns a `TaskNotCancelable` error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelTaskParams {
	/// The unique identifier of the task to cancel.
	pub id: String,

	/// Optional caller-defined metadata.
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

impl CancelTaskParams {
	/// Create params to cancel a task by its ID.
	///
	/// Metadata and tenant default to None. Chain `with_metadata()`
	/// to attach caller-defined context like cancellation reasons
	/// or audit trail references, and `with_tenant()` to scope the
	/// request to a specific tenant in multi-tenant deployments.
	///
	/// ```
	/// # use a2a::CancelTaskParams;
	/// let params = CancelTaskParams::new("task-1");
	/// assert_eq!(params.id, "task-1");
	/// ```
	#[must_use]
	pub fn new(id: impl Into<String>) -> Self {
		Self {
			id: id.into(),
			metadata: None,
			tenant: None,
		}
	}

	/// Attach caller-defined metadata for tracing or debugging.
	///
	/// The agent may log this metadata alongside the cancellation
	/// event but does not interpret or act on it. Common uses
	/// include the reason for cancellation or the identity of the
	/// caller requesting it. The parameter type is
	/// `serde_json::Map<String, serde_json::Value>` so the type
	/// system enforces the proto3 google.protobuf.Struct constraint—
	/// arrays and scalars cannot be passed at compile time.
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}

	/// Scope this request to a specific tenant.
	///
	/// In multi-tenant deployments the agent may serve multiple
	/// isolated tenants. Providing a tenant identifier routes the
	/// cancellation to the correct tenant's task store. When omitted
	/// the agent uses its default tenant resolution strategy.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

/// Parameters for the `SubscribeToTask` operation.
///
/// Re-attaches to a task's SSE event stream after a disconnection.
/// Unlike `SendMessage` or `SendStreamingMessage`, this doesn't send a new
/// message—it just reopens the stream for an existing task. The
/// agent replays any events the caller may have missed, then
/// continues streaming live updates.
///
/// Renamed from `ResubscribeTaskParams` in v1.0 to align with the
/// proto3 `SubscribeTaskRequest` naming convention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeToTaskParams {
	/// The unique identifier of the task to resubscribe to.
	pub id: String,

	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,
}

impl SubscribeToTaskParams {
	/// Create params to resubscribe to a task's SSE event stream.
	///
	/// Takes just the task ID—the agent looks up the task and
	/// replays any events the caller missed since disconnecting,
	/// then continues with live delivery. Tenant defaults to None;
	/// chain `with_tenant()` to scope the subscription to a specific
	/// tenant in multi-tenant deployments.
	///
	/// ```
	/// # use a2a::SubscribeToTaskParams;
	/// let params = SubscribeToTaskParams::new("task-1");
	/// assert_eq!(params.id, "task-1");
	/// ```
	#[must_use]
	pub fn new(id: impl Into<String>) -> Self {
		Self {
			id: id.into(),
			tenant: None,
		}
	}

	/// Scope this subscription to a specific tenant.
	///
	/// In multi-tenant deployments the agent may serve multiple
	/// isolated tenants. Providing a tenant identifier routes the
	/// stream subscription to the correct tenant's task. When
	/// omitted the agent uses its default tenant resolution strategy.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

/// Parameters for the `ListTasks` operation.
///
/// Filters and paginates tasks by context, status, and timing.
/// All fields are optional—an empty params object returns the
/// first page of all tasks visible to the caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksParams {
	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,

	/// Restrict results to tasks that belong to this context.
	/// Useful for retrieving all tasks created in one conversation
	/// or workflow session.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub context_id: Option<String>,

	/// Restrict results to tasks currently in this lifecycle state.
	/// When absent, tasks in all states are returned.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub status: Option<TaskState>,

	/// Maximum number of tasks to return per page.
	/// The agent may return fewer. When absent, the agent uses its
	/// default page size.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub page_size: Option<i32>,

	/// Opaque continuation token from a previous `ListTasks` response.
	/// Supply this to retrieve the next page of results.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub page_token: Option<String>,

	/// Maximum number of conversation history messages to include
	/// per task in the response. When absent the agent returns its
	/// default amount of history.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub history_length: Option<i32>,

	/// Restrict results to tasks whose status changed at or after this
	/// point in time. Typed as `time::OffsetDateTime` so invalid
	/// timestamps are rejected at deserialisation rather than silently
	/// passed through as opaque strings. Serialised as a quoted RFC 3339
	/// string on the wire for compatibility with all A2A implementations.
	#[serde(
		default,
		with = "crate::serde_helpers::serde_rfc3339_option",
		skip_serializing_if = "Option::is_none"
	)]
	pub status_timestamp_after: Option<time::OffsetDateTime>,

	/// Whether to include artifact data in each returned task.
	/// When absent the agent returns its default behaviour (often
	/// true). Set to false to reduce payload size when artifact
	/// content is not needed.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub include_artifacts: Option<bool>,
}

impl ListTasksParams {
	/// Create params with all filters set to None.
	///
	/// An empty params object returns the first page of all tasks
	/// visible to the caller. Chain the `with_*` methods to narrow
	/// the result set or control pagination.
	///
	/// ```
	/// # use a2a::ListTasksParams;
	/// let params = ListTasksParams::new();
	/// assert!(params.context_id.is_none());
	/// assert!(params.page_size.is_none());
	///
	/// // Retrieve the first 20 tasks in a specific context.
	/// let params = ListTasksParams::new()
	///     .with_context_id("ctx-1")
	///     .with_page_size(20);
	/// assert_eq!(params.context_id, Some("ctx-1".into()));
	/// assert_eq!(params.page_size, Some(20));
	/// ```
	#[must_use]
	pub fn new() -> Self {
		Self {
			tenant: None,
			context_id: None,
			status: None,
			page_size: None,
			page_token: None,
			history_length: None,
			status_timestamp_after: None,
			include_artifacts: None,
		}
	}

	/// Scope this listing to a specific tenant.
	///
	/// In multi-tenant deployments the agent may serve multiple
	/// isolated tenants. Providing a tenant identifier restricts
	/// the listing to that tenant's task store.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}

	/// Restrict results to tasks in the given context.
	///
	/// A context groups all tasks created during one conversation
	/// or workflow session. This is the most common filter—callers
	/// often want all tasks spawned within a single session.
	#[must_use]
	pub fn with_context_id(mut self, context_id: impl Into<String>) -> Self {
		self.context_id = Some(context_id.into());
		self
	}

	/// Restrict results to tasks in the given lifecycle state.
	///
	/// Combine with `with_status_timestamp_after()` to poll only
	/// recently-transitioned tasks in a specific state.
	#[must_use]
	pub fn with_status(mut self, status: TaskState) -> Self {
		self.status = Some(status);
		self
	}

	/// Set the maximum number of tasks to return per page.
	///
	/// The agent may honour a lower value than requested. To
	/// retrieve subsequent pages supply the `next_page_token` from
	/// the previous response via `with_page_token()`.
	#[must_use]
	pub fn with_page_size(mut self, page_size: i32) -> Self {
		self.page_size = Some(page_size);
		self
	}

	/// Supply a continuation token to retrieve the next page.
	///
	/// Use the `next_page_token` value from a previous
	/// `ListTasksResponse`. When this field is absent the agent
	/// returns the first page.
	#[must_use]
	pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
		self.page_token = Some(page_token.into());
		self
	}

	/// Request conversation history for each returned task.
	///
	/// The agent includes up to this many conversation messages per
	/// task. Set to 0 to explicitly request no history, which keeps
	/// the response payload small when only task status matters.
	#[must_use]
	pub fn with_history_length(mut self, history_length: i32) -> Self {
		self.history_length = Some(history_length);
		self
	}

	/// Restrict results to tasks whose status changed at or after
	/// the given point in time.
	///
	/// Useful for efficient polling: supply the timestamp of the
	/// last known update to receive only tasks that have changed
	/// since then. The value is serialised as an RFC 3339 string
	/// on the wire.
	#[must_use]
	pub fn with_status_timestamp_after(
		mut self,
		status_timestamp_after: time::OffsetDateTime,
	) -> Self {
		self.status_timestamp_after = Some(status_timestamp_after);
		self
	}

	/// Control whether artifact data is included per task.
	///
	/// Pass `false` to suppress artifacts in the response, reducing
	/// payload size when the caller only needs task status. Pass
	/// `true` to ensure artifacts are always included even if the
	/// agent's default omits them.
	#[must_use]
	pub fn with_include_artifacts(mut self, include_artifacts: bool) -> Self {
		self.include_artifacts = Some(include_artifacts);
		self
	}
}

impl Default for ListTasksParams {
	fn default() -> Self {
		Self::new()
	}
}

/// Response from the `ListTasks` operation.
///
/// Contains the matching tasks plus pagination metadata. The
/// `page_size` reports the maximum number of tasks per page that
/// the server applied (which may be lower than what the caller
/// requested). The `total_size` reports the total number of tasks
/// matching the filter across all pages—callers use this to display
/// pagination controls without exhausting the entire result set.
///
/// When `next_page_token` is non-empty there are more results—repeat
/// the request with that token via `ListTasksParams::with_page_token()`
/// to retrieve the next page. An empty string indicates this is the
/// final page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResponse {
	/// The tasks matching the filter criteria on this page.
	pub tasks: Vec<Task>,

	/// The effective page size applied by the server for this response.
	/// May be lower than the `page_size` requested by the caller.
	///
	/// Defaults to 0 when absent on the wire, matching proto3 `int32`
	/// zero-value semantics. This tolerates non-conformant senders that
	/// omit the field without breaking deserialisation.
	#[serde(default)]
	pub page_size: i32,

	/// Total number of tasks matching the filter criteria across all pages.
	/// Callers use this to compute total page count and display
	/// pagination controls without fetching every page.
	///
	/// Defaults to 0 when absent on the wire, matching proto3 `int32`
	/// zero-value semantics.
	#[serde(default)]
	pub total_size: i32,

	/// Opaque token for retrieving the next page of results.
	/// Empty string when this is the final page.
	pub next_page_token: String,
}

impl ListTasksResponse {
	/// Whether there are more pages of results available.
	///
	/// Returns `true` when `next_page_token` is non-empty—pass the
	/// token to `ListTasksParams::with_page_token()` to retrieve
	/// the next page. Returns `false` when this response is the
	/// final page.
	#[must_use]
	pub fn has_next_page(&self) -> bool {
		!self.next_page_token.is_empty()
	}
}

#[cfg(test)]
mod tests {
	use time::macros::datetime;

	use super::*;

	// GetTaskParams uses camelCase on the wire because the A2A spec
	// follows JavaScript naming conventions in its JSON schema. The
	// historyLength field must serialise as camelCase, not snake_case.
	#[test]
	fn get_task_params_camel_case() {
		let params = GetTaskParams {
			id: "task-1".into(),
			history_length: Some(5),
			tenant: None,
		};

		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["id"], "task-1");
		assert_eq!(json["historyLength"], 5);
		assert!(
			!json.as_object().unwrap().contains_key("history_length"),
			"must use camelCase, not snake_case"
		);
	}

	// Optional fields must be absent from the wire format when None,
	// not serialised as null. This keeps request payloads compact
	// and avoids confusing agents that distinguish between absent
	// and null values.
	#[test]
	fn get_task_params_omits_none_fields() {
		let params = GetTaskParams {
			id: "task-2".into(),
			history_length: None,
			tenant: None,
		};

		let json = serde_json::to_value(&params).unwrap();
		let obj = json.as_object().unwrap();
		assert!(obj.contains_key("id"));
		assert!(!obj.contains_key("historyLength"));
		assert!(!obj.contains_key("tenant"));

		// The old metadata field must not exist on GetTaskParams
		// in v1.0—it was removed from the proto GetTaskRequest.
		assert!(!obj.contains_key("metadata"));
	}

	// CancelTaskParams is intentionally minimal—just the task ID
	// and optional metadata/tenant. The agent doesn't need anything
	// else to process a cancellation request.
	#[test]
	fn cancel_task_params_serialise() {
		let params = CancelTaskParams {
			id: "task-3".into(),
			metadata: Some(
				serde_json::json!({"reason": "caller requested"})
					.as_object()
					.unwrap()
					.clone(),
			),
			tenant: None,
		};

		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["id"], "task-3");
		assert_eq!(json["metadata"]["reason"], "caller requested");
	}

	// SubscribeToTaskParams must round-trip through serde because
	// callers may deserialise their own requests (e.g. for logging
	// or replay). The structure is simple but correctness matters.
	#[test]
	fn subscribe_to_task_params_round_trip() {
		let params = SubscribeToTaskParams {
			id: "task-4".into(),
			tenant: None,
		};

		let json = serde_json::to_string(&params).unwrap();
		let back: SubscribeToTaskParams = serde_json::from_str(&json).unwrap();
		assert_eq!(back, params);
	}

	// Deserialise from wire JSON as it would arrive from another
	// A2A implementation. This validates the inbound parsing path
	// for task management requests.
	#[test]
	fn deserialise_get_task_from_wire() {
		let json = r#"{"id": "task-ext-1", "historyLength": 20}"#;
		let params: GetTaskParams = serde_json::from_str(json).unwrap();
		assert_eq!(params.id, "task-ext-1");
		assert_eq!(params.history_length, Some(20));
	}

	// GetTaskParams::new() creates params with just the task ID.
	// Optional fields (history_length, tenant) must be None so
	// the wire format is compact for the common case where the
	// caller just wants the task's current state.
	#[test]
	fn get_task_params_new_sets_id_only() {
		let params = GetTaskParams::new("task-builder-1");
		assert_eq!(params.id, "task-builder-1");
		assert!(params.history_length.is_none());
		assert!(params.tenant.is_none());
	}

	// with_history_length() controls how many conversation messages
	// the agent includes in the response. Verify it sets the field
	// without disturbing the ID or tenant.
	#[test]
	fn get_task_params_with_history_length() {
		let params = GetTaskParams::new("task-builder-2").with_history_length(10);
		assert_eq!(params.id, "task-builder-2");
		assert_eq!(params.history_length, Some(10));
	}

	// with_tenant() scopes the task lookup to a specific tenant in
	// multi-tenant deployments. Verify it sets the field without
	// disturbing the ID or history_length.
	#[test]
	fn get_task_params_with_tenant() {
		let params = GetTaskParams::new("task-builder-3").with_tenant("acme-corp");
		assert_eq!(params.tenant, Some("acme-corp".into()));
	}

	// The builder-constructed params must produce the same wire
	// format as an equivalent struct literal. This guards against
	// the builder accidentally setting fields to different types
	// or formats than the struct expects.
	#[test]
	fn get_task_params_builder_matches_struct_literal() {
		let from_builder = GetTaskParams::new("task-1").with_history_length(5);
		let from_literal = GetTaskParams {
			id: "task-1".into(),
			history_length: Some(5),
			tenant: None,
		};
		assert_eq!(
			serde_json::to_value(&from_builder).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// CancelTaskParams::new() follows the same pattern as
	// GetTaskParams::new()—ID only, optional fields default to
	// None. Cancel requests are intentionally minimal because the
	// agent only needs the task ID to process a cancellation.
	#[test]
	fn cancel_task_params_new_sets_id_only() {
		let params = CancelTaskParams::new("cancel-1");
		assert_eq!(params.id, "cancel-1");
		assert!(params.metadata.is_none());
		assert!(params.tenant.is_none());
	}

	// with_metadata() on CancelTaskParams allows callers to attach
	// context like cancellation reasons or audit trail IDs. The
	// agent may log this metadata but does not act on it.
	// The metadata field stores a Map (JSON object), so the
	// assertion compares against the extracted object.
	#[test]
	fn cancel_task_params_with_metadata() {
		let meta_map = serde_json::json!({"reason": "operator requested"})
			.as_object()
			.unwrap()
			.clone();
		let expected_map = meta_map.clone();
		let params = CancelTaskParams::new("cancel-2").with_metadata(meta_map);
		assert_eq!(params.metadata, Some(expected_map));
	}

	// with_tenant() on CancelTaskParams scopes the cancellation to
	// the named tenant. Verify the setter populates the field
	// without disturbing ID or metadata.
	#[test]
	fn cancel_task_params_with_tenant() {
		let params = CancelTaskParams::new("cancel-3").with_tenant("tenant-x");
		assert_eq!(params.tenant, Some("tenant-x".into()));
		assert!(params.metadata.is_none());
	}

	// CancelTaskParams tenant field must serialise as camelCase and
	// be absent when None, consistent with the other optional fields.
	#[test]
	fn cancel_task_params_tenant_wire_format() {
		let without_tenant = CancelTaskParams::new("cancel-wire-1");
		let json = serde_json::to_value(&without_tenant).unwrap();
		assert!(!json.as_object().unwrap().contains_key("tenant"));

		let with_tenant = CancelTaskParams::new("cancel-wire-2").with_tenant("org-y");
		let json = serde_json::to_value(&with_tenant).unwrap();
		assert_eq!(json["tenant"], "org-y");
	}

	// SubscribeToTaskParams::new() creates params for reconnecting
	// to a task's SSE stream. Takes just the task ID and defaults
	// tenant to None—the common case for single-tenant deployments.
	#[test]
	fn subscribe_to_task_params_new_sets_id_only() {
		let params = SubscribeToTaskParams::new("resub-1");
		assert_eq!(params.id, "resub-1");
		assert!(params.tenant.is_none());
	}

	// with_tenant() on SubscribeToTaskParams scopes the stream
	// subscription to the named tenant. Verify the setter populates
	// the field without disturbing the task ID.
	#[test]
	fn subscribe_to_task_params_with_tenant() {
		let params = SubscribeToTaskParams::new("resub-2").with_tenant("tenant-sub");
		assert_eq!(params.tenant, Some("tenant-sub".into()));
		assert_eq!(params.id, "resub-2");
	}

	// SubscribeToTaskParams tenant must appear on the wire when set
	// and be absent when None. The old metadata field must not appear
	// at all—it was removed in the rename from ResubscribeTaskParams.
	#[test]
	fn subscribe_to_task_params_wire_format() {
		let without_tenant = SubscribeToTaskParams::new("resub-wire-1");
		let json = serde_json::to_value(&without_tenant).unwrap();
		let obj = json.as_object().unwrap();
		assert!(obj.contains_key("id"));
		assert!(!obj.contains_key("tenant"));
		assert!(
			!obj.contains_key("metadata"),
			"metadata was removed in v1.0 rename"
		);

		let with_tenant = SubscribeToTaskParams::new("resub-wire-2").with_tenant("org-z");
		let json = serde_json::to_value(&with_tenant).unwrap();
		assert_eq!(json["tenant"], "org-z");
	}

	// ListTasksParams::new() creates a params object with every field
	// set to None. An empty params object is valid—it returns the
	// first page of all tasks without filtering. This is the baseline
	// that callers narrow down with the with_* builder methods.
	#[test]
	fn list_tasks_params_new_all_none() {
		let params = ListTasksParams::new();
		assert!(params.tenant.is_none());
		assert!(params.context_id.is_none());
		assert!(params.status.is_none());
		assert!(params.page_size.is_none());
		assert!(params.page_token.is_none());
		assert!(params.history_length.is_none());
		assert!(params.status_timestamp_after.is_none());
		assert!(params.include_artifacts.is_none());
	}

	// Default::default() on ListTasksParams must produce the same
	// result as ListTasksParams::new(). This prevents surprises for
	// callers who use the derive-style default.
	#[test]
	fn list_tasks_params_default_matches_new() {
		let from_new = ListTasksParams::new();
		let from_default = ListTasksParams::default();
		assert_eq!(from_new, from_default);
	}

	// Each with_* builder method must set exactly its own field
	// without disturbing the others. The builder chain is tested
	// end-to-end to verify all setters interact correctly.
	#[test]
	fn list_tasks_params_builder_chain() {
		let cutoff = datetime!(2026-01-01 00:00:00 UTC);
		let params = ListTasksParams::new()
			.with_tenant("acme")
			.with_context_id("ctx-42")
			.with_status(TaskState::Working)
			.with_page_size(10)
			.with_page_token("tok-abc")
			.with_history_length(5)
			.with_status_timestamp_after(cutoff)
			.with_include_artifacts(false);

		assert_eq!(params.tenant, Some("acme".into()));
		assert_eq!(params.context_id, Some("ctx-42".into()));
		assert_eq!(params.status, Some(TaskState::Working));
		assert_eq!(params.page_size, Some(10));
		assert_eq!(params.page_token, Some("tok-abc".into()));
		assert_eq!(params.history_length, Some(5));
		assert_eq!(params.status_timestamp_after, Some(cutoff));
		assert_eq!(params.include_artifacts, Some(false));
	}

	// All fields must serialise as camelCase and be absent when None.
	// An empty ListTasksParams must produce an empty JSON object,
	// not a bag of null-valued keys.
	#[test]
	fn list_tasks_params_omits_none_fields() {
		let params = ListTasksParams::new();
		let json = serde_json::to_value(&params).unwrap();
		let obj = json.as_object().unwrap();
		assert!(
			obj.is_empty(),
			"empty params must serialise as an empty object, got: {obj:?}"
		);
	}

	// Wire format for a fully-populated ListTasksParams: all fields
	// present and all keys in camelCase. This guards against any
	// rename attribute regression or missing skip_serializing_if.
	#[test]
	fn list_tasks_params_wire_format() {
		let params = ListTasksParams::new()
			.with_context_id("ctx-wire")
			.with_page_size(25)
			.with_page_token("next-tok");

		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["contextId"], "ctx-wire");
		assert_eq!(json["pageSize"], 25);
		assert_eq!(json["pageToken"], "next-tok");

		// Fields not set must be absent—never serialised as null.
		let obj = json.as_object().unwrap();
		assert!(!obj.contains_key("tenant"));
		assert!(!obj.contains_key("status"));
		assert!(!obj.contains_key("historyLength"));
		assert!(!obj.contains_key("statusTimestampAfter"));
		assert!(!obj.contains_key("includeArtifacts"));
	}

	// ListTasksParams must round-trip through serde so callers can
	// deserialise incoming requests (e.g. for logging or replay).
	// Use an empty params object as the simplest round-trip case.
	#[test]
	fn list_tasks_params_round_trip_empty() {
		let params = ListTasksParams::new();
		let json = serde_json::to_string(&params).unwrap();
		let back: ListTasksParams = serde_json::from_str(&json).unwrap();
		assert_eq!(back, params);
	}

	// Round-trip a fully-populated ListTasksParams to verify that
	// every field survives serialisation and deserialisation intact.
	#[test]
	fn list_tasks_params_round_trip_full() {
		let params = ListTasksParams::new()
			.with_tenant("tenant-rt")
			.with_context_id("ctx-rt")
			.with_status(TaskState::Completed)
			.with_page_size(50)
			.with_page_token("tok-rt")
			.with_history_length(3)
			.with_status_timestamp_after(datetime!(2026-03-01 00:00:00 UTC))
			.with_include_artifacts(true);

		let json = serde_json::to_string(&params).unwrap();
		let back: ListTasksParams = serde_json::from_str(&json).unwrap();
		assert_eq!(back, params);
	}

	// Deserialise a ListTasksParams from wire JSON as it would arrive
	// from an external A2A implementation. All keys are camelCase.
	// This validates the inbound parsing path for the ListTasks request.
	#[test]
	fn list_tasks_params_deserialise_from_wire() {
		let wire = r#"{
            "contextId": "ctx-external",
            "pageSize": 100,
            "includeArtifacts": false
        }"#;
		let params: ListTasksParams = serde_json::from_str(wire).unwrap();
		assert_eq!(params.context_id, Some("ctx-external".into()));
		assert_eq!(params.page_size, Some(100));
		assert_eq!(params.include_artifacts, Some(false));
		assert!(params.tenant.is_none());
		assert!(params.status.is_none());
	}

	// ListTasksResponse with tasks and a pagination token must serialise
	// correctly. All four fields always appear on the wire—next_page_token
	// is a required String (empty string means no further pages).
	#[test]
	fn list_tasks_response_wire_format() {
		use crate::task::{Task, TaskStatus};

		let task = Task::new(
			"task-list-1",
			"ctx-list-1",
			TaskStatus::submitted().with_timestamp(datetime!(2026-03-24 10:00:00 UTC)),
		);
		let response = ListTasksResponse {
			tasks: vec![task],
			page_size: 25,
			total_size: 1,
			next_page_token: "tok-next".into(),
		};

		let json = serde_json::to_value(&response).unwrap();
		assert!(json["tasks"].as_array().unwrap().len() == 1);
		assert_eq!(json["nextPageToken"], "tok-next");
		assert_eq!(json["pageSize"], 25, "pageSize must appear on the wire");
		assert_eq!(json["totalSize"], 1, "totalSize must appear on the wire");
	}

	// When there are no further pages the next_page_token is an empty
	// string. The tasks array must still appear even on the last page.
	// page_size and total_size are always present.
	#[test]
	fn list_tasks_response_empty_next_page_token_on_last_page() {
		use crate::task::{Task, TaskStatus};

		let task = Task::new(
			"task-list-2",
			"ctx-list-2",
			TaskStatus::submitted().with_timestamp(datetime!(2026-03-24 11:00:00 UTC)),
		);
		let response = ListTasksResponse {
			tasks: vec![task],
			page_size: 50,
			total_size: 1,
			next_page_token: String::new(),
		};

		let json = serde_json::to_value(&response).unwrap();
		let obj = json.as_object().unwrap();
		assert!(obj.contains_key("tasks"));
		assert_eq!(json["nextPageToken"], "");
	}

	// An empty tasks array is valid—it means no tasks matched the
	// filter. All four required fields must still be present on the wire.
	#[test]
	fn list_tasks_response_empty_tasks_array() {
		let response = ListTasksResponse {
			tasks: vec![],
			page_size: 20,
			total_size: 0,
			next_page_token: String::new(),
		};

		let json = serde_json::to_value(&response).unwrap();
		assert_eq!(json["tasks"].as_array().unwrap().len(), 0);
		let obj = json.as_object().unwrap();
		// All four fields are always present on the wire.
		assert_eq!(
			obj.len(),
			4,
			"tasks + page_size + total_size + next_page_token always present"
		);
		assert_eq!(json["pageSize"], 20);
		assert_eq!(json["totalSize"], 0);
		assert_eq!(json["nextPageToken"], "");
	}

	// ListTasksResponse must round-trip through serde so the client
	// can deserialise responses from remote agents correctly.
	#[test]
	fn list_tasks_response_round_trip() {
		use crate::task::{Task, TaskStatus};

		let task_a = Task::new(
			"task-rt-a",
			"ctx-rt",
			TaskStatus::working().with_timestamp(datetime!(2026-03-24 09:00:00 UTC)),
		);
		let task_b = Task::new(
			"task-rt-b",
			"ctx-rt",
			TaskStatus::completed().with_timestamp(datetime!(2026-03-24 09:30:00 UTC)),
		);
		let response = ListTasksResponse {
			tasks: vec![task_a, task_b],
			page_size: 10,
			total_size: 2,
			next_page_token: "cursor-rt".into(),
		};

		let json = serde_json::to_string(&response).unwrap();
		let back: ListTasksResponse = serde_json::from_str(&json).unwrap();
		assert_eq!(back, response);
		assert_eq!(back.tasks.len(), 2);
		assert_eq!(back.tasks[0].id, "task-rt-a");
		assert_eq!(back.tasks[1].id, "task-rt-b");
		assert_eq!(back.next_page_token, "cursor-rt");
	}
}
