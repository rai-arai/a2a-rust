// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Push notification operation types for the A2A protocol.
//!
//! Push notifications allow an agent to send status updates to a
//! caller's webhook instead of requiring the caller to poll. This
//! is essential for long-running tasks where the caller doesn't
//! want to hold an SSE connection open indefinitely.
//!
//! The flow is: the caller sets a push notification config on a task
//! (via `CreateTaskPushNotificationConfig`), and the agent POSTs
//! JSON-RPC notifications to the configured URL whenever the task's
//! status changes or new artifacts are produced.
//!
//! Each config carries its own authentication credentials so the
//! receiving endpoint can verify that notifications are genuine.
//! The protocol supports multiple configs per task, identified by
//! unique config IDs. The v1.0 wire format is fully flat—there is
//! no nested `PushNotificationConfig` wrapper.

use serde::{Deserialize, Serialize};

/// Authentication credentials for push notification delivery.
///
/// When the agent sends a notification to the webhook URL it includes
/// these credentials in the HTTP request (typically as a Bearer token
/// in the Authorization header). The receiving endpoint validates the
/// credentials before processing the notification.
///
/// `scheme` is required; `credentials` is optional and may be set
/// after construction via [`with_credentials`]. Use
/// [`AuthenticationInfo::bearer`] as the most common construction path,
/// or [`AuthenticationInfo::new`] when the scheme is not "Bearer".
///
/// The `Debug` implementation redacts the `credentials` field so that
/// tokens and API keys do not leak into logs or diagnostic output.
///
/// [`with_credentials`]: AuthenticationInfo::with_credentials
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationInfo {
	/// HTTP authentication scheme name (e.g. "Bearer" or "Basic").
	/// A single required scheme keeps the wire format simple—callers
	/// that need multi-scheme support should create separate configs.
	pub scheme: String,

	/// The credential value (token, API key, etc.).
	/// The format depends on the scheme—for Bearer this is the raw
	/// token string, for Basic it is the base64-encoded
	/// username:password pair. Optional so the scheme can be declared
	/// separately from credential provisioning.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub credentials: Option<String>,
}

impl AuthenticationInfo {
	/// Create authentication info with the given scheme and credentials.
	///
	/// `credentials` is wrapped in `Some` automatically. For the common
	/// Bearer token pattern prefer [`bearer`]. To construct with only a
	/// scheme and add credentials later, use the default constructor and
	/// chain [`with_credentials`].
	///
	/// [`with_credentials`]: AuthenticationInfo::with_credentials
	///
	/// ```
	/// # use a2a::AuthenticationInfo;
	/// let auth = AuthenticationInfo::new("Bearer", "tok_secret_123");
	/// assert_eq!(auth.scheme, "Bearer");
	/// assert_eq!(auth.credentials.as_deref(), Some("tok_secret_123"));
	/// ```
	#[must_use]
	pub fn new(scheme: impl Into<String>, credentials: impl Into<String>) -> Self {
		Self {
			scheme: scheme.into(),
			credentials: Some(credentials.into()),
		}
	}

	/// Create Bearer token authentication with the given credential.
	///
	/// This is the most common authentication pattern for push
	/// notification webhooks. The agent includes the token as a
	/// Bearer credential in the Authorization header when `POSTing`
	/// notifications. Equivalent to `new("Bearer", credentials)`.
	///
	/// ```
	/// # use a2a::AuthenticationInfo;
	/// let auth = AuthenticationInfo::bearer("tok_secret_123");
	/// assert_eq!(auth.scheme, "Bearer");
	/// assert_eq!(auth.credentials.as_deref(), Some("tok_secret_123"));
	/// ```
	#[must_use]
	pub fn bearer(credentials: impl Into<String>) -> Self {
		Self::new("Bearer", credentials)
	}

	/// Set credentials on an existing `AuthenticationInfo`.
	///
	/// Useful when the scheme is known at construction time but
	/// credentials are resolved later—for example from a secret
	/// store or environment variable. Overwrites any previously
	/// set credentials.
	///
	/// ```
	/// # use a2a::AuthenticationInfo;
	/// let auth = AuthenticationInfo { scheme: "Bearer".into(), credentials: None }
	///     .with_credentials("tok_from_vault");
	/// assert_eq!(auth.credentials.as_deref(), Some("tok_from_vault"));
	/// ```
	#[must_use]
	pub fn with_credentials(mut self, credentials: impl Into<String>) -> Self {
		self.credentials = Some(credentials.into());
		self
	}
}

