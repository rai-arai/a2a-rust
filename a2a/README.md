# Arai A2A *Core*

Core types and protocol logic for the [Agent-to-Agent (A2A) protocol](https://a2a-protocol.org/).

This crate provides everything you need to work with A2A messages, tasks, agent cards, and JSON-RPC envelopes—without pulling in an async runtime, HTTP framework, or cryptographic library. It compiles to `wasm32-unknown-unknown` and is suitable for use in shared libraries, edge workers, and anywhere you need the protocol's type system without transport concerns.

## What's included

- **Agent cards**—`AgentCard`, capabilities, skills, interfaces, security schemes, and all OAuth flow types
- **Tasks and messages**—`Task`, `TaskStatus`, `TaskState`, `Message`, `Part`, `Artifact`, `Role`
- **Streaming events**—`StreamResponse`, `TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent`
- **JSON-RPC envelopes**—`JsonRpcRequest`, `JsonRpcResponse`
- **Operation types**—typed params and results for all 11 A2A methods
- **Error codes**—`A2AError` with all spec-defined error codes and factory methods

## Usage

```toml
[dependencies]
a2a = "0.0.0"
```

```rust
use a2a::{Task, TaskStatus, TaskState, Message, Role, Part};

let task = Task::new("task-1", "ctx-1", TaskStatus::new(TaskState::Submitted));
let message = Message::text("msg-1", Role::User, "Hello, agent!");
```

For server, client, transport, and JWS support, see the companion crates: [`a2a-server`](../server), [`a2a-client`](../client), [`a2a-axum`](../axum), [`a2a-jws`](../jws).

## License

Copyright [Responsible Engineering Ab](https://responsible.engineering), governed by [Omnifi Foundation](https://omnifi.foundation).

[MPL-2.0](../LICENSE.txt)
