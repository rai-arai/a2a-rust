// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Atomic content units in the A2A protocol.
//!
//! Parts are the building blocks of messages and artifacts. The A2A v1.0
//! protocol defines four kinds of content—text, raw bytes, URL references,
//! and structured data—discriminated by field presence on the wire (proto3
//! oneof semantics). A single message can mix content types freely: an agent
//! might respond with a text explanation and a URL reference in the same
//! message.
//!
//! The wire format for a Part is a flat JSON object where exactly one of the
//! four content fields is present. Optional fields (metadata, filename,
//! mediaType) appear at the same level. There is no separate `"kind"` tag—
//! the content type is inferred from which key appears.
//!
//! ```json
//! {"text":"hello"}
//! {"url":"https://x.com/f.pdf","filename":"f.pdf","mediaType":"application/pdf"}
//! {"raw":"SGVsbG8=","mediaType":"text/plain"}
//! {"data":{"key":"val"}}
//! {"text":"hello","metadata":{"source":"llm"}}
//! ```
//!
//! Because field presence drives the discriminant, standard serde enum
//! representations cannot model this directly. A custom Serialize/Deserialize
//! implementation reads and writes the flat structure manually, enforcing that
//! exactly one content key is present and rejecting ambiguous inputs.

use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The content discriminant of a Part—exactly one variant is present on the wire.
///
/// The variant determines which JSON key carries the payload:
///
/// - `Text` → `"text"` key—plain or formatted text
/// - `Raw`  → `"raw"` key—base64-encoded inline bytes
/// - `Url`  → `"url"` key—external content reference
/// - `Data` → `"data"` key—any structured JSON value
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PartContent {
	/// Plain or formatted text content.
	Text(String),

	/// Inline binary content as decoded bytes.
	///
	/// On the wire these bytes are base64-encoded under the `"raw"` key.
	/// The in-memory representation is always the actual byte payload so that
	/// callers work with real data rather than an encoded form.
	Raw(Vec<u8>),

	/// A URL reference to external content.
	Url(String),

	/// Structured data as a JSON value.
	Data(serde_json::Value),
}

/// A single content unit within a message or artifact.
///
/// Parts carry the actual payload—text, URL references, inline bytes, or
/// structured data. The content variant is determined by which field is
/// present in the JSON (proto3 oneof semantics).
///
/// Optional fields (`metadata`, `filename`, `media_type`) apply to any
/// content variant and are omitted from the wire when absent. Use the
/// `with_*` chainable setters to attach them after construction.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
	/// The content payload—exactly one of text, raw, url, or data.
	pub content: PartContent,

	/// Arbitrary key-value metadata attached to this part.
	/// Constrained to a JSON object—matching proto3 google.protobuf.Struct
	/// which never serialises as an array or scalar.
	pub metadata: Option<serde_json::Map<String, serde_json::Value>>,

	/// Human-readable filename for this content.
	pub filename: Option<String>,

	/// MIME type of the content (e.g. `"application/pdf"`, `"text/plain"`).
	pub media_type: Option<String>,
}

impl Part {
	/// Create a text part with no metadata, filename, or media type.
	///
	/// Text is the most common part type—used for natural language requests,
	/// responses, and explanations. Chain [`with_metadata`][Part::with_metadata]
	/// to attach caller-defined context.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::text("Hello, agent!");
	/// assert_eq!(part.to_string(), "Hello, agent!");
	/// ```
	#[must_use]
	pub fn text(text: impl Into<String>) -> Self {
		Self {
			content: PartContent::Text(text.into()),
			metadata: None,
			filename: None,
			media_type: None,
		}
	}