impl std::fmt::Debug for AuthenticationInfo {
	/// Formats the struct for diagnostic output, redacting the
	/// `credentials` field so that tokens and API keys never appear
	/// in logs, panic messages, or test output—regardless of how
	/// the struct is printed.
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AuthenticationInfo")
			.field("scheme", &self.scheme)
			.field(
				"credentials",
				&self.credentials.as_ref().map(|_| "[REDACTED]"),
			)
			.finish()
	}
}

/// A push notification configuration for a task—serves dual duty as both
/// the CRUD request/response body and an embedded config in message sends.
///
/// ## Dual-role design
///
/// The A2A v1.0 proto defines separate request message types for each CRUD
/// operation (`CreateTaskPushNotificationConfigRequest`, `GetTaskPushNotificationConfigRequest`,
/// etc.). This crate collapses them into a single type for simplicity: the
/// same struct is used as the request/response body for all push notification
/// CRUD operations AND as the embedded config inside `SendMessageConfiguration`.
///
/// This works because the two usage contexts need different subsets of fields:
///
/// - **CRUD operations** (task already exists): supply `task_id` and optionally `id`.
///   Use [`new`][TaskPushNotificationConfig::new] to construct these.
/// - **Embedded in `SendMessageConfiguration`** (task not yet created): omit `task_id`
///   because the task doesn't exist when the message is sent. The agent assigns a
///   task ID and begins delivering notifications to the configured URL once the task
///   is created. Use [`for_url`][TaskPushNotificationConfig::for_url] to construct these.
///
/// Both `task_id` and `id` are `Option` rather than required fields because of this
/// dual-role design—not because they are semantically optional in both contexts.
///
/// ## Construction patterns
///
/// ```
/// # use a2a::{TaskPushNotificationConfig, AuthenticationInfo};
///
/// // For CRUD operations where the task already exists:
/// let crud_config = TaskPushNotificationConfig::new(
///     "task-1",
///     "https://hooks.example.com/notify",
/// );
///
/// // For embedding in SendMessageConfiguration before the task is created:
/// let embedded_config = TaskPushNotificationConfig::for_url(
///     "https://hooks.example.com/notify",
/// );
/// ```
///
/// Fields that were previously nested inside a `PushNotificationConfig` wrapper
/// are now flat at the top level—this is the v1.0 wire format change.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPushNotificationConfig {
	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,

	/// Optional identifier for this config.
	/// When multiple configs exist for a task the ID distinguishes
	/// them. The agent may assign an ID server-side if the caller
	/// omits one on creation.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub id: Option<String>,

	/// The task this notification config is associated with.
	/// The task must already exist on the agent—setting a config on
	/// a nonexistent task ID is an error (the agent returns
	/// `TaskNotFound`). Optional so the same type can represent a
	/// per-message push config in `SendMessageConfiguration` where no
	/// task ID is known at send time.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub task_id: Option<String>,

	/// The webhook URL the agent should POST notifications to.
	/// Should be an HTTPS URL in production—the agent sends
	/// JSON-RPC formatted notification payloads here.
	pub url: String,

	/// Optional session or correlation token.
	/// Included in notification payloads so the receiving endpoint
	/// can correlate notifications with its own internal state
	/// without maintaining a separate mapping.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub token: Option<String>,

	/// Optional authentication for the webhook endpoint.
	/// When provided the agent attaches these credentials to every
	/// HTTP request made to the webhook URL.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub authentication: Option<AuthenticationInfo>,
}

impl std::fmt::Debug for TaskPushNotificationConfig {
	/// Formats the config for diagnostic output, redacting the `token`
	/// field so that session tokens and correlation secrets never appear
	/// in logs, panic messages, or test output. The `authentication`
	/// field delegates to `AuthenticationInfo`'s own redacting Debug impl.
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("TaskPushNotificationConfig")
			.field("tenant", &self.tenant)
			.field("id", &self.id)
			.field("task_id", &self.task_id)
			.field("url", &self.url)
			.field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
			.field("authentication", &self.authentication)
			.finish()
	}
}

impl TaskPushNotificationConfig {
	/// Create a task-bound push notification config.
	///
	/// The task ID and webhook URL are required for task-level configs.
	/// All other optional fields (tenant, config ID, token, authentication)
	/// default to None and can be set through the chainable `with_*`
	/// methods. To create a config with only a URL (no task ID), use
	/// [`for_url`].
	///
	/// ```
	/// # use a2a::TaskPushNotificationConfig;
	/// let config = TaskPushNotificationConfig::new(
	///     "task-1",
	///     "https://hooks.example.com/notify",
	/// );
	/// assert_eq!(config.task_id.as_deref(), Some("task-1"));
	/// assert_eq!(config.url, "https://hooks.example.com/notify");
	/// assert!(config.id.is_none());
	/// ```
	#[must_use]
	pub fn new(task_id: impl Into<String>, url: impl Into<String>) -> Self {
		Self {
			tenant: None,
			id: None,
			task_id: Some(task_id.into()),
			url: url.into(),
			token: None,
			authentication: None,
		}
	}

