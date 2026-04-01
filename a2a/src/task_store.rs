// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Storage abstraction for A2A task persistence.
//!
//! The A2A protocol defines five task management operations (`GetTask`,
//! `ListTasks`, `CancelTask`, plus `SendMessage`/`SendStreamingMessage`
//! which create and update tasks). Every server implementation needs to
//! persist Task records to back these operations, but the protocol says
//! nothing about how—that's a deployment decision.
//!
//! This module defines the `TaskStore` trait that storage backends
//! implement. The trait operates directly on the crate's own domain
//! types (`Task`, `ListTasksParams`, `ListTasksResponse`) with no
//! intermediate representation. Implementations choose the backend—
//! in-memory, `SQLite`, Postgres, Redis—at compile time via the
//! `a2a-store` crate or by implementing the trait directly.

use std::future::Future;

use crate::error::A2AError;
use crate::operation::task_management::{ListTasksParams, ListTasksResponse};
use crate::task::Task;

/// Storage backend for A2A task persistence.
///
/// Implementations must be safe to share across threads (`Send + Sync`)
/// since handlers typically hold the store behind an `Arc` and serve
/// concurrent requests.
///
/// All methods return `A2AError` for consistency with the protocol
/// error type. Store-level errors (database connection failures, query
/// errors, serialisation problems) should map to
/// `A2AError::internal_error()` with a diagnostic message that does
/// not expose internal details to the caller.
///
/// The trait uses native async fn in traits (AFIT) for zero-cost
/// monomorphisation and wasm32 compatibility. Callers who need runtime
/// polymorphism (`dyn TaskStore`) can wrap implementations in their
/// own boxing layer.
pub trait TaskStore: Send + Sync {
	/// Persist a task, creating or replacing any existing task with the
	/// same ID.
	///
	/// Called after every state transition—initial creation from
	/// `SendMessage`, status updates from the handler, and cancellation
	/// from `CancelTask`. Must be idempotent: saving the same task ID
	/// twice overwrites the previous record.
	fn save(&self, task: &Task) -> impl Future<Output = Result<(), A2AError>> + Send;

	/// Load a task by its unique ID.
	///
	/// Returns `None` if no task with that ID exists in the store.
	/// Backs the `GetTask` protocol operation. Implementations should
	/// return a clone of the stored task—the caller owns the returned
	/// value independently of the store's internal state.
	fn load(&self, id: &str) -> impl Future<Output = Result<Option<Task>, A2AError>> + Send;

	/// Query tasks with filtering and pagination.
	///
	/// Backs the `ListTasks` protocol operation. The filters come from
	/// `ListTasksParams`—context ID, task state, timestamp range,
	/// pagination cursor, history length limits, and artifact inclusion.
	/// Implementations apply all applicable filters and return a
	/// paginated response with `total_size` reflecting the filtered
	/// count (not the page count).
	fn list(
		&self,
		params: &ListTasksParams,
	) -> impl Future<Output = Result<ListTasksResponse, A2AError>> + Send;

	/// Remove a task by its unique ID.
	///
	/// Not an A2A protocol operation—stores need a cleanup mechanism
	/// for TTL expiry, administrative deletion, and test teardown.
	/// Must be idempotent: removing a nonexistent ID returns `Ok(())`.
	fn remove(&self, id: &str) -> impl Future<Output = Result<(), A2AError>> + Send;
}
