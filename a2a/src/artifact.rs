// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Task output artifacts in the A2A protocol.
//!
//! Artifacts are the outputs a task produces—distinct from conversation
//! messages. While messages represent the dialogue between requester and
//! responder, artifacts represent the work product. An agent processing
//! a "render this webpage" request might produce a text message saying
//! "here's the rendered page" alongside an artifact containing the
//! actual HTML content.
//!
//! A single task can produce multiple artifacts. For example, a document
//! processing agent might produce a summary artifact, an extracted-data
//! artifact, and a thumbnail-image artifact from a single PDF input.
//!
//! Artifacts are composed of Parts (the same text/file/data union used
//! in messages), giving them the same content flexibility. The artifactId
//! is assigned by the agent and must be unique within the task.

use serde::{Deserialize, Serialize};

use crate::part::Part;

/// A discrete output produced by an agent during task execution.
///
/// Artifacts appear in the task's artifacts list as work completes.
/// During streaming, each new or updated artifact generates a
/// `TaskArtifactUpdateEvent` on the SSE connection.
///
/// The name and description are optional human-readable labels.
/// The parts carry the actual content—at least one is expected,
/// though the protocol doesn't enforce this at the wire level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
	/// Unique identifier for this artifact within its task.
	/// Assigned by the agent. Clients reference this ID when
	/// fetching or displaying specific artifacts.
	pub artifact_id: String,

	/// Human-readable name for this artifact.
	/// Helps people and downstream agents understand what the
	/// artifact contains without inspecting its parts. For example,
	/// "Rendered HTML" or "Extracted contacts".
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,

	/// Longer description of the artifact's contents or purpose.
	/// Useful when the name alone isn't enough to distinguish
	/// artifacts from each other within a multi-artifact task.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub description: Option<String>,

	/// The content of this artifact, as one or more typed parts.
	/// Same part types as messages—text, file references, and
	/// structured data can all appear as artifact content.
	pub parts: Vec<Part>,

	/// Arbitrary key-value metadata attached to this artifact.
	/// Passed through by the protocol without interpretation.
	/// Constrained to a JSON object—matching proto3 google.protobuf.Struct
	/// which never serialises as an array or scalar.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub metadata: Option<serde_json::Map<String, serde_json::Value>>,

	/// Protocol extension URIs attached to this artifact.
	/// Omitted from the wire format when empty.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub extensions: Vec<String>,
}

impl Artifact {
	/// Create a new artifact with the given ID and content parts.
	///
	/// The artifact ID is assigned by the agent and must be unique
	/// within the task. Parts carry the actual content—text, file
	/// references, or structured data. Optional fields (name,
	/// description, metadata, extensions) are left as None. Use
	/// the `with_*` chainable setters to populate them.
	///
	/// ```
	/// # use a2a::{Artifact, Part};
	/// let artifact = Artifact::new("artifact-1", vec![
	///     Part::text("rendered content"),
	/// ]);
	/// ```
	#[must_use]
	pub fn new(artifact_id: impl Into<String>, parts: Vec<Part>) -> Self {
		Self {
			artifact_id: artifact_id.into(),
			name: None,
			description: None,
			parts,
			metadata: None,
			extensions: Vec::new(),
		}
	}

	/// Assign a human-readable name to this artifact.
	///
	/// Helps people and downstream agents understand what the artifact
	/// contains without inspecting its parts. For example, "Rendered
	/// HTML", "Extracted contacts", or "Analysis report". When multiple
	/// artifacts exist on a task, distinct names make them easy to
	/// distinguish in logs and caller interfaces.
	#[must_use]
	pub fn with_name(mut self, name: impl Into<String>) -> Self {
		self.name = Some(name.into());
		self
	}

	/// Add a longer description of the artifact's contents or purpose.
	///
	/// Useful when the name alone isn't enough to distinguish artifacts
	/// from each other within a multi-artifact task. For example, a
	/// name of "Report" might pair with a description of "Full PDF
	/// analysis report including compliance findings and risk scores".
	#[must_use]
	pub fn with_description(mut self, description: impl Into<String>) -> Self {
		self.description = Some(description.into());
		self
	}

