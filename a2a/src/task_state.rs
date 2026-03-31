// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Task lifecycle states defined by the A2A specification.
//!
//! The A2A protocol models agent work as a task that moves through
//! a defined set of states. These states control how callers interpret
//! responses and when they should poll, stream, or stop waiting.
//!
//! The proto definition uses `UPPER_SNAKE` (SUBMITTED, WORKING, etc.)
//! but the JSON-RPC wire format uses kebab-case for multi-word states
//! (input-required, auth-required) and lowercase for single-word states.
//! Serialisation is handled by a custom Serialize/Deserialize impl
//! rather than serde rename attributes, because Unknown(String) must
//! round-trip the original unrecognised wire string unchanged—the
//! derive macros cannot express that without a custom impl.
//!
//! Terminal states (completed, failed, canceled, rejected) indicate
//! the task will not change again. Non-terminal states (submitted,
//! working, input-required, auth-required) indicate the task may still
//! progress and callers should continue observing.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The lifecycle state of an A2A task.
///
/// Agents set the task state as they progress through work. Callers
/// use the state to decide whether to keep streaming, poll, or
/// present input prompts to the operator.
///
/// The distinction between terminal and non-terminal states matters
/// for streaming—the server closes SSE connections when a task
/// reaches a terminal state (completed, failed, canceled, rejected).
///
/// Unknown(String) preserves the exact wire string for any state
/// value not recognised by this version of the crate. This supports
/// forward-compatible deserialisation when newer spec versions
/// introduce additional states—serialising Unknown back to the
/// wire produces the original string unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TaskState {
	/// The task has been accepted but work has not started yet.
	/// Agents return this immediately when they need to queue
	/// the request for later processing.
	Submitted,

	/// The agent is actively processing the task.
	/// Streaming connections will receive status and artifact
	/// updates while the task remains in this state.
	Working,

	/// The task finished successfully.
	/// Terminal state—artifacts are available and the task
	/// will not change again.
	Completed,

	/// The task encountered an unrecoverable error.
	/// Terminal state—the status message (a Message object)
	/// carries diagnostic information.
	Failed,

	/// The task was canceled by the caller.
	/// Terminal state—the agent acknowledged the cancellation.
	Canceled,

	/// The agent needs additional input from the caller to continue.
	/// The caller should send another message (via `SendMessage`)
	/// referencing this task's ID with the requested information.
	/// This is the A2A equivalent of a "human in the loop" pause.
	InputRequired,

	/// The agent rejected the task outright.
	/// Terminal state—the agent decided not to process this
	/// request, possibly because the input is outside its
	/// declared skills or capabilities.
	Rejected,

	/// The task requires authentication before it can proceed.
	/// The caller should authenticate using one of the agent's
	/// declared security schemes and retry.
	AuthRequired,

	/// A state value not recognised by this version of the crate.
	///
	/// Carries the original wire string so that round-trip
	/// serialisation is lossless—deserialising an unknown state
	/// and then re-serialising it produces the same bytes that
	/// arrived on the wire. This is critical for proxy and
	/// middleware components that must not corrupt state values
	/// from newer spec versions.
	Unknown(String),
}

impl Serialize for TaskState {
	/// Serialises the task state to its JSON-RPC wire string.
	///
	/// Known variants map to their spec-defined wire strings.
	/// Unknown(s) serialises to the stored string s unchanged,
	/// preserving the original value through any number of
	/// deserialise-then-serialise round trips.
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(match self {
			TaskState::Submitted => "submitted",
			TaskState::Working => "working",
			TaskState::Completed => "completed",
			TaskState::Failed => "failed",
			TaskState::Canceled => "canceled",
			TaskState::InputRequired => "input-required",
			TaskState::Rejected => "rejected",
			TaskState::AuthRequired => "auth-required",
			TaskState::Unknown(value) => value.as_str(),
		})
	}
}