	/// Create a push notification config with only a webhook URL.
	///
	/// Used in `SendMessageConfiguration` where the task ID is not yet
	/// known at send time. The agent assigns a task ID when it creates
	/// the task and includes the task ID in subsequent notifications to
	/// the configured webhook URL.
	///
	/// ```
	/// # use a2a::TaskPushNotificationConfig;
	/// let config = TaskPushNotificationConfig::for_url(
	///     "https://hooks.example.com/notify",
	/// );
	/// assert!(config.task_id.is_none());
	/// assert_eq!(config.url, "https://hooks.example.com/notify");
	/// ```
	#[must_use]
	pub fn for_url(url: impl Into<String>) -> Self {
		Self {
			tenant: None,
			id: None,
			task_id: None,
			url: url.into(),
			token: None,
			authentication: None,
		}
	}

	/// Set the task ID on this config.
	///
	/// Useful when updating a config created with `for_url()` once
	/// the task ID becomes known, or to override the task ID on an
	/// existing config in multi-task scenarios.
	#[must_use]
	pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
		self.task_id = Some(task_id.into());
		self
	}

	/// Set the tenant scope for multi-tenant deployments.
	///
	/// Isolates this config to the correct tenant's namespace when
	/// the agent is shared across multiple organisations.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}

	/// Set the config identifier.
	///
	/// When a task has multiple push notification configs (different
	/// endpoints for different event types, or primary/fallback
	/// webhooks) the ID distinguishes them. The agent may also
	/// assign an ID server-side if the caller omits one.
	#[must_use]
	pub fn with_id(mut self, id: impl Into<String>) -> Self {
		self.id = Some(id.into());
		self
	}

	/// Set the session or correlation token.
	///
	/// The agent includes this token in every notification payload
	/// it sends to the webhook, allowing the receiving endpoint to
	/// correlate incoming notifications with its own internal state
	/// (session IDs, request traces, workflow runs, etc.).
	#[must_use]
	pub fn with_token(mut self, token: impl Into<String>) -> Self {
		self.token = Some(token.into());
		self
	}

	/// Set the authentication credentials for the webhook endpoint.
	///
	/// When provided the agent attaches these credentials to every
	/// HTTP request it makes to the webhook URL. This lets the
	/// receiving endpoint verify that notifications are genuine
	/// rather than spoofed by a third party.
	#[must_use]
	pub fn with_authentication(mut self, authentication: AuthenticationInfo) -> Self {
		self.authentication = Some(authentication);
		self
	}
}

/// Parameters for the `GetTaskPushNotificationConfig` operation.
///
/// Retrieves a specific push notification config for a task.
/// Both the task ID and the config ID are required—this matches
/// the v1.0 spec where every config has a stable ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskPushNotificationConfigParams {
	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,

	/// The task whose notification config to retrieve.
	pub task_id: String,

	/// The identifier of the specific config to retrieve.
	pub id: String,
}

impl GetTaskPushNotificationConfigParams {
	/// Create params to retrieve a push notification config.
	///
	/// Both `task_id` and `id` are required—unlike the old API,
	/// v1.0 always requires an explicit config ID to get a specific
	/// config. Chain `with_tenant()` to scope to a tenant namespace.
	///
	/// ```
	/// # use a2a::GetTaskPushNotificationConfigParams;
	/// let params = GetTaskPushNotificationConfigParams::new("task-1", "cfg-1");
	/// assert_eq!(params.task_id, "task-1");
	/// assert_eq!(params.id, "cfg-1");
	/// assert!(params.tenant.is_none());
	/// ```
	#[must_use]
	pub fn new(task_id: impl Into<String>, id: impl Into<String>) -> Self {
		Self {
			tenant: None,
			task_id: task_id.into(),
			id: id.into(),
		}
	}

	/// Scope the request to a specific tenant.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

/// Parameters for the `ListTaskPushNotificationConfigs` operation.
///
/// Lists all push notification configs associated with a task,
/// with optional cursor-based pagination for tasks that have many
/// configs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskPushNotificationConfigsParams {
	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,

	/// The task whose notification configs to list.
	pub task_id: String,