	/// Attach arbitrary metadata to this artifact.
	///
	/// Metadata is passed through by the protocol without
	/// interpretation. Common uses include content hashes, generation
	/// timestamps, pipeline version identifiers, or domain-specific
	/// annotations that don't fit into the protocol's structured fields.
	/// The parameter type is `serde_json::Map<String, serde_json::Value>`
	/// so the type system enforces the proto3 google.protobuf.Struct
	/// constraint—arrays and scalars cannot be passed at compile time.
	///
	/// ```
	/// # use a2a::{Artifact, Part};
	/// let artifact = Artifact::new("a-1", vec![Part::text("content")])
	///     .with_metadata(
	///         serde_json::json!({"version": "1.0"}).as_object().unwrap().clone()
	///     );
	/// ```
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}

	/// Declare protocol extension URIs for this artifact.
	///
	/// Extensions signal that the artifact uses capabilities beyond
	/// the base protocol. Callers that don't recognise an extension
	/// URI can still process the artifact's standard fields but may
	/// miss extended semantics.
	#[must_use]
	pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
		self.extensions = extensions;
		self
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// A minimal artifact has just an ID and parts. The optional
	// fields (name, description, metadata) should be omitted from
	// the wire format to keep payloads compact. An empty extensions
	// vec must also be omitted.
	#[test]
	fn minimal_artifact_wire_format() {
		let artifact = Artifact {
			artifact_id: "art-1".into(),
			name: None,
			description: None,
			parts: vec![Part::text("<html>rendered page</html>")],
			metadata: None,
			extensions: vec![],
		};
		let json = serde_json::to_value(&artifact).unwrap();
		let obj = json.as_object().unwrap();

		assert_eq!(obj["artifactId"], "art-1");
		assert!(obj["parts"].is_array());
		assert!(!obj.contains_key("name"));
		assert!(!obj.contains_key("description"));
		assert!(!obj.contains_key("metadata"));
		assert!(!obj.contains_key("extensions"));
	}

	// A fully populated artifact exercises all fields and verifies
	// the camelCase wire names are correct. The round-trip must be
	// lossless—no field should be dropped or mangled.
	#[test]
	fn full_artifact_round_trips() {
		let artifact = Artifact {
			artifact_id: "art-42".into(),
			name: Some("analysis results".into()),
			description: Some("structured output from the analysis pipeline".into()),
			parts: vec![
				Part::data(serde_json::json!({"score": 0.87, "labels": ["safe", "compliant"]})),
				Part::url("https://storage.example.com/report.pdf")
					.with_filename("report.pdf")
					.with_media_type("application/pdf"),
			],
			metadata: Some(
				serde_json::json!({"pipeline_version": "2.1"})
					.as_object()
					.unwrap()
					.clone(),
			),
			extensions: vec!["ext:audit-trail".into()],
		};

		let json = serde_json::to_string(&artifact).unwrap();
		let back: Artifact = serde_json::from_str(&json).unwrap();
		assert_eq!(back, artifact);
	}

	// The artifactId field must serialise as "artifactId" (camelCase),
	// not "artifact_id" (snake_case). This is a common source of
	// interop failures because Rust's natural naming convention
	// conflicts with the spec's wire format.
	#[test]
	fn artifact_id_uses_camel_case_on_wire() {
		let artifact = Artifact {
			artifact_id: "test-identifier".into(),
			name: None,
			description: None,
			parts: vec![],
			metadata: None,
			extensions: vec![],
		};
		let json = serde_json::to_value(&artifact).unwrap();

		// Must be "artifactId", not "artifact_id".
		assert!(
			json.get("artifactId").is_some(),
			"artifactId must use camelCase on the wire"
		);
		assert!(
			json.get("artifact_id").is_none(),
			"snake_case artifact_id must not appear on the wire"
		);
	}

	// Artifact::new creates an artifact with just the ID and parts,
	// leaving optional scalar fields as None and extensions as an
	// empty vec. This is the common path for agents producing simple
	// single-part outputs.
	#[test]
	fn new_sets_id_and_parts_only() {
		let artifact = Artifact::new("artifact-new-1", vec![Part::text("output content")]);

		assert_eq!(artifact.artifact_id, "artifact-new-1");
		assert_eq!(artifact.parts.len(), 1);
		assert!(artifact.name.is_none());
		assert!(artifact.description.is_none());
		assert!(artifact.metadata.is_none());
		assert!(artifact.extensions.is_empty());
	}

	// Artifact chainable setters populate each optional field
	// independently. A fully configured artifact built through
	// the builder should produce identical wire output to struct
	// literal construction.
	#[test]
	fn setters_populate_optional_fields() {
		let artifact = Artifact::new(
			"artifact-configured-1",
			vec![Part::data(serde_json::json!({"score": 0.95}))],
		)
		.with_name("analysis results")
		.with_description("confidence scores from the ML pipeline")
		.with_metadata(
			serde_json::json!({"pipeline_version": "3.0"})
				.as_object()
				.unwrap()
				.clone(),
		)
		.with_extensions(vec!["ext:ml-scores".into()]);

		assert_eq!(artifact.name.as_deref(), Some("analysis results"));
		assert_eq!(
			artifact.description.as_deref(),
			Some("confidence scores from the ML pipeline")
		);
		assert_eq!(
			artifact.metadata.as_ref().unwrap()["pipeline_version"],
			"3.0"
		);
		assert_eq!(artifact.extensions, vec!["ext:ml-scores"]);

		// Round-trip to verify wire compatibility.
		let json = serde_json::to_string(&artifact).unwrap();
		let back: Artifact = serde_json::from_str(&json).unwrap();
		assert_eq!(back, artifact);
	}
}