impl<'de> Deserialize<'de> for TaskState {
	/// Deserialises a task state from its JSON-RPC wire string.
	///
	/// Recognised strings map to their typed variants. Any other
	/// string—including values introduced by future spec versions
	///—deserialises as `Unknown(original_string)` rather than
	/// failing. This keeps callers forward-compatible with agents
	/// running newer protocol versions.
	fn deserialize<De: Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
		let raw = String::deserialize(deserializer)?;
		let state = match raw.as_str() {
			"submitted" => TaskState::Submitted,
			"working" => TaskState::Working,
			"completed" => TaskState::Completed,
			"failed" => TaskState::Failed,
			"canceled" => TaskState::Canceled,
			"input-required" => TaskState::InputRequired,
			"rejected" => TaskState::Rejected,
			"auth-required" => TaskState::AuthRequired,
			_ => TaskState::Unknown(raw),
		};
		Ok(state)
	}
}

impl std::fmt::Display for TaskState {
	/// Formats the task state as its wire-format string.
	///
	/// Produces the exact string that appears on the JSON-RPC wire
	/// (e.g. "submitted", "working", "input-required"). This makes
	/// Display output consistent with what agents and callers see in
	/// protocol messages, which simplifies logging and diagnostics.
	/// Unknown(s) displays as the stored string s, matching the
	/// serialisation behaviour so log output and wire format agree.
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let label = match self {
			TaskState::Submitted => "submitted",
			TaskState::Working => "working",
			TaskState::Completed => "completed",
			TaskState::Failed => "failed",
			TaskState::Canceled => "canceled",
			TaskState::InputRequired => "input-required",
			TaskState::Rejected => "rejected",
			TaskState::AuthRequired => "auth-required",
			TaskState::Unknown(value) => value.as_str(),
		};
		formatter.write_str(label)
	}
}

