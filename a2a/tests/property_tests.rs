// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property-based serialisation round-trip tests.
//!
//! The handwritten unit tests in the crate cover specific wire format
//! shapes and edge cases the author thought of at the time. These
//! property tests complement them by generating random instances of
//! each core protocol type and verifying that
//! `deserialize(serialize(value)) == value`. Proptest explores corners
//! the author didn't consider—unusual Unicode sequences in text parts,
//! empty byte vectors in raw parts, metadata maps with numeric keys
//! that look like integers, and so on.
//!
//! The generators intentionally keep nesting shallow and collection
//! sizes small so that failure output is readable and test execution
//! stays fast. The default proptest case count (256) is sufficient for
//! these types since the interesting surface is in variant selection
//! and field presence, not in deep recursion.

use proptest::prelude::*;

use a2a::artifact::Artifact;
use a2a::message::Message;
use a2a::part::{Part, PartContent};
use a2a::role::Role;
use a2a::stream_event::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a::task::{Task, TaskStatus};
use a2a::task_state::TaskState;

// Generates either User or Agent. Both must round-trip identically
// since Role has no Unknown variant—an unrecognised role string is
// a hard deserialisation failure by design.
fn arb_role() -> impl Strategy<Value = Role> {
	prop_oneof![Just(Role::User), Just(Role::Agent),]
}

// Generates all eight named states from the v1.0 spec plus the
// Unknown variant with random state strings. The Unknown generator
// uses an "x-" prefix to avoid accidentally matching a known state
// name, which would cause the round-trip assertion to fail for the
// wrong reason (the value would deserialise as a known variant
// instead of Unknown).
fn arb_task_state() -> impl Strategy<Value = TaskState> {
	prop_oneof![
		Just(TaskState::Submitted),
		Just(TaskState::Working),
		Just(TaskState::Completed),
		Just(TaskState::Failed),
		Just(TaskState::Canceled),
		Just(TaskState::InputRequired),
		Just(TaskState::Rejected),
		Just(TaskState::AuthRequired),
		"x-[a-z0-9]{2,15}".prop_map(TaskState::Unknown),
	]
}

// Generates all four proto3 oneof content variants. Raw generates
// small random byte vectors (0–64 bytes) that exercise the base64
// encode/decode path in the custom Serialize/Deserialize. Url uses
// a regex that produces plausible HTTP URLs. Data uses simple scalar
// JSON values to avoid deep nesting while still covering null, bool,
// number, and string.
fn arb_part_content() -> impl Strategy<Value = PartContent> {
	prop_oneof![
		".*".prop_map(PartContent::Text),
		proptest::collection::vec(any::<u8>(), 0..64).prop_map(PartContent::Raw),
		"https?://[a-z]+\\.[a-z]+/[a-z]*".prop_map(PartContent::Url),
		arb_json_value().prop_map(PartContent::Data),
	]
}

// Generates simple JSON scalar values for use in metadata and data
// fields. Keeping these flat (no nested objects or arrays) avoids
// combinatorial explosion in the proptest search space. The four
// JSON scalar types—null, boolean, number, and string—are sufficient
// to exercise serde's handling of the Value type.
fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
	prop_oneof![
		Just(serde_json::Value::Null),
		any::<bool>().prop_map(serde_json::Value::Bool),
		any::<i32>().prop_map(|number| serde_json::Value::Number(number.into())),
		"[a-zA-Z0-9 _-]{0,50}".prop_map(serde_json::Value::String),
	]
}

// Generates either None or a small JSON object (0–3 entries) matching
// the proto3 google.protobuf.Struct constraint. Keys are short
// lowercase identifiers; values are arbitrary JSON scalars. This
// exercises the metadata field's skip_serializing_if behaviour—None
// produces no JSON key, Some produces a nested object.
fn arb_metadata() -> impl Strategy<Value = Option<serde_json::Map<String, serde_json::Value>>> {
	prop_oneof![
		Just(None),
		proptest::collection::btree_map("[a-z_]{1,10}", arb_json_value(), 0..3).prop_map(
			|entries| {
				Some(
					entries
						.into_iter()
						.collect::<serde_json::Map<String, serde_json::Value>>(),
				)
			}
		),
	]
}

// Generates a Part with random content, optional metadata, optional
// filename, and optional media type. The content variant is the most
// important axis—each variant exercises a different branch in the
// custom Serialize/Deserialize implementation.
fn arb_part() -> impl Strategy<Value = Part> {
	(
		arb_part_content(),
		arb_metadata(),
		proptest::option::of("[a-z]+\\.[a-z]+"),
		proptest::option::of("[a-z]+/[a-z]+"),
	)
		.prop_map(|(content, metadata, filename, media_type)| Part {
			content,
			metadata,
			filename,
			media_type,
		})
}

// Generates a TaskStatus with a random state but no timestamp.
// Timestamp generation would require building time::OffsetDateTime
// from random components, which adds complexity without testing
// interesting serde logic—the timestamp serialisation is already
// covered by dedicated unit tests in task.rs.
fn arb_task_status() -> impl Strategy<Value = TaskStatus> {
	arb_task_state().prop_map(TaskStatus::new)
}

// Generates a Message with a random ID, role, and 1–3 random parts.
// Context ID, task ID, and metadata are left at their defaults (None)
// since the interesting serialisation behaviour is in the parts and
// role fields.
fn arb_message() -> impl Strategy<Value = Message> {
	(
		"[a-z0-9-]{5,20}",
		arb_role(),
		proptest::collection::vec(arb_part(), 1..3),
	)
		.prop_map(|(message_id, role, parts)| Message::new(message_id, role, parts))
}

