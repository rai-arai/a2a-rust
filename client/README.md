# Arai A2A *Client*

HTTP client for calling remote [A2A](https://a2a-protocol.org/) agents from Rust.

This crate wraps `reqwest` and provides typed methods for every A2A operation—message sending, task management, push notification CRUD, and agent card discovery. Streaming operations return a `Stream` of typed events parsed from SSE, so you can iterate over status updates and artifact deliveries as they arrive.

## What's included

- **`A2AClient`**—typed methods for all 11 JSON-RPC operations, plus convenience wrappers like `get_task_by_id()` and `cancel_task_by_id()`
- **`discover_agent()`**—fetches an agent's card from its well-known endpoint
- **`into_event_stream()`**—converts an HTTP response into a typed `Stream<Item = Result<StreamResponse, A2AError>>`

## Usage

```toml
[dependencies]
a2a = "0.0.0"
a2a-client = "0.0.0"
```

```rust
use a2a::{Message, Role, SendMessageParams};
use a2a_client::{A2AClient, discover_agent};

let http = reqwest::Client::new();
let card = discover_agent(&http, "https://agent.example.com").await?;
let client = A2AClient::from_agent_card(http, &card)?;

let params = SendMessageParams::new(
    Message::text("msg-1", Role::User, "Hello, agent!"),
);
let result = client.message_send(&params).await?;
```

For the core protocol types, see [`a2a`](../a2a). For building agents rather than calling them, see [`a2a-server`](../server).

## License

Copyright [Responsible Engineering Ab](https://responsible.engineering), governed by [Omnifi Foundation](https://omnifi.foundation).

[MPL-2.0](../LICENSE.txt)