	/// Maximum number of configs to return in one page.
	/// When omitted the agent uses its own default page size.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub page_size: Option<i32>,

	/// Cursor token from the previous page's `next_page_token`.
	/// When omitted the agent returns the first page.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub page_token: Option<String>,
}

impl ListTaskPushNotificationConfigsParams {
	/// Create params to list all push notification configs for a task.
	///
	/// Returns all configs associated with the given task ID, subject
	/// to pagination. Optional fields default to None; chain
	/// `with_tenant()`, `with_page_size()`, and `with_page_token()`
	/// to control scoping and pagination.
	///
	/// ```
	/// # use a2a::ListTaskPushNotificationConfigsParams;
	/// let params = ListTaskPushNotificationConfigsParams::new("task-1");
	/// assert_eq!(params.task_id, "task-1");
	/// assert!(params.page_size.is_none());
	/// ```
	#[must_use]
	pub fn new(task_id: impl Into<String>) -> Self {
		Self {
			tenant: None,
			task_id: task_id.into(),
			page_size: None,
			page_token: None,
		}
	}

	/// Scope the request to a specific tenant.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}

	/// Set the maximum number of configs to return per page.
	#[must_use]
	pub fn with_page_size(mut self, page_size: i32) -> Self {
		self.page_size = Some(page_size);
		self
	}

	/// Set the cursor token to continue from a previous page.
	#[must_use]
	pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
		self.page_token = Some(page_token.into());
		self
	}
}

/// Parameters for the `DeleteTaskPushNotificationConfig` operation.
///
/// Removes a push notification config from a task. After deletion
/// the agent stops sending notifications to the config's webhook URL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTaskPushNotificationConfigParams {
	/// Optional tenant identifier for multi-tenant deployments. This is a
	/// crate extension—the A2A proto carries tenant as an HTTP path
	/// parameter, but the JSON-RPC binding includes it in the request body
	/// for routing.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub tenant: Option<String>,

	/// The task whose notification config to delete.
	pub task_id: String,

	/// The identifier of the config to delete.
	pub id: String,
}

impl DeleteTaskPushNotificationConfigParams {
	/// Create params to delete a specific push notification config.
	///
	/// Both `task_id` and `id` are required—the agent needs both to
	/// identify which config to remove. After deletion the agent stops
	/// delivering notifications to the config's webhook URL.
	///
	/// ```
	/// # use a2a::DeleteTaskPushNotificationConfigParams;
	/// let params = DeleteTaskPushNotificationConfigParams::new("task-1", "cfg-1");
	/// assert_eq!(params.task_id, "task-1");
	/// assert_eq!(params.id, "cfg-1");
	/// ```
	#[must_use]
	pub fn new(task_id: impl Into<String>, id: impl Into<String>) -> Self {
		Self {
			tenant: None,
			task_id: task_id.into(),
			id: id.into(),
		}
	}

	/// Scope the request to a specific tenant.
	#[must_use]
	pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
		self.tenant = Some(tenant.into());
		self
	}
}

/// The result of a `ListTaskPushNotificationConfigs` operation.
///
/// Wraps the list of configs with a pagination cursor that mirrors the
/// pattern used by `ListTasksResponse`. When `next_page_token` is
/// non-empty there are more configs available—pass the token to the
/// next list call to continue. An empty string indicates this is the
/// final page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskPushNotificationConfigsResponse {
	/// The push notification configs for this page. Defaults to empty
	/// when absent on the wire, matching proto3 repeated field semantics.
	#[serde(default)]
	pub configs: Vec<TaskPushNotificationConfig>,

	/// Opaque cursor token for retrieving the next page of results.
	/// Empty string when this response is the final page. Defaults to
	/// empty when absent on the wire, matching proto3 string zero-value.
	#[serde(default)]
	pub next_page_token: String,
}