impl TaskState {
	/// Whether this state is terminal—the task will not change again.
	///
	/// Streaming connections close when a terminal state is reached.
	/// Callers should stop polling after receiving a terminal state.
	/// Unknown(_) returns false as a safe default—a caller that
	/// doesn't recognise a state should keep observing rather than
	/// assuming the task is done.
	#[must_use]
	pub fn is_terminal(&self) -> bool {
		matches!(
			self,
			TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// The A2A JSON-RPC binding requires specific state names on the
	// wire. This test validates every known variant against the exact
	// strings from the specification to catch serialisation regressions.
	// Each state is checked for both serialisation and round-trip
	// fidelity because a mismatch in either direction means protocol
	// non-compliance. Unknown is excluded here—it has its own
	// round-trip test below.
	#[test]
	fn serialises_to_spec_wire_values() {
		let cases = vec![
			(TaskState::Submitted, "\"submitted\""),
			(TaskState::Working, "\"working\""),
			(TaskState::Completed, "\"completed\""),
			(TaskState::Failed, "\"failed\""),
			(TaskState::Canceled, "\"canceled\""),
			(TaskState::InputRequired, "\"input-required\""),
			(TaskState::Rejected, "\"rejected\""),
			(TaskState::AuthRequired, "\"auth-required\""),
		];

		for (state, expected) in cases {
			let json = serde_json::to_string(&state).unwrap();
			assert_eq!(json, expected, "wire format mismatch for {:?}", state);

			let round_tripped: TaskState = serde_json::from_str(&json).unwrap();
			assert_eq!(round_tripped, state, "round-trip failed for {}", expected);
		}
	}

	// The is_terminal() predicate controls streaming lifecycle and
	// caller polling behaviour. Getting this wrong means callers
	// either hang waiting for a task that's done, or stop watching
	// a task that's still in progress. Every variant is checked
	// explicitly rather than just spot-checking a couple.
	// Unknown(_) must return false—a safe default that keeps
	// callers observing rather than assuming completion.
	#[test]
	fn terminal_states_are_correct() {
		assert!(!TaskState::Submitted.is_terminal());
		assert!(!TaskState::Working.is_terminal());
		assert!(!TaskState::InputRequired.is_terminal());
		assert!(!TaskState::AuthRequired.is_terminal());
		assert!(!TaskState::Unknown("processing".into()).is_terminal());

		assert!(TaskState::Completed.is_terminal());
		assert!(TaskState::Failed.is_terminal());
		assert!(TaskState::Canceled.is_terminal());
		assert!(TaskState::Rejected.is_terminal());
	}

	// Unknown state strings from newer spec versions deserialise as
	// Unknown(original_string) rather than failing. This is critical
	// for forward compatibility—a caller built against spec v1 must
	// be able to handle tasks from a v2 agent that introduces new states.
	// The stored string must exactly match the wire value received.
	#[test]
	fn unknown_state_deserialises_unknown_values() {
		let result: TaskState = serde_json::from_str("\"processing\"").unwrap();
		assert_eq!(result, TaskState::Unknown("processing".into()));

		let result: TaskState = serde_json::from_str("\"paused\"").unwrap();
		assert_eq!(result, TaskState::Unknown("paused".into()));
	}

	// Unknown(String) must round-trip losslessly through serde.
	// Deserialising an unrecognised wire string and re-serialising
	// it must produce the original bytes unchanged. This is the core
	// requirement for proxy and middleware components that forward
	// task states between agents without interpreting them.
	#[test]
	fn unknown_state_round_trips_original_string() {
		let original = "\"processing\"";
		let state: TaskState = serde_json::from_str(original).unwrap();

		// The stored string must be exactly what came off the wire.
		assert_eq!(state, TaskState::Unknown("processing".into()));

		// Re-serialising must produce the original wire value.
		let re_serialised = serde_json::to_string(&state).unwrap();
		assert_eq!(
			re_serialised, original,
			"Unknown must round-trip to its original wire string"
		);
	}

	// Display must produce the exact wire-format strings so that log
	// output matches what appears in protocol messages. Each known
	// variant is checked explicitly. Unknown(s) must display the
	// stored string s—not "unknown"—so log output agrees with
	// both the wire format and re-serialisation.
	#[test]
	fn display_matches_wire_format_strings() {
		assert_eq!(TaskState::Submitted.to_string(), "submitted");
		assert_eq!(TaskState::Working.to_string(), "working");
		assert_eq!(TaskState::Completed.to_string(), "completed");
		assert_eq!(TaskState::Failed.to_string(), "failed");
		assert_eq!(TaskState::Canceled.to_string(), "canceled");
		assert_eq!(TaskState::InputRequired.to_string(), "input-required");
		assert_eq!(TaskState::Rejected.to_string(), "rejected");
		assert_eq!(TaskState::AuthRequired.to_string(), "auth-required");

		// Unknown(s) must display the stored string, not a generic
		// "unknown" placeholder—this ensures Display and serde agree.
		assert_eq!(
			TaskState::Unknown("processing".into()).to_string(),
			"processing"
		);
	}

	// Display output must match the serde serialisation for every
	// variant. This is a cross-check between the two separate
	// implementations—Display uses a manual match, serde uses a
	// custom Serialize impl. If they diverge, logging will show
	// different strings than the wire format.
	#[test]
	fn display_agrees_with_serde_serialisation() {
		let all_states = [
			TaskState::Submitted,
			TaskState::Working,
			TaskState::Completed,
			TaskState::Failed,
			TaskState::Canceled,
			TaskState::InputRequired,
			TaskState::Rejected,
			TaskState::AuthRequired,
			TaskState::Unknown("future-state".into()),
		];

		for state in &all_states {
			let display_string = state.to_string();
			let serde_string = serde_json::to_string(state).unwrap();
			// serde wraps in quotes: "submitted" vs submitted
			let serde_unquoted = serde_string.trim_matches('"');
			assert_eq!(
				display_string, serde_unquoted,
				"Display and serde disagree for {:?}",
				state
			);
		}
	}
}
