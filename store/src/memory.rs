// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! In-memory task store backed by a `HashMap` behind a `tokio::sync::RwLock`.
//!
//! Suitable for tests, prototyping, and single-process deployments where
//! durability across restarts is not required. All data is lost when the
//! process exits.
//!
//! The `list` implementation performs a linear scan with predicate matching
//! on the full task set. This is adequate for in-memory use where the task
//! count is bounded by available memory. Production deployments with large
//! task volumes should use a database-backed implementation that pushes
//! filtering to the query layer.

use std::collections::HashMap;

use a2a::error::A2AError;
use a2a::operation::task_management::{ListTasksParams, ListTasksResponse};
use a2a::task::Task;
use a2a::task_store::TaskStore;

/// In-memory task store for tests and prototyping.
///
/// Uses `tokio::sync::RwLock` for concurrent reader access—multiple
/// `GetTask` calls can proceed in parallel while writes (save, remove)
/// take exclusive access. The lock is held only for the duration of the
/// `HashMap` operation, not across await points, so contention is minimal.
pub struct MemoryTaskStore {
	tasks: tokio::sync::RwLock<HashMap<String, Task>>,
}

impl MemoryTaskStore {
	/// Create an empty in-memory task store.
	#[must_use]
	pub fn new() -> Self {
		Self {
			tasks: tokio::sync::RwLock::new(HashMap::new()),
		}
	}
}

impl Default for MemoryTaskStore {
	fn default() -> Self {
		Self::new()
	}
}

impl TaskStore for MemoryTaskStore {
	async fn save(&self, task: &Task) -> Result<(), A2AError> {
		let mut store = self.tasks.write().await;
		store.insert(task.id.clone(), task.clone());
		Ok(())
	}

	async fn load(&self, id: &str) -> Result<Option<Task>, A2AError> {
		let store = self.tasks.read().await;
		Ok(store.get(id).cloned())
	}

	async fn list(&self, params: &ListTasksParams) -> Result<ListTasksResponse, A2AError> {
		let store = self.tasks.read().await;

		// Apply filters to produce the full matching set before pagination.
		let mut matching: Vec<Task> = store
			.values()
			.filter(|task| matches_filters(task, params))
			.cloned()
			.collect();

		// Sort by ID for deterministic pagination order. Without a sort,
		// the HashMap iteration order would vary across runs, making
		// page_token offsets unreliable.
		matching.sort_by(|first, second| first.id.cmp(&second.id));

		let total_size = matching.len();

		// Parse the page_token as an offset into the sorted result set.
		// An empty or absent token starts from the beginning.
		let offset: usize = params
			.page_token
			.as_deref()
			.and_then(|token| token.parse().ok())
			.unwrap_or(0);

		let page_size = usize::try_from(params.page_size.unwrap_or(20).max(1)).unwrap_or(20);

		let page: Vec<Task> = matching
			.into_iter()
			.skip(offset)
			.take(page_size)
			.map(|task| apply_response_limits(task, params))
			.collect();

		// If there are more results beyond this page, set the next token
		// to the offset of the first item on the next page.
		let next_offset = offset + page.len();
		let next_page_token = if next_offset < total_size {
			next_offset.to_string()
		} else {
			String::new()
		};

		Ok(ListTasksResponse {
			tasks: page,
			page_size: i32::try_from(page_size).unwrap_or(i32::MAX),
			total_size: i32::try_from(total_size).unwrap_or(i32::MAX),
			next_page_token,
		})
	}

	async fn remove(&self, id: &str) -> Result<(), A2AError> {
		let mut store = self.tasks.write().await;
		store.remove(id);
		Ok(())
	}
}

// Check whether a task matches all the filters specified in the params.
// Every filter is optional—absent filters match all tasks. All present
// filters must match (AND semantics).
fn matches_filters(task: &Task, params: &ListTasksParams) -> bool {
	if let Some(ref context_id) = params.context_id
		&& task.context_id != *context_id
	{
		return false;
	}

	if let Some(ref status) = params.status
		&& task.status.state != *status
	{
		return false;
	}

	if let Some(ref after) = params.status_timestamp_after {
		match task.status.timestamp {
			Some(ref timestamp) if timestamp >= after => {}
			_ => return false,
		}
	}

	true
}

// Apply response-level limits to a task before returning it in a list
// response. These don't affect which tasks match—they control how much
// detail each matched task carries in the response.
fn apply_response_limits(mut task: Task, params: &ListTasksParams) -> Task {
	if let Some(history_length) = params.history_length {
		let limit = usize::try_from(history_length.max(0)).unwrap_or(0);
		if task.history.len() > limit {
			let start = task.history.len() - limit;
			task.history = task.history.split_off(start);
		}
	}

	if let Some(false) = params.include_artifacts {
		task.artifacts.clear();
	}

	task
}

#[cfg(test)]
mod tests {
	use super::*;
	use a2a::task::TaskStatus;
	use a2a::task_state::TaskState;

	fn make_task(id: &str, context_id: &str, state: TaskState) -> Task {
		Task::new(id, context_id, TaskStatus::new(state))
	}

	// Saving a task and loading it back produces an identical value.
	// This is the fundamental contract of the store—what goes in must
	// come out unchanged.
	#[tokio::test]
	async fn save_and_load_round_trips() {
		let store = MemoryTaskStore::new();
		let task = make_task("task-1", "ctx-1", TaskState::Submitted);

		store.save(&task).await.unwrap();
		let loaded = store.load("task-1").await.unwrap();

		assert_eq!(loaded, Some(task));
	}