	/// Create a URL reference part with no metadata, filename, or media type.
	///
	/// The URL scheme is unconstrained—it could be `https://`, `s3://`, or
	/// any scheme the agent and caller agree on. Chain
	/// [`with_filename`][Part::with_filename] and
	/// [`with_media_type`][Part::with_media_type] to add identification
	/// metadata so the receiving agent can present and process the referenced
	/// content correctly.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::url("https://example.com/report.pdf")
	///     .with_filename("report.pdf")
	///     .with_media_type("application/pdf");
	/// ```
	#[must_use]
	pub fn url(url: impl Into<String>) -> Self {
		Self {
			content: PartContent::Url(url.into()),
			metadata: None,
			filename: None,
			media_type: None,
		}
	}

	/// Create a raw inline bytes part from an actual byte slice.
	///
	/// Accepts any `impl AsRef<[u8]>`, so callers can pass `&[u8]`, `Vec<u8>`,
	/// or a byte literal like `b"Hello"`. The bytes are stored as-is and are
	/// base64-encoded only when the part is serialised to the wire.
	///
	/// Used for small payloads where URL indirection adds unnecessary latency
	/// or infrastructure requirements. Chain
	/// [`with_media_type`][Part::with_media_type] so the receiving agent can
	/// interpret the bytes correctly without relying on extension sniffing.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::raw(b"Hello")
	///     .with_media_type("text/plain");
	/// ```
	#[must_use]
	pub fn raw(bytes: impl AsRef<[u8]>) -> Self {
		Self {
			content: PartContent::Raw(bytes.as_ref().to_vec()),
			metadata: None,
			filename: None,
			media_type: None,
		}
	}

	/// Create a raw bytes part by decoding a base64-encoded string.
	///
	/// Intended for callers that already hold a base64 value—for example, when
	/// forwarding a payload received from a remote agent without re-encoding it.
	/// The string is decoded once at construction time; the stored bytes are the
	/// decoded payload, not the base64 form.
	///
	/// # Errors
	///
	/// Returns [`base64::DecodeError`] if `encoded` is not valid standard
	/// base64.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::raw_from_base64("SGVsbG8=").unwrap();
	/// // The part stores b"Hello" (5 bytes), not the 8-character base64 string.
	/// ```
	pub fn raw_from_base64(encoded: impl Into<String>) -> Result<Self, base64::DecodeError> {
		use base64::Engine;
		let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.into())?;
		Ok(Self::raw(bytes))
	}

	/// Create a structured data part from a JSON value.
	///
	/// The value can be any valid JSON—objects, arrays, or primitives. Used for
	/// machine-readable payloads such as API responses, configuration, and form
	/// data. Chain [`with_metadata`][Part::with_metadata] to attach
	/// caller-defined context.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::data(serde_json::json!({"temperature": 23.5}));
	/// ```
	#[must_use]
	pub fn data(data: serde_json::Value) -> Self {
		Self {
			content: PartContent::Data(data),
			metadata: None,
			filename: None,
			media_type: None,
		}
	}

	/// Attach caller-defined metadata to this part.
	///
	/// Metadata is passed through by the protocol without interpretation.
	/// Middleware and routing layers may use it for tracing, provenance,
	/// or content classification. The parameter type is
	/// `serde_json::Map<String, serde_json::Value>` so the type system
	/// enforces the proto3 google.protobuf.Struct constraint—arrays and
	/// scalars cannot be passed at compile time.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::text("hello")
	///     .with_metadata(
	///         serde_json::json!({"source": "llm"}).as_object().unwrap().clone()
	///     );
	/// ```
	#[must_use]
	pub fn with_metadata(mut self, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
		self.metadata = Some(metadata);
		self
	}

	/// Attach a human-readable filename to this part.
	///
	/// Works with any content variant. Helps receiving agents present or save
	/// the content with a meaningful label. Commonly paired with URL and raw
	/// parts.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::url("https://example.com/doc.pdf")
	///     .with_filename("doc.pdf");
	/// ```
	#[must_use]
	pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
		self.filename = Some(filename.into());
		self
	}

	/// Attach a MIME type to this part.
	///
	/// Works with any content variant. Helps receiving agents render or process
	/// the content correctly without relying on extension sniffing. Serialises
	/// as the camelCase key `"mediaType"` per the A2A v1.0 wire spec.
	///
	/// ```
	/// # use a2a::Part;
	/// let part = Part::raw(b"Hello")
	///     .with_media_type("text/plain");
	/// ```
	#[must_use]
	pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
		self.media_type = Some(media_type.into());
		self
	}
}

