# Arai A2A *Axum*

[Axum](https://github.com/tokio-rs/axum) transport adapter for serving [A2A](https://a2a-protocol.org/) agents over HTTP.

This crate provides `a2a_router()`—a single function that builds an axum `Router` with a JSON-RPC endpoint and agent card discovery. It handles SSE streaming, request context extraction (Bearer tokens, API keys, correlation IDs), and body size limits out of the box.

## What's included

- **`a2a_router()`**—builds a `Router` with:
  - `POST /`—JSON-RPC endpoint supporting both request-response and SSE streaming
  - `GET /.well-known/agent.json`—agent card discovery

## Usage

```toml
[dependencies]
a2a = "0.0.0"
a2a-server = "0.0.0"
a2a-axum = "0.0.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use a2a::{AgentCapabilities, AgentCard, AgentCardRequired, AgentInterface};
use a2a_axum::a2a_router;

let card = AgentCard::new(AgentCardRequired {
    name: "My Agent".into(),
    description: "Does useful things".into(),
    supported_interfaces: vec![
        AgentInterface::new("https://localhost:3000/", "JSONRPC", "1.0"),
    ],
    version: "1.0.0".into(),
    capabilities: AgentCapabilities::default(),
    skills: vec![],
    default_input_modes: vec!["text/plain".into()],
    default_output_modes: vec!["text/plain".into()],
});

let router = a2a_router(my_handler, card);
let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
axum::serve(listener, router).await.unwrap();
```

## Examples

The `examples/` directory contains three runnable agents:

| Example | Command | What it shows |
|---------|---------|---------------|
| `echo_agent` | `cargo run -p a2a-axum --example echo_agent` | Stateless echo, immediate Message response |
| `streaming_agent` | `cargo run -p a2a-axum --example streaming_agent` | SSE streaming with status updates and artifacts |
| `stateful_agent` | `cargo run -p a2a-axum --example stateful_agent` | In-memory task store, get/cancel/input-required |

For the handler trait, see [`a2a-server`](../server). For the core protocol types, see [`a2a`](../a2a).

## License

Copyright [Responsible Engineering Ab](https://responsible.engineering), governed by [Omnifi Foundation](https://omnifi.foundation).

[MPL-2.0](../LICENSE.txt)
