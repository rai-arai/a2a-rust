# Arai A2A *Server*

Handler trait and JSON-RPC dispatch for building [A2A](https://a2a-protocol.org/) agents in Rust.

This crate provides `A2AHandler`—the trait you implement to build an agent—and the dispatch layer that routes incoming JSON-RPC requests to your handler methods. It is runtime-agnostic: the trait uses stable async fn in traits and returns `futures_core::Stream` for streaming operations, so it works with any async executor.

## What's included

- **`A2AHandler`**—one async method per protocol operation, each with a default that returns `UnsupportedOperation`. Implement only what your agent supports.
- **`dispatch()`**—routes a `JsonRpcRequest` to the right handler method, deserialises params, and wraps results into `JsonRpcResponse` envelopes.
- **`RequestContext`**—per-request metadata (auth tokens, trace IDs) passed from the transport layer to your handler. Credential types (`BearerToken`, `ApiKey`) are redacted in debug output.
- **`EventStream`**—the return type for streaming operations (`SendStreamingMessage`, `SubscribeToTask`).

## Usage

```toml
[dependencies]
a2a = "0.0.0"
a2a-server = "0.0.0"
```

```rust
use a2a::{A2AError, Message, Role, SendMessageParams, SendMessageResult};
use a2a_server::{A2AHandler, RequestContext};

struct MyAgent;

impl A2AHandler for MyAgent {
    fn message_send(
        &self,
        _context: &RequestContext,
        params: SendMessageParams,
    ) -> impl std::future::Future<Output = Result<SendMessageResult, A2AError>> + Send + '_ {
        async move {
            let reply = Message::text("reply-1", Role::Agent, "Got it!");
            Ok(SendMessageResult::Message(reply))
        }
    }
}
```

For a ready-made HTTP transport, see [`a2a-axum`](../axum). For the core protocol types, see [`a2a`](../a2a).

## License

Copyright [Responsible Engineering Ab](https://responsible.engineering), governed by [Omnifi Foundation](https://omnifi.foundation).

[MPL-2.0](../LICENSE.txt)