impl ListTaskPushNotificationConfigsResponse {
	/// Whether there are more pages of configs available.
	///
	/// Returns `true` when `next_page_token` is non-empty—pass the
	/// token to the next list request to retrieve the following page.
	/// Returns `false` when this response is the final page.
	#[must_use]
	pub fn has_next_page(&self) -> bool {
		!self.next_page_token.is_empty()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// AuthenticationInfo::new() wraps credentials in Some automatically.
	// Both scheme and credentials must be stored as provided.
	#[test]
	fn authentication_info_new_stores_both_fields() {
		let auth = AuthenticationInfo::new("Bearer", "tok_secret_123");
		assert_eq!(auth.scheme, "Bearer");
		assert_eq!(auth.credentials.as_deref(), Some("tok_secret_123"));
	}

	// AuthenticationInfo::bearer() is the most common construction
	// path—it should produce scheme="Bearer" with credentials wrapped
	// in Some, ready to use without further chaining.
	#[test]
	fn authentication_info_bearer_sets_scheme_and_credentials() {
		let auth = AuthenticationInfo::bearer("tok_secret");
		assert_eq!(auth.scheme, "Bearer");
		assert_eq!(auth.credentials.as_deref(), Some("tok_secret"));
	}

	// AuthenticationInfo::bearer() must be equivalent to
	// AuthenticationInfo::new("Bearer", ...). If they diverge the
	// shorthand factory becomes misleading.
	#[test]
	fn authentication_info_bearer_matches_new_with_bearer_scheme() {
		let via_bearer = AuthenticationInfo::bearer("my-token");
		let via_new = AuthenticationInfo::new("Bearer", "my-token");
		assert_eq!(via_bearer, via_new);
	}

	// AuthenticationInfo serialises with camelCase field names.
	// When credentials are set, both fields must appear on the wire.
	#[test]
	fn authentication_info_serialises_correctly() {
		let auth = AuthenticationInfo::bearer("tok_abc");
		let json = serde_json::to_value(&auth).unwrap();
		assert_eq!(json["scheme"], "Bearer");
		assert_eq!(json["credentials"], "tok_abc");
		let obj = json.as_object().unwrap();
		assert_eq!(obj.len(), 2, "exactly two fields on the wire");
	}

	// AuthenticationInfo omits credentials from the wire when None.
	// A scheme-only auth info is valid—credentials may be provisioned
	// later via with_credentials().
	#[test]
	fn authentication_info_omits_absent_credentials() {
		let auth = AuthenticationInfo {
			scheme: "Bearer".into(),
			credentials: None,
		};
		let json = serde_json::to_value(&auth).unwrap();
		let obj = json.as_object().unwrap();
		assert_eq!(
			obj.len(),
			1,
			"only scheme on the wire when credentials absent"
		);
		assert!(obj.get("credentials").is_none());
	}

	// with_credentials() sets credentials on an existing AuthenticationInfo.
	// Useful when credentials come from a secret store after construction.
	#[test]
	fn authentication_info_with_credentials_sets_field() {
		let auth = AuthenticationInfo {
			scheme: "Bearer".into(),
			credentials: None,
		}
		.with_credentials("tok_from_vault");
		assert_eq!(auth.credentials.as_deref(), Some("tok_from_vault"));
	}

	// AuthenticationInfo round-trips through serde without data loss.
	// Both the populated and None-credentials cases must survive.
	#[test]
	fn authentication_info_round_trips() {
		let auth = AuthenticationInfo::new("ApiKey", "key_12345");
		let json = serde_json::to_string(&auth).unwrap();
		let back: AuthenticationInfo = serde_json::from_str(&json).unwrap();
		assert_eq!(back, auth);
	}

	// TaskPushNotificationConfig::new() sets task_id (as Some) and url
	// and leaves every other field as None—confirming the two-argument
	// constructor is the minimal valid task-bound construction.
	#[test]
	fn task_push_notification_config_new_sets_required_fields() {
		let config = TaskPushNotificationConfig::new("task-1", "https://hooks.example.com/notify");
		assert_eq!(config.task_id.as_deref(), Some("task-1"));
		assert_eq!(config.url, "https://hooks.example.com/notify");
		assert!(config.tenant.is_none());
		assert!(config.id.is_none());
		assert!(config.token.is_none());
		assert!(config.authentication.is_none());
	}

	// A minimal TaskPushNotificationConfig (task_id + url only) must
	// emit exactly those two keys on the wire. Optional fields that are
	// None must be absent, not null.
	#[test]
	fn task_push_notification_config_omits_none_fields() {
		let config = TaskPushNotificationConfig::new("task-min", "https://example.com/hook");
		let json = serde_json::to_value(&config).unwrap();
		let obj = json.as_object().unwrap();
		assert_eq!(obj.len(), 2, "only taskId and url should be present");
		assert_eq!(json["taskId"], "task-min");
		assert_eq!(json["url"], "https://example.com/hook");
	}

	// TaskPushNotificationConfig::for_url() creates a config with only
	// the webhook URL—no task ID. This is the construction path for
	// per-message push configs in SendMessageConfiguration where the
	// task ID is not yet known at send time.
	#[test]
	fn task_push_notification_config_for_url_has_no_task_id() {
		let config = TaskPushNotificationConfig::for_url("https://hooks.example.com/msg");
		assert!(config.task_id.is_none());
		assert_eq!(config.url, "https://hooks.example.com/msg");
		let json = serde_json::to_value(&config).unwrap();
		let obj = json.as_object().unwrap();
		assert_eq!(obj.len(), 1, "only url should be present");
		assert!(obj.get("taskId").is_none(), "taskId must be absent");
	}

	// with_task_id() attaches a task ID to a config that was created
	// without one (e.g. via for_url()). The resulting config must
	// serialise taskId into the wire format.
	#[test]
	fn task_push_notification_config_with_task_id() {
		let config = TaskPushNotificationConfig::for_url("https://hooks.example.com/notify")
			.with_task_id("task-from-setter");
		assert_eq!(config.task_id.as_deref(), Some("task-from-setter"));
		let json = serde_json::to_value(&config).unwrap();
		assert_eq!(json["taskId"], "task-from-setter");
	}

	// The chainable setters on TaskPushNotificationConfig each
	// populate exactly one optional field without clobbering the
	// others. Exercise all four setters in sequence.
	#[test]
	fn task_push_notification_config_setters_populate_all_optional_fields() {
		let config = TaskPushNotificationConfig::new("task-42", "https://hooks.example.com/a2a")
			.with_tenant("acme-corp")
			.with_id("cfg-99")
			.with_token("session-xyz")
			.with_authentication(AuthenticationInfo::bearer("tok_123"));

		assert_eq!(config.task_id.as_deref(), Some("task-42"));
		assert_eq!(config.url, "https://hooks.example.com/a2a");
		assert_eq!(config.tenant.as_deref(), Some("acme-corp"));
		assert_eq!(config.id.as_deref(), Some("cfg-99"));
		assert_eq!(config.token.as_deref(), Some("session-xyz"));
		let auth = config.authentication.unwrap();
		assert_eq!(auth.scheme, "Bearer");
		assert_eq!(auth.credentials.as_deref(), Some("tok_123"));
	}

	// A fully populated TaskPushNotificationConfig must round-trip
	// through serde without losing any field. This is the shape a
	// production caller would send for a fully secured webhook.
	#[test]
	fn task_push_notification_config_full_round_trip() {
		let config = TaskPushNotificationConfig::new("task-rt", "https://hooks.example.com/rt")
			.with_tenant("tenant-rt")
			.with_id("cfg-rt")
			.with_token("token-rt")
			.with_authentication(AuthenticationInfo::new("Bearer", "secret-rt"));

		let json = serde_json::to_string(&config).unwrap();
		let back: TaskPushNotificationConfig = serde_json::from_str(&json).unwrap();
		assert_eq!(back, config);
	}

	// TaskPushNotificationConfig uses camelCase on the wire.
	// The v1.0 flat format must have taskId and url at the top
	// level—there must be no nested pushNotificationConfig wrapper.
	#[test]
	fn task_push_notification_config_is_flat_camel_case() {
		let config = TaskPushNotificationConfig::new("task-flat", "https://example.com/flat")
			.with_id("cfg-flat");
		let json = serde_json::to_value(&config).unwrap();

		assert!(json.get("taskId").is_some(), "must have taskId at root");
		assert!(json.get("url").is_some(), "must have url at root");
		assert!(json.get("id").is_some(), "must have id at root");
		assert!(
			json.get("pushNotificationConfig").is_none(),
			"must NOT have nested pushNotificationConfig wrapper"
		);
		assert!(json.get("task_id").is_none(), "must not use snake_case");
	}

	// The builder-constructed config must produce the same wire format
	// as a struct literal with equivalent field values. This guards
	// against the builder accidentally setting fields differently.
	#[test]
	fn task_push_notification_config_builder_matches_struct_literal() {
		let from_builder =
			TaskPushNotificationConfig::new("task-cmp", "https://example.com/cmp-hook")
				.with_id("cfg-cmp")
				.with_token("tok-cmp");

		let from_literal = TaskPushNotificationConfig {
			tenant: None,
			id: Some("cfg-cmp".into()),
			task_id: Some("task-cmp".into()),
			url: "https://example.com/cmp-hook".into(),
			token: Some("tok-cmp".into()),
			authentication: None,
		};

		assert_eq!(
			serde_json::to_value(&from_builder).unwrap(),
			serde_json::to_value(&from_literal).unwrap()
		);
	}

	// GetTaskPushNotificationConfigParams::new() requires both task_id
	// and id—this is the v1.0 change from the old API where config_id
	// was optional. Tenant defaults to None.
	#[test]
	fn get_params_new_sets_required_fields() {
		let params = GetTaskPushNotificationConfigParams::new("task-get-1", "cfg-get-1");
		assert_eq!(params.task_id, "task-get-1");
		assert_eq!(params.id, "cfg-get-1");
		assert!(params.tenant.is_none());
	}

	// with_tenant() on get params scopes the retrieval to a specific
	// tenant namespace in multi-tenant deployments.
	#[test]
	fn get_params_with_tenant() {
		let params =
			GetTaskPushNotificationConfigParams::new("task-get-2", "cfg-get-2").with_tenant("org");
		assert_eq!(params.tenant.as_deref(), Some("org"));
	}

	// GetTaskPushNotificationConfigParams serialises with camelCase
	// field names. The id field is the config ID at root level, not
	// the old pushNotificationConfigId.
	#[test]
	fn get_params_camel_case_wire_format() {
		let params = GetTaskPushNotificationConfigParams::new("task-get-3", "cfg-get-3");
		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["taskId"], "task-get-3");
		assert_eq!(json["id"], "cfg-get-3");
		assert!(
			json.get("pushNotificationConfigId").is_none(),
			"old field name must not appear"
		);
	}