impl fmt::Display for Part {
	/// Formats a part as a human-readable summary for logging and diagnostics.
	///
	/// Text parts display their full text content. URL parts show the URL in
	/// brackets. Raw parts show the decoded byte count so the length reflects
	/// the actual payload size rather than the base64 expansion. Data parts
	/// show the JSON value inside brackets. Use serde for wire serialisation—
	/// this output is not parseable.
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self.content {
			PartContent::Text(text) => write!(formatter, "{text}"),
			PartContent::Url(url) => write!(formatter, "[url: {url}]"),
			PartContent::Raw(bytes) => write!(formatter, "[raw: {} bytes]", bytes.len()),
			PartContent::Data(data) => write!(formatter, "[data: {data}]"),
		}
	}
}

impl Serialize for Part {
	/// Writes a flat JSON object where exactly one content key is present
	/// alongside any populated optional fields.
	///
	/// The `media_type` field serialises as `"mediaType"` (camelCase) per the
	/// A2A v1.0 proto3 JSON mapping. All other field names are already camelCase
	/// or single-word. Optional fields are omitted entirely when None.
	fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
	where
		Ser: Serializer,
	{
		// Count entries up front so serde can allocate the map in one pass.
		let mut len = 1usize;
		if self.metadata.is_some() {
			len += 1;
		}
		if self.filename.is_some() {
			len += 1;
		}
		if self.media_type.is_some() {
			len += 1;
		}

		let mut map = serializer.serialize_map(Some(len))?;

		match &self.content {
			PartContent::Text(text) => map.serialize_entry("text", text)?,
			PartContent::Raw(bytes) => {
				// Bytes are stored decoded; encode to standard base64 for the
				// wire so the JSON value matches the A2A v1.0 proto3 spec.
				use base64::Engine;
				let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
				map.serialize_entry("raw", &encoded)?;
			}
			PartContent::Url(url) => map.serialize_entry("url", url)?,
			PartContent::Data(data) => map.serialize_entry("data", data)?,
		}

		if let Some(meta) = &self.metadata {
			map.serialize_entry("metadata", meta)?;
		}
		if let Some(filename) = &self.filename {
			map.serialize_entry("filename", filename)?;
		}
		if let Some(media_type) = &self.media_type {
			map.serialize_entry("mediaType", media_type)?;
		}

		map.end()
	}
}

impl<'de> Deserialize<'de> for Part {
	fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
	where
		De: Deserializer<'de>,
	{
		deserializer.deserialize_map(PartVisitor)
	}
}

/// Read a field value from a serde map, enforcing uniqueness.
///
/// The A2A protocol does not allow duplicate keys within a Part object.
/// If a field has already been seen (`already_seen` is true), the function
/// returns a `duplicate_field` error immediately. Otherwise it reads the
/// next value from the map access and returns it.
///
/// This helper is used by the `PartVisitor` to keep each match arm concise
/// while preserving the duplicate-field validation that the spec requires.
fn parse_unique<'de, Access, Value>(
	map: &mut Access,
	already_seen: bool,
	field: &'static str,
) -> Result<Value, Access::Error>
where
	Access: MapAccess<'de>,
	Value: Deserialize<'de>,
{
	if already_seen {
		return Err(de::Error::duplicate_field(field));
	}
	map.next_value()
}

