// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Task storage implementations for A2A agents.
//!
//! This crate provides concrete `TaskStore` implementations that agents
//! can use to persist task state. The `MemoryTaskStore` is included via
//! the `memory` feature (enabled by default). Database-backed
//! implementations (`SQLite`, Postgres) can be added behind their own
//! feature flags.

#[cfg(feature = "memory")]
pub mod memory;

#[cfg(feature = "memory")]
pub use memory::MemoryTaskStore;