	// GetTaskPushNotificationConfigParams round-trips through serde.
	// This confirms the two required fields survive serialisation
	// and deserialisation intact.
	#[test]
	fn get_params_round_trips() {
		let params = GetTaskPushNotificationConfigParams::new("task-rt-get", "cfg-rt-get")
			.with_tenant("rt-tenant");
		let json = serde_json::to_string(&params).unwrap();
		let back: GetTaskPushNotificationConfigParams = serde_json::from_str(&json).unwrap();
		assert_eq!(back, params);
	}

	// ListTaskPushNotificationConfigsParams::new() requires only
	// task_id. Pagination fields and tenant default to None.
	#[test]
	fn list_params_new_sets_task_id_only() {
		let params = ListTaskPushNotificationConfigsParams::new("task-list-1");
		assert_eq!(params.task_id, "task-list-1");
		assert!(params.tenant.is_none());
		assert!(params.page_size.is_none());
		assert!(params.page_token.is_none());
	}

	// The chainable setters on list params each populate exactly one
	// optional field—none of them should clobber the others.
	#[test]
	fn list_params_setters_populate_all_optional_fields() {
		let params = ListTaskPushNotificationConfigsParams::new("task-list-2")
			.with_tenant("list-tenant")
			.with_page_size(25)
			.with_page_token("tok-page-2");

		assert_eq!(params.tenant.as_deref(), Some("list-tenant"));
		assert_eq!(params.page_size, Some(25));
		assert_eq!(params.page_token.as_deref(), Some("tok-page-2"));
	}

