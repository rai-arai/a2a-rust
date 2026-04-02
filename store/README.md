# Arai A2A *Store*

Task storage implementations for [A2A](https://a2a-protocol.org/) agents.

Every A2A server needs to persist Task records to back the protocol's task management operations. This crate provides concrete `TaskStore` implementations so you don't have to build one from scratch. The `MemoryTaskStore` is included by default for tests and prototyping. Database-backed implementations can be added behind feature flags.

## What's included

- **`MemoryTaskStore`**—in-memory `HashMap` behind a `tokio::sync::RwLock`. Fast, concurrent, suitable for tests and single-process deployments. All data is lost on process exit.

## Usage

```toml
[dependencies]
a2a = "0.0.0"
a2a-store = "0.0.0"
```

```rust
use a2a::{Task, TaskStatus, TaskState, TaskStore, ListTasksParams};
use a2a_store::MemoryTaskStore;

let store = MemoryTaskStore::new();

let task = Task::new("task-1", "ctx-1", TaskStatus::new(TaskState::Submitted));
store.save(&task).await?;

let loaded = store.load("task-1").await?;
let response = store.list(&ListTasksParams::new()).await?;
```

The `TaskStore` trait is defined in the core [`a2a`](../a2a) crate. Implement it directly for custom backends, or use the implementations provided here.

## License

Copyright [Responsible Engineering Ab](https://responsible.engineering), governed by [Omnifi Foundation](https://omnifi.foundation).

[MPL-2.0](../LICENSE.txt)