/// Read a base64-encoded "raw" field and decode it to bytes, enforcing uniqueness.
///
/// The A2A v1.0 proto3 spec encodes raw byte content as a standard base64
/// string on the wire. This function reads the encoded string from the map,
/// decodes it immediately, and returns the raw bytes. Storing decoded bytes
/// means consumers never need to re-decode, and the byte count reported by
/// `Part::fmt` reflects the actual payload size rather than the base64 expansion.
fn decode_raw<'de, Access>(map: &mut Access, already_seen: bool) -> Result<Vec<u8>, Access::Error>
where
	Access: MapAccess<'de>,
{
	use base64::Engine;

	if already_seen {
		return Err(de::Error::duplicate_field("raw"));
	}
	let encoded: String = map.next_value()?;
	base64::engine::general_purpose::STANDARD
		.decode(&encoded)
		.map_err(de::Error::custom)
}

/// Read a metadata field as a JSON object, enforcing uniqueness and type.
///
/// The A2A spec maps Part metadata to a proto3 `google.protobuf.Struct`,
/// which is always a JSON object. Arrays and scalar values are rejected at
/// parse time so that structural errors surface immediately rather than
/// propagating silently through the system as the wrong type.
fn parse_metadata<'de, Access>(
	map: &mut Access,
	already_seen: bool,
) -> Result<serde_json::Map<String, serde_json::Value>, Access::Error>
where
	Access: MapAccess<'de>,
{
	if already_seen {
		return Err(de::Error::duplicate_field("metadata"));
	}
	let raw_meta: serde_json::Value = map.next_value()?;
	match raw_meta {
		serde_json::Value::Object(obj_map) => Ok(obj_map),
		other => Err(de::Error::custom(format!(
			"Part metadata must be a JSON object, got: {other}"
		))),
	}
}

/// Serde visitor that constructs a [`Part`] from a flat JSON object.
///
/// Reads all recognised fields from the map and then validates that exactly one
/// content key was present. Unknown keys are consumed and discarded for forward
/// compatibility with future spec additions.
struct PartVisitor;

impl<'de> Visitor<'de> for PartVisitor {
	type Value = Part;

	fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(
			"a JSON object with exactly one content key: \"text\", \"raw\", \"url\", or \"data\"",
		)
	}

	fn visit_map<MapAccessor>(self, mut map: MapAccessor) -> Result<Self::Value, MapAccessor::Error>
	where
		MapAccessor: MapAccess<'de>,
	{
		let mut text: Option<String> = None;
		let mut raw: Option<Vec<u8>> = None;
		let mut url: Option<String> = None;
		let mut data: Option<serde_json::Value> = None;
		let mut metadata: Option<serde_json::Map<String, serde_json::Value>> = None;
		let mut filename: Option<String> = None;
		let mut media_type: Option<String> = None;

		while let Some(key) = map.next_key::<String>()? {
			match key.as_str() {
				"text" => text = Some(parse_unique(&mut map, text.is_some(), "text")?),
				"raw" => raw = Some(decode_raw(&mut map, raw.is_some())?),
				"url" => url = Some(parse_unique(&mut map, url.is_some(), "url")?),
				"data" => data = Some(parse_unique(&mut map, data.is_some(), "data")?),
				"metadata" => metadata = Some(parse_metadata(&mut map, metadata.is_some())?),
				"filename" => {
					filename = Some(parse_unique(&mut map, filename.is_some(), "filename")?);
				}
				"mediaType" => {
					media_type = Some(parse_unique(&mut map, media_type.is_some(), "mediaType")?);
				}
				_ => {
					map.next_value::<de::IgnoredAny>()?;
				}
			}
		}

		// Exactly one content key is required. Counting before matching avoids
		// silently applying proto3 "last wins" semantics—ambiguous inputs are
		// rejected so interop bugs surface immediately rather than producing
		// different results depending on key order.
		let content_count = [text.is_some(), raw.is_some(), url.is_some(), data.is_some()]
			.iter()
			.filter(|&&present| present)
			.count();

		if content_count == 0 {
			return Err(de::Error::custom(
				"Part must contain exactly one content key: \"text\", \"raw\", \"url\", or \"data\"",
			));
		}

		if content_count > 1 {
			return Err(de::Error::custom(
				"Part must contain exactly one content key, but multiple were found; \
                 ambiguous inputs are rejected for safety",
			));
		}

		let content = if let Some(t) = text {
			PartContent::Text(t)
		} else if let Some(r) = raw {
			PartContent::Raw(r)
		} else if let Some(u) = url {
			PartContent::Url(u)
		} else if let Some(d) = data {
			PartContent::Data(d)
		} else {
			unreachable!("content_count was 1 but no content variant matched");
		};

		Ok(Part {
			content,
			metadata,
			filename,
			media_type,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Verifies that a plain text part serialises to a flat JSON object with
	/// only the `"text"` key present. The v1.0 wire format has no `"kind"`
	/// discriminator—content type is inferred from field presence. Absent
	/// optional fields must not appear in the output.
	#[test]
	fn text_part_wire_format() {
		let part = Part::text("hello");
		let json = serde_json::to_value(&part).unwrap();

		assert_eq!(json["text"], "hello");

		let obj = json.as_object().unwrap();
		assert!(
			!obj.contains_key("kind"),
			"v1.0 wire format has no kind discriminator"
		);
		assert!(!obj.contains_key("metadata"));
		assert!(!obj.contains_key("filename"));
		assert!(!obj.contains_key("mediaType"));
	}

	/// Verifies that metadata appears at the top level alongside the content
	/// field, not nested inside it. The key `"metadata"` must be present with
	/// the exact value provided, and other optional fields remain absent.
	#[test]
	fn text_part_with_metadata() {
		let part = Part::text("hello").with_metadata(
			serde_json::json!({"source": "llm"})
				.as_object()
				.unwrap()
				.clone(),
		);
		let json = serde_json::to_value(&part).unwrap();

		assert_eq!(json["text"], "hello");
		assert_eq!(json["metadata"]["source"], "llm");

		let obj = json.as_object().unwrap();
		assert!(!obj.contains_key("filename"));
		assert!(!obj.contains_key("mediaType"));
	}

	/// Verifies that a URL part serialises to a flat JSON object containing the
	/// `"url"` key plus `"filename"` and `"mediaType"` when set. The camelCase
	/// `"mediaType"` key is required by the A2A v1.0 proto3 JSON mapping, and
	/// no stray content keys from other variants must appear.
	#[test]
	fn url_part_wire_format() {
		let part = Part::url("https://x.com/f.pdf")
			.with_filename("f.pdf")
			.with_media_type("application/pdf");
		let json = serde_json::to_value(&part).unwrap();

		assert_eq!(json["url"], "https://x.com/f.pdf");
		assert_eq!(json["filename"], "f.pdf");
		assert_eq!(json["mediaType"], "application/pdf");

		let obj = json.as_object().unwrap();
		assert!(!obj.contains_key("text"));
		assert!(!obj.contains_key("raw"));
		assert!(!obj.contains_key("data"));
	}

	/// Verifies that a URL part constructed with no optional fields omits all
	/// three optional keys entirely, keeping the wire payload compact.
	#[test]
	fn url_part_minimal() {
		let part = Part::url("https://example.com/doc");
		let json = serde_json::to_value(&part).unwrap();

		assert_eq!(json["url"], "https://example.com/doc");

		let obj = json.as_object().unwrap();
		assert!(!obj.contains_key("filename"));
		assert!(!obj.contains_key("mediaType"));
		assert!(!obj.contains_key("metadata"));
	}

	/// Verifies that a raw bytes part serialises to `{"raw":"<base64>",
	/// "mediaType":"..."}`. Bytes are stored decoded in memory and must be
	/// re-encoded to standard base64 when written to the wire. The mediaType
	/// guides the receiving agent when interpreting the payload.
	///
	/// Construction via `Part::raw(b"Hello")` stores five actual bytes.
	/// Serialisation must produce the canonical base64 encoding of those bytes
	/// (`"SGVsbG8="`) regardless of how the part was originally constructed.
	#[test]
	fn raw_part_wire_format() {
		// b"Hello" encodes to "SGVsbG8=" in standard base64.
		let part = Part::raw(b"Hello").with_media_type("text/plain");
		let json = serde_json::to_value(&part).unwrap();

		assert_eq!(json["raw"], "SGVsbG8=");
		assert_eq!(json["mediaType"], "text/plain");

		let obj = json.as_object().unwrap();
		assert!(!obj.contains_key("text"));
		assert!(!obj.contains_key("url"));
		assert!(!obj.contains_key("data"));
	}

	/// Verifies that a data part serialises to `{"data":{...}}` where the
	/// inner value is the full JSON payload. The `"data"` key wraps any valid
	/// JSON value—objects, arrays, or primitives.
	#[test]
	fn data_part_wire_format() {
		let part = Part::data(serde_json::json!({"key": "val"}));
		let json = serde_json::to_value(&part).unwrap();

		assert_eq!(json["data"]["key"], "val");

		let obj = json.as_object().unwrap();
		assert!(!obj.contains_key("text"));
		assert!(!obj.contains_key("raw"));
		assert!(!obj.contains_key("url"));
	}

	/// Serialises all four content variants to JSON and deserialises them back,
	/// asserting structural equality throughout. This verifies that the custom
	/// serde implementation is a lossless codec—the fundamental correctness
	/// property for any wire type that must interoperate with external agents.
	///
	/// For raw parts the round-trip must preserve the exact decoded bytes:
	/// the Part is serialised to base64 on the wire, then deserialised back to
	/// bytes. Both sides must compare equal to the original in-memory Part.
	#[test]
	fn all_variants_round_trip() {
		let parts = vec![
			Part::text("round-trip text"),
			Part::url("https://example.com/f.pdf")
				.with_filename("f.pdf")
				.with_media_type("application/pdf"),
			// b"\x01\x02\x03" → base64 "AQID" on the wire, then decoded back.
			Part::raw(b"\x01\x02\x03".as_ref()).with_media_type("application/octet-stream"),
			Part::data(serde_json::json!({"answer": 42})),
		];

		for part in parts {
			let json = serde_json::to_string(&part).unwrap();
			let back: Part = serde_json::from_str(&json).unwrap();
			assert_eq!(back, part, "round-trip failed for: {json}");
		}
	}

	/// Confirms that all three optional fields (metadata, filename, mediaType)
	/// are absent from the serialised JSON when they are None. Wire payloads
	/// for the common case must be compact—null-valued keys are not permitted.
	#[test]
	fn optional_fields_omitted_when_none() {
		let parts = [
			Part::text("no extras"),
			Part::url("https://example.com"),
			Part::raw(b"\x01\x02\x03".as_ref()),
			Part::data(serde_json::json!(null)),
		];

		for part in &parts {
			let json = serde_json::to_value(part).unwrap();
			let obj = json.as_object().unwrap();
			assert!(
				!obj.contains_key("metadata"),
				"metadata must be absent when None"
			);
			assert!(
				!obj.contains_key("filename"),
				"filename must be absent when None"
			);
			assert!(
				!obj.contains_key("mediaType"),
				"mediaType must be absent when None"
			);
		}
	}

	/// Deserialises JSON produced by an external implementation (for example,
	/// the Python A2A SDK) for all four content variants. Validates that the
	/// inbound parsing path handles the exact wire format the spec defines,
	/// including the camelCase `"mediaType"` key and all optional fields.
	#[test]
	fn deserialises_from_external_wire_json() {
		let text_json = r#"{"text":"what is the weather?"}"#;
		let text_part: Part = serde_json::from_str(text_json).unwrap();
		assert!(matches!(&text_part.content, PartContent::Text(t) if t == "what is the weather?"));

		let url_json =
			r#"{"url":"https://x.com/f.pdf","filename":"f.pdf","mediaType":"application/pdf"}"#;
		let url_part: Part = serde_json::from_str(url_json).unwrap();
		assert!(matches!(&url_part.content, PartContent::Url(u) if u == "https://x.com/f.pdf"));
		assert_eq!(url_part.filename.as_deref(), Some("f.pdf"));
		assert_eq!(url_part.media_type.as_deref(), Some("application/pdf"));

		let raw_json = r#"{"raw":"SGVsbG8=","mediaType":"text/plain"}"#;
		let raw_part: Part = serde_json::from_str(raw_json).unwrap();
		// "SGVsbG8=" decodes to b"Hello" (5 bytes); the in-memory variant
		// stores the decoded bytes, not the base64 string.
		assert!(matches!(&raw_part.content, PartContent::Raw(r) if r.as_slice() == b"Hello"));
		assert_eq!(raw_part.media_type.as_deref(), Some("text/plain"));

		let data_json = r#"{"data":{"key":"val"}}"#;
		let data_part: Part = serde_json::from_str(data_json).unwrap();
		assert!(matches!(&data_part.content, PartContent::Data(d) if d["key"] == "val"));
	}

	/// A JSON object with no recognised content key must be rejected at parse
	/// time. An empty Part is meaningless on the wire—failing loudly here
	/// prevents silently accepting corrupt messages from external agents.
	#[test]
	fn rejects_empty_part() {
		let json = r#"{"metadata":{"note":"no content key"}}"#;
		let result = serde_json::from_str::<Part>(json);
		assert!(
			result.is_err(),
			"deserialisation should fail with no content key"
		);
	}

	/// A JSON object with both `"text"` and `"url"` keys must be rejected.
	/// Proto3 "last wins" semantics are deliberately not followed here—ambiguous
	/// inputs produce an error rather than a silently different result depending
	/// on key order, which would be a source of interop bugs across
	/// implementations.
	#[test]
	fn rejects_multiple_content_keys() {
		let json = r#"{"text":"hello","url":"https://example.com"}"#;
		let result = serde_json::from_str::<Part>(json);
		assert!(
			result.is_err(),
			"deserialisation should fail when multiple content keys are present"
		);
	}

	/// Verifies that `Part::text()` produces a JSON object with exactly the
	/// `"text"` key and the provided string value. The constructor is the
	/// primary path for building text parts, and its wire output must match
	/// the spec exactly.
	#[test]
	fn text_constructor_matches_expected_wire() {
		let part = Part::text("hello world");
		let json = serde_json::to_value(&part).unwrap();
		assert_eq!(json, serde_json::json!({"text": "hello world"}));
	}

	/// Verifies that `Part::url()` chained with `with_filename()` and
	/// `with_media_type()` produces the correct flat JSON object with all three
	/// fields present. The builder setters must not interfere with each other
	/// and must serialise `media_type` as the camelCase key `"mediaType"`.
	#[test]
	fn url_constructor_with_filename_and_media_type() {
		let part = Part::url("https://example.com/doc.pdf")
			.with_filename("doc.pdf")
			.with_media_type("application/pdf");

		let json = serde_json::to_value(&part).unwrap();
		assert_eq!(
			json,
			serde_json::json!({
				"url": "https://example.com/doc.pdf",
				"filename": "doc.pdf",
				"mediaType": "application/pdf"
			})
		);
	}

	/// Verifies that `Part::raw()` chained with `with_media_type()` produces
	/// the correct flat JSON object with the camelCase `"mediaType"` key. The
	/// bytes are stored decoded and re-encoded to base64 on serialisation—the
	/// wire JSON must carry the canonical base64 form regardless of how the
	/// part was constructed. The raw content key and the media type must
	/// coexist at the top level with no other keys present.
	#[test]
	fn raw_constructor_with_media_type() {
		// b"Hello" (5 bytes) → base64 "SGVsbG8=" on the wire.
		let part = Part::raw(b"Hello").with_media_type("text/plain");

		let json = serde_json::to_value(&part).unwrap();
		assert_eq!(
			json,
			serde_json::json!({"raw": "SGVsbG8=", "mediaType": "text/plain"})
		);
	}

	/// Display for text parts must emit the full text content so it is directly
	/// readable in log output without any wrapping brackets.
	#[test]
	fn display_text_shows_content() {
		let part = Part::text("hello world");
		assert_eq!(part.to_string(), "hello world");
	}

	/// Display for URL parts must wrap the URL in `[url: ...]` brackets so it
	/// is visually distinct from inline text in log output.
	#[test]
	fn display_url_shows_url() {
		let part = Part::url("https://example.com/report.pdf");
		assert_eq!(part.to_string(), "[url: https://example.com/report.pdf]");
	}

	/// Display for raw parts must show the decoded byte count—the actual payload
	/// size—rather than the length of the base64 string, which is always larger
	/// due to base64 expansion. Reporting the base64 length was flagged as
	/// misleading because it overstates the true data size by roughly one third.
	///
	/// `b"Hello"` is 5 bytes. Its base64 encoding `"SGVsbG8="` is 8 characters.
	/// The display must read `[raw: 5 bytes]`, not `[raw: 8 bytes]`.
	#[test]
	fn display_raw_shows_byte_length() {
		let part = Part::raw(b"Hello");
		assert_eq!(part.to_string(), "[raw: 5 bytes]");
	}

	/// Verifies that `Part::raw_from_base64` decodes a valid standard base64
	/// string and stores the decoded bytes in the `PartContent::Raw` variant.
	///
	/// This constructor is the intended path for callers that already hold a
	/// base64 value received from an external agent—it decodes once at
	/// construction time so all downstream code works with actual bytes.
	/// The stored bytes must exactly match the expected decoded payload.
	#[test]
	fn raw_from_base64_decodes_and_stores_bytes() {
		let part = Part::raw_from_base64("SGVsbG8=").unwrap();
		assert!(
			matches!(&part.content, PartContent::Raw(b) if b.as_slice() == b"Hello"),
			"raw_from_base64 must store the decoded bytes, not the base64 string"
		);
	}

	/// Verifies that `Part::raw_from_base64` returns an error when the input
	/// is not valid base64. Structural errors must surface at construction time
	/// rather than propagating silently through the system.
	#[test]
	fn raw_from_base64_rejects_invalid_input() {
		let result = Part::raw_from_base64("not valid base64!!!");
		assert!(
			result.is_err(),
			"raw_from_base64 must return Err for invalid base64 input"
		);
	}

	/// Verifies the end-to-end round-trip when a caller starts with a base64
	/// string, constructs the part via `raw_from_base64`, serialises to JSON,
	/// and deserialises back. The final in-memory bytes must equal the original
	/// decoded payload, confirming that the encode→serialise→deserialise→decode
	/// path is lossless.
	#[test]
	fn raw_from_base64_round_trips_bytes() {
		// Start with a canonical base64 string as received from a remote agent.
		let original_encoded = "AQIDBA=="; // b"\x01\x02\x03\x04"
		let part = Part::raw_from_base64(original_encoded).unwrap();

		// Serialise to wire JSON—bytes must be re-encoded to base64.
		let wire = serde_json::to_string(&part).unwrap();

		// Deserialise back—base64 on the wire must decode to the same bytes.
		let restored: Part = serde_json::from_str(&wire).unwrap();

		assert_eq!(
			part, restored,
			"bytes must survive serialize→deserialize intact"
		);

		// Confirm the actual payload is the four bytes we started with.
		assert!(matches!(&restored.content, PartContent::Raw(b)
                if b.as_slice() == b"\x01\x02\x03\x04"));
	}

	/// Display for data parts must emit the JSON value inside `[data: ...]`
	/// brackets so structured content is visible in diagnostic output without
	/// being confused with plain text.
	#[test]
	fn display_data_shows_json() {
		let part = Part::data(serde_json::json!({"answer": 42}));
		let display = part.to_string();
		assert!(display.starts_with("[data: "));
		assert!(display.contains("42"));
	}
}