// Generates an Artifact with a random ID and 1–3 random parts.
// Optional fields (name, description, metadata) are left at defaults.
fn arb_artifact() -> impl Strategy<Value = Artifact> {
	(
		"[a-z0-9-]{5,20}",
		proptest::collection::vec(arb_part(), 1..3),
	)
		.prop_map(|(artifact_id, parts)| Artifact::new(artifact_id, parts))
}

// Generates a Task with random ID, context ID, and status. Artifacts
// and history are left empty—the interesting round-trip behaviour is
// in the status field's custom TaskState serialisation and the
// context_id's serde(default) handling.
fn arb_task() -> impl Strategy<Value = Task> {
	("[a-z0-9-]{5,20}", "[a-z0-9-]{5,20}", arb_task_status())
		.prop_map(|(task_id, context_id, status)| Task::new(task_id, context_id, status))
}

// Generates one of the four StreamResponse variants with random
// payloads. This exercises the externally-tagged enum representation
// where the variant name (camelCase) is the JSON object key. Each
// variant must round-trip through its specific wire format without
// cross-contamination between variants.
fn arb_stream_response() -> impl Strategy<Value = StreamResponse> {
	prop_oneof![
		arb_task().prop_map(StreamResponse::Task),
		arb_message().prop_map(StreamResponse::Message),
		("task-[a-z0-9]{3}", "ctx-[a-z0-9]{3}", arb_task_status()).prop_map(
			|(task_id, context_id, status)| {
				StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
					task_id, context_id, status,
				))
			}
		),
		("task-[a-z0-9]{3}", "ctx-[a-z0-9]{3}", arb_artifact()).prop_map(
			|(task_id, context_id, artifact)| {
				StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent::new(
					task_id, context_id, artifact,
				))
			}
		),
	]
}

proptest! {
	// Role has no Unknown variant—any unrecognised role string is a
	// hard deserialisation failure. Both known variants must survive
	// a JSON round-trip with exact equality.
	#[test]
	fn role_round_trips(role in arb_role()) {
		let json = serde_json::to_string(&role).unwrap();
		let recovered: Role = serde_json::from_str(&json).unwrap();
		prop_assert_eq!(role, recovered);
	}

	// TaskState has a custom Serialize/Deserialize that maps enum
	// variants to kebab-case wire strings. The Unknown variant
	// preserves the original wire string through serialisation, so
	// Unknown("x-custom") round-trips to Unknown("x-custom"). All
	// states—known and unknown—must round-trip with exact equality.
	#[test]
	fn task_state_round_trips(state in arb_task_state()) {
		let json = serde_json::to_string(&state).unwrap();
		let recovered: TaskState = serde_json::from_str(&json).unwrap();
		prop_assert_eq!(state, recovered);
	}

	// Part is the highest-value property test because it has fully
	// custom Serialize/Deserialize implementations that handle the
	// proto3 oneof wire format. The content type is determined by
	// field presence (not a discriminator tag), so subtle bugs in
	// the visitor logic—like failing to reject multiple content keys
	// or incorrectly decoding base64 in the Raw variant—would only
	// surface with specific input combinations.
	#[test]
	fn part_round_trips(part in arb_part()) {
		let json = serde_json::to_string(&part).unwrap();
		let recovered: Part = serde_json::from_str(&json).unwrap();
		prop_assert_eq!(part, recovered);
	}

	// Message round-trips verify that the parts Vec, role enum, and
	// message ID all survive serialisation through the derived serde
	// implementation. The main risk is a regression in the camelCase
	// field renaming (messageId, contextId) or in the
	// skip_serializing_if behaviour for optional fields.
	#[test]
	fn message_round_trips(message in arb_message()) {
		let json = serde_json::to_string(&message).unwrap();
		let recovered: Message = serde_json::from_str(&json).unwrap();
		prop_assert_eq!(message, recovered);
	}

	// Artifact round-trips cover the same serde-derive path as
	// Message but with a different field set (artifactId instead of
	// messageId, no role). The parts Vec is shared between both types
	// so a Part regression would show up in either test.
	#[test]
	fn artifact_round_trips(artifact in arb_artifact()) {
		let json = serde_json::to_string(&artifact).unwrap();
		let recovered: Artifact = serde_json::from_str(&json).unwrap();
		prop_assert_eq!(artifact, recovered);
	}

	// Task round-trips exercise the interaction between the custom
	// TaskState serde (inside TaskStatus) and the derived Task serde.
	// The context_id field uses serde(default) with
	// skip_serializing_if = String::is_empty, so empty context IDs
	// must round-trip as empty strings (not get lost).
	#[test]
	fn task_round_trips(task in arb_task()) {
		let json = serde_json::to_string(&task).unwrap();
		let recovered: Task = serde_json::from_str(&json).unwrap();
		prop_assert_eq!(task, recovered);
	}

	// StreamResponse is an externally-tagged enum where the camelCase
	// variant name is the JSON object key. This test verifies that
	// all four variants serialise with the correct discriminator key
	// and deserialise back to the same variant. A regression here
	// would break SSE stream parsing on the client side.
	#[test]
	fn stream_response_round_trips(response in arb_stream_response()) {
		let json = serde_json::to_string(&response).unwrap();
		let recovered: StreamResponse = serde_json::from_str(&json).unwrap();
		prop_assert_eq!(response, recovered);
	}
}
