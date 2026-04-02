// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A stateful agent that demonstrates task lifecycle management
//! and multi-turn conversations.
//!
//! This agent maintains an in-memory task store and supports the
//! full task lifecycle: create via message/send, retrieve via
//! GetTask, and cancel via CancelTask. Tasks start in the
//! submitted state and move to working, then completed.
//!
//! The agent also demonstrates multi-turn interaction through the
//! input-required state. When it receives a message containing the
//! word "confirm", it pauses and asks the caller to confirm before
//! proceeding. The caller sends a follow-up message referencing the
//! task ID to continue.
//!
//! Run with:
//!   cargo run --example stateful_agent --features axum
//!
//! Test create + retrieve:
//!   # Create a task
//!   curl -X POST http://localhost:3002 \
//!     -H "Content-Type: application/json" \
//!     -d '{
//!       "jsonrpc": "2.0", "method": "SendMessage", "id": "1",
//!       "params": {
//!         "message": {
//!           "messageId": "msg-1", "role": "user",
//!           "parts": [{"text": "process this document"}]
//!         }
//!       }
//!     }'
//!
//!   # Retrieve the task
//!   curl -X POST http://localhost:3002 \
//!     -H "Content-Type: application/json" \
//!     -d '{
//!       "jsonrpc": "2.0", "method": "GetTask", "id": "2",
//!       "params": {"id": "task-1"}
//!     }'
//!
//! Test input_required flow (multi-turn confirmation):
//!   # Step 1: send a message containing "confirm"—the agent pauses
//!   # and returns a task in the input-required state, asking for
//!   # confirmation before proceeding.
//!   curl -X POST http://localhost:3002 \
//!     -H "Content-Type: application/json" \
//!     -d '{
//!       "jsonrpc": "2.0", "method": "SendMessage", "id": "3",
//!       "params": {
//!         "message": {
//!           "messageId": "msg-2", "role": "user",
//!           "parts": [{"text": "please confirm this action"}]
//!         }
//!       }
//!     }'
//!   # The response carries {"task": {"id": "task-2", "status": {"state": "input-required", ...}}}
//!
//!   # Step 2: send a follow-up message referencing the task ID with
//!   # "yes" to confirm. The agent transitions to working then completed.
//!   curl -X POST http://localhost:3002 \
//!     -H "Content-Type: application/json" \
//!     -d '{
//!       "jsonrpc": "2.0", "method": "SendMessage", "id": "4",
//!       "params": {
//!         "message": {
//!           "messageId": "msg-3", "role": "user",
//!           "taskId": "task-2",
//!           "parts": [{"text": "yes"}]
//!         }
//!       }
//!     }'

use std::collections::HashMap;
use std::sync::Mutex;

use a2a::{
	A2AError, AgentCapabilities, AgentCard, AgentCardRequired, AgentInterface, AgentSkill,
	Artifact, CancelTaskParams, GetTaskParams, Message, Part, PartContent, Role, SendMessageParams,
	SendMessageResult, Task, TaskStatus,
};
use a2a_axum::a2a_router;
use a2a_server::{A2AHandler, RequestContext};

/// In-memory task store for the stateful agent.
///
/// Protected by a Mutex for thread-safe access across concurrent
/// requests. In a production agent, this would be a database or
/// distributed cache. The Mutex is fine here because the critical
/// sections are tiny (HashMap lookups and inserts).
struct TaskStore {
	tasks: Mutex<HashMap<String, Task>>,
	next_task_number: Mutex<u64>,
}

impl TaskStore {
	fn new() -> Self {
		Self {
			tasks: Mutex::new(HashMap::new()),
			next_task_number: Mutex::new(1),
		}
	}

	fn next_task_id(&self) -> String {
		let mut counter = self.next_task_number.lock().unwrap();
		let id = format!("task-{counter}");
		*counter += 1;
		id
	}

	fn store(&self, task: Task) {
		self.tasks.lock().unwrap().insert(task.id.clone(), task);
	}

	fn get(&self, task_id: &str) -> Option<Task> {
		self.tasks.lock().unwrap().get(task_id).cloned()
	}
}

/// A stateful agent that manages task lifecycle.
///
/// Holds an in-memory task store and supports creating, retrieving,
/// and canceling tasks. Demonstrates how agents maintain state across
/// multiple JSON-RPC calls from the same or different callers.
struct StatefulAgent {
	store: TaskStore,
}