	// ListTaskPushNotificationConfigsParams serialises optional fields
	// only when they are set, and emits camelCase keys.
	#[test]
	fn list_params_omits_none_and_uses_camel_case() {
		let params = ListTaskPushNotificationConfigsParams::new("task-list-3").with_page_size(10);
		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["taskId"], "task-list-3");
		assert_eq!(json["pageSize"], 10);
		assert!(json.get("tenant").is_none(), "absent when None");
		assert!(json.get("pageToken").is_none(), "absent when None");
	}

	// DeleteTaskPushNotificationConfigParams::new() requires both
	// task_id and id. Tenant defaults to None.
	#[test]
	fn delete_params_new_sets_required_fields() {
		let params = DeleteTaskPushNotificationConfigParams::new("task-del-1", "cfg-del-1");
		assert_eq!(params.task_id, "task-del-1");
		assert_eq!(params.id, "cfg-del-1");
		assert!(params.tenant.is_none());
	}

	// with_tenant() on delete params scopes the deletion to the
	// correct tenant namespace. Without it a multi-tenant agent
	// cannot determine which tenant's config to remove.
	#[test]
	fn delete_params_with_tenant() {
		let params =
			DeleteTaskPushNotificationConfigParams::new("task-del-2", "cfg-del-2").with_tenant("t");
		assert_eq!(params.tenant.as_deref(), Some("t"));
	}

	// DeleteTaskPushNotificationConfigParams serialises with camelCase
	// keys. The id field at root level is the config ID—not the old
	// pushNotificationConfigId.
	#[test]
	fn delete_params_camel_case_wire_format() {
		let params = DeleteTaskPushNotificationConfigParams::new("task-del-3", "cfg-del-3");
		let json = serde_json::to_value(&params).unwrap();
		assert_eq!(json["taskId"], "task-del-3");
		assert_eq!(json["id"], "cfg-del-3");
		assert!(
			json.get("pushNotificationConfigId").is_none(),
			"old field name must not appear"
		);
	}

	// DeleteTaskPushNotificationConfigParams round-trips through serde.
	#[test]
	fn delete_params_round_trips() {
		let params = DeleteTaskPushNotificationConfigParams::new("task-rt-del", "cfg-rt-del")
			.with_tenant("rt-del-tenant");
		let json = serde_json::to_string(&params).unwrap();
		let back: DeleteTaskPushNotificationConfigParams = serde_json::from_str(&json).unwrap();
		assert_eq!(back, params);
	}

	// ListTaskPushNotificationConfigsResponse wraps the configs list with a
	// required pagination cursor string, matching the ListTasksResponse
	// pattern. An empty next_page_token signals the final page—it must
	// still appear on the wire as an empty string, not be absent.
	#[test]
	fn list_response_final_page_has_empty_token() {
		let response = ListTaskPushNotificationConfigsResponse {
			configs: vec![],
			next_page_token: String::new(),
		};
		let json = serde_json::to_value(&response).unwrap();
		let obj = json.as_object().unwrap();

		// next_page_token is a required String—it must appear on the wire
		// even when empty, so callers can detect the final page without
		// relying on field absence.
		assert!(
			obj.contains_key("nextPageToken"),
			"nextPageToken must be present even when empty"
		);
		assert_eq!(json["nextPageToken"], "");
		assert!(json["configs"].as_array().unwrap().is_empty());
	}

	// has_next_page() returns true only when the token is non-empty.
	// This is the idiomatic way to check for further pages without
	// comparing against an empty string at every call site.
	#[test]
	fn list_response_has_next_page() {
		let with_token = ListTaskPushNotificationConfigsResponse {
			configs: vec![],
			next_page_token: "cursor-abc".into(),
		};
		assert!(
			with_token.has_next_page(),
			"has_next_page must return true when token is non-empty"
		);

		let without_token = ListTaskPushNotificationConfigsResponse {
			configs: vec![],
			next_page_token: String::new(),
		};
		assert!(
			!without_token.has_next_page(),
			"has_next_page must return false when token is empty"
		);
	}

	// ListTaskPushNotificationConfigsResponse serialises the non-empty
	// token and uses camelCase for nextPageToken.
	#[test]
	fn list_response_serialises_with_next_page_token() {
		let config = TaskPushNotificationConfig::new("task-lr-1", "https://hooks.example.com/lr");
		let response = ListTaskPushNotificationConfigsResponse {
			configs: vec![config],
			next_page_token: "cursor-abc".into(),
		};
		let json = serde_json::to_value(&response).unwrap();
		assert_eq!(json["nextPageToken"], "cursor-abc");
		assert_eq!(json["configs"].as_array().unwrap().len(), 1);
	}

	// ListTaskPushNotificationConfigsResponse round-trips through serde
	// with populated configs and a pagination cursor.
	#[test]
	fn list_response_round_trips() {
		let config = TaskPushNotificationConfig::new("task-lr-rt", "https://hooks.example.com/rt")
			.with_id("cfg-lr-rt")
			.with_authentication(AuthenticationInfo::bearer("tok-rt"));

		let response = ListTaskPushNotificationConfigsResponse {
			configs: vec![config],
			next_page_token: "next-page-cursor".into(),
		};
		let json = serde_json::to_string(&response).unwrap();
		let back: ListTaskPushNotificationConfigsResponse = serde_json::from_str(&json).unwrap();
		assert_eq!(back, response);
	}

	// Deserialise a TaskPushNotificationConfig from the v1.0 flat
	// wire format (as produced by an external agent) to confirm
	// the struct handles real incoming JSON correctly.
	#[test]
	fn task_push_notification_config_deserialises_from_wire() {
		let json = r#"{
            "taskId": "task-wire-1",
            "url": "https://external.example.com/hook",
            "id": "cfg-wire-1",
            "authentication": {
                "scheme": "Bearer",
                "credentials": "external-token"
            }
        }"#;
		let config: TaskPushNotificationConfig = serde_json::from_str(json).unwrap();
		assert_eq!(config.task_id.as_deref(), Some("task-wire-1"));
		assert_eq!(config.url, "https://external.example.com/hook");
		assert_eq!(config.id.as_deref(), Some("cfg-wire-1"));
		let auth = config.authentication.unwrap();
		assert_eq!(auth.scheme, "Bearer");
		assert_eq!(auth.credentials.as_deref(), Some("external-token"));
	}
}
