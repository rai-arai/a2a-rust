# a2a-rust

Rust implementation of the [A2A (Agent-to-Agent) protocol](https://a2a-protocol.org/latest/), the open standard for agent-to-agent communication developed under the Linux Foundation.

Runtime-agnostic, spec-compliant, feature-gated.

## Features

- **Core types** — protocol data types with zero runtime dependencies beyond `serde`
- **Client** — discover agents and send messages over HTTP, with SSE streaming support
- **Server** — typed handler trait for implementing A2A agents
- **Axum integration** — optional router and SSE support for axum-based servers
- **Runtime freedom** — core compiles to `wasm32`, server works with any Tower-compatible runtime

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `client` | no | HTTP client for calling remote A2A agents |
| `server` | no | `A2AHandler` trait and JSON-RPC dispatch |
| `axum` | no | Axum router integration (enables `server`) |

## License

LGPL-3.0-or-later