	// Loading a task that was never saved returns None, not an error.
	// This is how GetTask distinguishes "not found" from "store failure".
	#[tokio::test]
	async fn load_nonexistent_returns_none() {
		let store = MemoryTaskStore::new();
		let loaded = store.load("does-not-exist").await.unwrap();
		assert_eq!(loaded, None);
	}

	// Saving the same task ID twice overwrites the first version.
	// This is the idempotency guarantee—handlers can save after every
	// state transition without worrying about duplicates.
	#[tokio::test]
	async fn save_overwrites_existing() {
		let store = MemoryTaskStore::new();

		let initial = make_task("task-1", "ctx-1", TaskState::Submitted);
		store.save(&initial).await.unwrap();

		let updated = make_task("task-1", "ctx-1", TaskState::Working);
		store.save(&updated).await.unwrap();

		let loaded = store.load("task-1").await.unwrap().unwrap();
		assert_eq!(loaded.status.state, TaskState::Working);
	}

	// Removing a task makes it no longer loadable. The remove operation
	// returns Ok(()) regardless of whether the task existed—idempotent.
	#[tokio::test]
	async fn remove_deletes_task() {
		let store = MemoryTaskStore::new();
		let task = make_task("task-1", "ctx-1", TaskState::Completed);
		store.save(&task).await.unwrap();

		store.remove("task-1").await.unwrap();
		assert_eq!(store.load("task-1").await.unwrap(), None);
	}

	// Removing a nonexistent task is not an error. This makes cleanup
	// safe to retry without checking existence first.
	#[tokio::test]
	async fn remove_nonexistent_is_ok() {
		let store = MemoryTaskStore::new();
		assert!(store.remove("never-existed").await.is_ok());
	}

	// Listing with no filters returns all tasks. The response includes
	// pagination metadata even when everything fits on one page.
	#[tokio::test]
	async fn list_returns_all_tasks() {
		let store = MemoryTaskStore::new();
		store
			.save(&make_task("a", "ctx-1", TaskState::Submitted))
			.await
			.unwrap();
		store
			.save(&make_task("b", "ctx-1", TaskState::Working))
			.await
			.unwrap();

		let response = store.list(&ListTasksParams::new()).await.unwrap();

		assert_eq!(response.tasks.len(), 2);
		assert_eq!(response.total_size, 2);
		assert!(response.next_page_token.is_empty());
	}

	// Listing with a context_id filter returns only tasks in that context.
	#[tokio::test]
	async fn list_filters_by_context_id() {
		let store = MemoryTaskStore::new();
		store
			.save(&make_task("a", "ctx-1", TaskState::Submitted))
			.await
			.unwrap();
		store
			.save(&make_task("b", "ctx-2", TaskState::Submitted))
			.await
			.unwrap();
		store
			.save(&make_task("c", "ctx-1", TaskState::Working))
			.await
			.unwrap();

		let params = ListTasksParams::new().with_context_id("ctx-1");
		let response = store.list(&params).await.unwrap();

		assert_eq!(response.tasks.len(), 2);
		assert_eq!(response.total_size, 2);
		assert!(response.tasks.iter().all(|task| task.context_id == "ctx-1"));
	}

	// Listing with a status filter returns only tasks in that state.
	#[tokio::test]
	async fn list_filters_by_status() {
		let store = MemoryTaskStore::new();
		store
			.save(&make_task("a", "ctx-1", TaskState::Submitted))
			.await
			.unwrap();
		store
			.save(&make_task("b", "ctx-1", TaskState::Working))
			.await
			.unwrap();
		store
			.save(&make_task("c", "ctx-1", TaskState::Working))
			.await
			.unwrap();

		let params = ListTasksParams::new().with_status(TaskState::Working);
		let response = store.list(&params).await.unwrap();

		assert_eq!(response.tasks.len(), 2);
		assert!(
			response
				.tasks
				.iter()
				.all(|task| task.status.state == TaskState::Working)
		);
	}

	// Pagination splits the result set across multiple pages. The
	// next_page_token allows the caller to fetch subsequent pages
	// until the token is empty (final page).
	#[tokio::test]
	async fn list_paginates() {
		let store = MemoryTaskStore::new();
		for index in 0..5 {
			store
				.save(&make_task(
					&format!("task-{index}"),
					"ctx-1",
					TaskState::Submitted,
				))
				.await
				.unwrap();
		}

		let params = ListTasksParams::new().with_page_size(2);
		let page_one = store.list(&params).await.unwrap();

		assert_eq!(page_one.tasks.len(), 2);
		assert_eq!(page_one.total_size, 5);
		assert!(!page_one.next_page_token.is_empty());

		let page_two = store
			.list(&params.clone().with_page_token(&page_one.next_page_token))
			.await
			.unwrap();

		assert_eq!(page_two.tasks.len(), 2);
		assert!(!page_two.next_page_token.is_empty());

		let page_three = store
			.list(&params.clone().with_page_token(&page_two.next_page_token))
			.await
			.unwrap();

		assert_eq!(page_three.tasks.len(), 1);
		assert!(page_three.next_page_token.is_empty());
	}
}
