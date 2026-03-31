# a2a

Rust implementation of the [Agent-to-Agent (A2A) protocol](https://a2aproject.github.io/A2A/), the open standard for inter-agent communication under the [Linux Foundation](https://www.linuxfoundation.org/).

Spec-compliant. Runtime-agnostic. Feature-gated.

## Why this crate

The A2A protocol defines how autonomous agents discover each other and exchange work over JSON-RPC 2.0 with SSE streaming. Official SDKs exist for Python, TypeScript, and Java. This crate brings the protocol to Rust with a design shaped by the language's strengths:

- **Zero-cost layering** — core types depend only on `serde`. No async runtime, no HTTP framework, no TLS stack. Add layers as you need them through feature flags.
- **Compile-time protocol safety** — exhaustive pattern matching on task states, stream events, and content types. `#[non_exhaustive]` enums and structs with `#[serde(other)]` on `TaskState::Unknown` for forward-compatible deserialisation when the spec evolves.
- **Native async traits** — the `A2AHandler` trait uses `async fn` in traits (stable since Rust 1.75), not `#[async_trait]`. No hidden allocations in the handler path.
- **Full spec coverage** — all 11 JSON-RPC methods, agent card discovery, SSE streaming, push notification CRUD, JWS signature verification, and the complete security scheme model.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  axum feature                                       │
│  a2a_router() · /.well-known/agent.json · SSE       │
├─────────────────────┬───────────────────────────────┤
│  server feature     │  client feature               │
│  A2AHandler trait   │  A2AClient · discover_agent() │
│  dispatch()         │  SSE stream parsing            │
├─────────────────────┴───────────────────────────────┤
│  Core (no feature flags)                            │
│  AgentCard · Task · Message · Part · Artifact       │
│  StreamResponse · TaskState · Role · A2AError       │
│  JsonRpcRequest · JsonRpcResponse                   │
│  All operation param/result types                   │
│                                                     │
│  jws feature — JWS signature verification (ES256,   │
│  RS256) via RustCrypto, no native dependencies      │
│                                                     │
│  serde + serde_json + time · wasm32-compatible      │
└─────────────────────────────────────────────────────┘
```

## Quick start

Add the crate with the features you need:

```toml
# Core types only (for shared libraries, wasm, or custom transports)
[dependencies]
a2a = "0.1"

# Building an agent with axum
[dependencies]
a2a = { version = "0.1", features = ["axum"] }

# Calling remote agents
[dependencies]
a2a = { version = "0.1", features = ["client"] }
```

### Implementing an agent

```rust
use a2a::*;

struct EchoAgent;

impl A2AHandler for EchoAgent {
    async fn message_send(
        &self,
        _context: &RequestContext,
        params: SendMessageParams,
    ) -> Result<SendMessageResult, A2AError> {
        let reply = Message::new("reply-1", Role::Agent, params.message.parts)
            .with_context_id(
                params.message.context_id.unwrap_or_else(|| "default-ctx".into()),
            );
        Ok(SendMessageResult::Message(reply))
    }
}
```

### Serving with axum

```rust
use a2a::*;
use a2a::agent_card::AgentInterface;

let card = AgentCard::new(AgentCardRequired {
    name: "Echo Agent".into(),
    description: "Echoes messages back to the caller".into(),
    supported_interfaces: vec![
        AgentInterface::new("https://localhost:3000/", "JSONRPC", "1.0"),
    ],
    version: "1.0.0".into(),
    capabilities: AgentCapabilities::default(),
    skills: vec![],
    default_input_modes: vec!["text/plain".into()],
    default_output_modes: vec!["text/plain".into()],
});

let router = a2a_router(EchoAgent, card);
let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
axum::serve(listener, router).await.unwrap();
```

### Streaming with SSE

```rust
use a2a::*;

struct StreamAgent;

impl A2AHandler for StreamAgent {
    async fn message_stream(
        &self,
        _context: &RequestContext,
        _params: SendMessageParams,
    ) -> Result<EventStream<'_>, A2AError> {
        let events = vec![
            Ok(StreamResponse::StatusUpdate(
                TaskStatusUpdateEvent::new("task-1", "ctx-1", TaskStatus::working()),
            )),
            Ok(StreamResponse::StatusUpdate(
                TaskStatusUpdateEvent::new("task-1", "ctx-1", TaskStatus::completed()),
            )),
        ];
        Ok(Box::pin(futures_util::stream::iter(events)))
    }
}
```

### Calling a remote agent

```rust
use a2a::*;