impl A2AHandler for StatefulAgent {
	/// Create a new task from the incoming message.
	///
	/// If the message contains the word "confirm", the task enters
	/// the input-required state to demonstrate multi-turn interaction.
	/// Otherwise, the task is immediately completed with an artifact
	/// containing the processed result.
	fn message_send(
		&self,
		_context: &RequestContext,
		params: SendMessageParams,
	) -> impl std::future::Future<Output = Result<SendMessageResult, A2AError>> + Send + '_ {
		async move {
			let task_id = self.store.next_task_id();
			let context_id = params
				.message
				.context_id
				.clone()
				.unwrap_or_else(|| format!("context-for-{task_id}"));

			// Check if the message requests confirmation.
			let needs_confirmation = params.message.parts.iter().any(|part| {
				if let PartContent::Text(text) = &part.content {
					text.to_lowercase().contains("confirm")
				} else {
					false
				}
			});

			let task = if needs_confirmation {
				// Pause and ask the caller for confirmation before proceeding.
				// The caller must send a follow-up message referencing this task.
				Task::new(
					&task_id,
					&context_id,
					TaskStatus::input_required(Message::text(
						format!("{task_id}-input-request"),
						Role::Agent,
						"Please confirm you want to proceed by sending 'yes'.",
					)),
				)
			} else {
				// Process immediately and return a completed task with an artifact.
				Task::new(&task_id, &context_id, TaskStatus::completed()).with_artifacts(vec![
					Artifact::new(
						format!("{task_id}-result"),
						vec![Part::text("Document processed successfully.")],
					)
					.with_name("processing result"),
				])
			};

			self.store.store(task.clone());
			Ok(SendMessageResult::Task(task))
		}
	}

	/// Retrieve a task by ID from the in-memory store.
	fn get_task(
		&self,
		_context: &RequestContext,
		params: GetTaskParams,
	) -> impl std::future::Future<Output = Result<Task, A2AError>> + Send + '_ {
		async move {
			self.store
				.get(&params.id)
				.ok_or_else(|| A2AError::task_not_found(&params.id))
		}
	}

	/// Cancel a task by moving it to the canceled state.
	///
	/// Only non-terminal tasks can be canceled. If the task has
	/// already completed, failed, or been canceled, returns a
	/// TaskNotCancelable error. The terminal check and the status
	/// update are performed under a single lock acquisition to
	/// avoid a TOCTOU race between the check and the write.
	fn cancel_task(
		&self,
		_context: &RequestContext,
		params: CancelTaskParams,
	) -> impl std::future::Future<Output = Result<Task, A2AError>> + Send + '_ {
		async move {
			let mut tasks = self.store.tasks.lock().unwrap();
			let task = tasks
				.get_mut(&params.id)
				.ok_or_else(|| A2AError::task_not_found(&params.id))?;

			if task.status.state.is_terminal() {
				return Err(A2AError::task_not_cancelable(&params.id));
			}

			task.status = TaskStatus::canceled();
			Ok(task.clone())
		}
	}
}

#[tokio::main]
async fn main() {
	// Get a TCP listener with a random port
	let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
		.await
		.expect("failed to bind to a port");
	let port = listener
		.local_addr()
		.expect("failed to get local address")
		.port();

	let card = AgentCard::new(AgentCardRequired {
		name: "Stateful Agent".into(),
		description: "Demonstrates task lifecycle management with create, retrieve, and cancel"
			.into(),
		supported_interfaces: vec![AgentInterface::new(
			format!("http://localhost:{port}"),
			"JSONRPC",
			"1.0",
		)],
		version: "1.0".into(),
		capabilities: AgentCapabilities::default(),
		skills: vec![
			AgentSkill::new(
				"document-processing",
				"Document Processing",
				"Processes documents and returns structured results",
				vec!["documents".into(), "processing".into()],
			)
			.with_examples(vec!["process this document".into()]),
		],
		default_input_modes: vec!["text/plain".into()],
		default_output_modes: vec!["text/plain".into()],
	});

	let agent = StatefulAgent {
		store: TaskStore::new(),
	};

	let router = a2a_router(agent, card);

	println!("Stateful agent listening on http://localhost:{port}");
	println!("Agent card at http://localhost:{port}/.well-known/agent.json");

	axum::serve(listener, router)
		.await
		.expect("server terminated unexpectedly");
}
