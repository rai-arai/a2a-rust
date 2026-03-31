// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Internal serde helpers for types that need custom serialisation logic.
//!
//! This module is not part of the public API—it is `pub(crate)` only.
//! Callers never interact with it directly; the helpers are wired up
//! via `#[serde(with = "...")]` attributes on individual struct fields.
//!
//! Currently provides one helper:
//!
//!   `serde_rfc3339_option` — serialises and deserialises
//!   `Option<time::OffsetDateTime>` as an RFC 3339 string on the wire,
//!   and as `None` / absent when the field is not set.
//!
//! The `time::serde::rfc3339` module handles bare `OffsetDateTime` but
//! has no built-in support for the `Option` wrapper. This module fills
//! that gap so the A2A timestamp fields can be typed as
//! `Option<OffsetDateTime>` without sacrificing wire compatibility.

/// Returns `true` when the referenced boolean is `false`.
///
/// Used with `#[serde(skip_serializing_if = "crate::serde_helpers::is_false")]`
/// to omit boolean fields from the wire format when they hold the proto3
/// default value. Callers treat an absent field as `false` per the proto3
/// JSON spec—this keeps payloads compact for the common case.
// The `&bool` signature is required by serde's `skip_serializing_if`, which
// always passes a reference to the field value. Changing to `bool` would break
// the serde attribute on every call site.
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_false(value: &bool) -> bool {
	!*value
}

/// Serde helper for `Option<time::OffsetDateTime>` in RFC 3339 format.
///
/// Wire representation:
///   - `Some(dt)` — serialises to a quoted RFC 3339 string, e.g.
///     `"2026-03-15T10:00:00Z"`.
///   - `None` — this serialiser is only called when
///     `skip_serializing_if = "Option::is_none"` is also present on the
///     field, so `None` values are omitted from the wire format entirely.
///     If the serialiser is called with `None` it emits a JSON `null`,
///     which is the serde convention for optional absent values.
///
/// Deserialisation:
///   - A quoted RFC 3339 string is parsed into `Some(OffsetDateTime)`.
///   - A JSON `null` or absent key (with `#[serde(default)]`) becomes `None`.
///   - Any string that does not parse as RFC 3339 is a hard error.
pub(crate) mod serde_rfc3339_option {
	use serde::{Deserialize, Deserializer, Serializer};
	use time::OffsetDateTime;
	use time::format_description::well_known::Rfc3339;

	/// Serialise an `Option<OffsetDateTime>` as an RFC 3339 string or null.
	///
	/// The `&Option<T>` signature is required by serde's `#[serde(with = "...")]`
	/// API, which generates calls of the form `serialize(&self.field, serializer)`.
	#[allow(clippy::ref_option)]
	pub fn serialize<S>(date: &Option<OffsetDateTime>, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		match date {
			Some(dt) => {
				let formatted = dt.format(&Rfc3339).map_err(serde::ser::Error::custom)?;
				serializer.serialize_str(&formatted)
			}
			None => serializer.serialize_none(),
		}
	}

	/// Deserialise an RFC 3339 string or null into `Option<OffsetDateTime>`.
	pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OffsetDateTime>, D::Error>
	where
		D: Deserializer<'de>,
	{
		let raw: Option<String> = Option::deserialize(deserializer)?;
		match raw {
			Some(raw_string) => {
				let dt = OffsetDateTime::parse(&raw_string, &Rfc3339)
					.map_err(serde::de::Error::custom)?;
				Ok(Some(dt))
			}
			None => Ok(None),
		}
	}
}