let http = reqwest::Client::new();
let card = discover_agent(&http, "https://agent.example.com").await?;
let client = A2AClient::from_agent_card(http, &card)?;

// Send a message with full control over params
let params = SendMessageParams::new(
    Message::text("msg-1", Role::User, "Hello, agent!"),
);
let result = client.message_send(&params).await?;

// Task management
let task = client.get_task_by_id("task-1").await?;
let canceled = client.cancel_task_by_id("task-1").await?;
```

## Feature flags

| Flag     | Default | Dependencies added              | What it enables                                     |
|----------|---------|----------------------------------|-----------------------------------------------------|
| _(none)_ | —       | `serde`, `serde_json`, `time`    | All protocol types, JSON-RPC envelopes               |
| `server` | off     | `futures-core`                   | `A2AHandler` trait, `dispatch()`, `DispatchResult`   |
| `client` | off     | `reqwest`, `eventsource-stream`, `futures-*` | `A2AClient`, `discover_agent()`, SSE stream parsing |
| `axum`   | off     | `axum`, `tokio` (enables `server`) | `a2a_router()`, `/.well-known/agent.json` endpoint  |
| `jws`    | off     | `p256`, `rsa`, `sha2`, `signature` | JWS signature verification (ES256, RS256)           |

## Protocol coverage

All operations defined in the [A2A v1.0 specification](https://a2aproject.github.io/A2A/):

| Method                                          | Client | Server | Description                         |
|-------------------------------------------------|--------|--------|-------------------------------------|
| `SendMessage`                                   | yes    | yes    | Send a message, get a Task or reply |
| `SendStreamingMessage`                          | yes    | yes    | Send with SSE streaming response    |
| `GetTask`                                       | yes    | yes    | Retrieve a task by ID               |
| `ListTasks`                                     | yes    | yes    | List tasks with filtering           |
| `CancelTask`                                    | yes    | yes    | Cancel a running task               |
| `SubscribeToTask`                               | yes    | yes    | Reconnect to a task's SSE stream    |
| `CreateTaskPushNotificationConfig`              | yes    | yes    | Create/update webhook config        |
| `GetTaskPushNotificationConfig`                 | yes    | yes    | Retrieve webhook config             |
| `ListTaskPushNotificationConfigs`               | yes    | yes    | List all webhook configs for a task |
| `DeleteTaskPushNotificationConfig`              | yes    | yes    | Remove a webhook config             |
| `GetExtendedAgentCard`                          | yes    | yes    | Fetch authenticated agent card      |

Agent discovery (`GET /.well-known/agent.json`) is supported on both client (`discover_agent()`) and server (`a2a_router()`) sides.

## Examples

The `examples/` directory contains runnable agents demonstrating different patterns:

| Example | Command | What it shows |
|---------|---------|---------------|
| `echo_agent` | `cargo run --example echo_agent --features axum` | Minimal stateless agent, immediate Message response |
| `streaming_agent` | `cargo run --example streaming_agent --features axum` | SSE streaming with progress updates and artifact delivery |
| `stateful_agent` | `cargo run --example stateful_agent --features axum` | In-memory task store, tasks/get, tasks/cancel, input-required |

Each example listens on a different port (3000, 3001, 3002) so they can run simultaneously. Test with `curl` commands shown in the example file headers.

## Design decisions

**Typed timestamps.** Timestamps are represented as `time::OffsetDateTime` and normalised to UTC on construction to match proto3 `google.protobuf.Timestamp` semantics. Wire format is RFC 3339.

**`TaskState::Unknown` with `#[serde(other)]`.** Any state string not recognised by this version of the crate deserialises as `Unknown` instead of failing. This means a client built against one spec version can handle tasks from agents running a newer version. The trade-off: re-serialising `Unknown` produces the string `"unknown"`, not the original value.

**`#[non_exhaustive]` on public types.** `A2AError`, `VerificationError`, and key enums are marked `#[non_exhaustive]` so fields and variants can be added in future minor versions without breaking downstream code.

## Minimum supported Rust version

Edition 2024. Requires Rust 1.85+ for native `async fn` in trait support and edition 2024 features.

## Contributing

Contributions are welcome. The project tracks the [A2A specification](https://a2aproject.github.io/A2A/) and uses the [Python SDK](https://github.com/a2aproject/a2a-python) as the reference for JSON wire format.

To run the full test suite:

```sh
cargo test --all-features
```

## License

Copyright [Responsible Engineering Ab](https://responsible.engineering), governed by [Omnifi Foundation](https://omnifi.foundation).

[MPL-2.0](LICENSE.txt)
