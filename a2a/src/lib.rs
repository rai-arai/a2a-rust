// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A2A protocol implementation for Rust.
//!
//! This crate implements the Agent-to-Agent (A2A) protocol, the open
//! standard for agent-to-agent communication developed under the Linux
//! Foundation. The goal is spec-compliant, runtime-agnostic protocol
//! support that works everywhere from axum servers to wasm32 edge workers.
//!
//! The crate is structured around the protocol's own concepts:
//!
//!   `task_state` — lifecycle states (submitted, working, completed, etc.)
//!   `part`       — atomic content units (text, url, raw bytes, structured data)
//!   `message`    — conversation turns between requester and agent
//!   `artifact`   — task outputs composed of parts
//!   `task`       — the central coordination object
//!   `agent_card` — agent discovery and capability declaration
//!   `error`      — protocol error codes and types
//!   `jsonrpc`    — JSON-RPC 2.0 envelope types
//!   `operation`  — typed request/response structs for JSON-RPC methods
//!   `client`     — HTTP client for calling remote agents (feature-gated)
//!   `server`     — handler trait and JSON-RPC dispatch (feature-gated)
//!   `axum_integration` — axum router adapter (feature-gated, implies server)
//!
//! Feature flags gate runtime-specific functionality:
//!   `client` — HTTP client for calling remote A2A agents
//!   `server` — handler trait and JSON-RPC dispatch
//!   `axum`   — axum router integration (enables server)
//!
//! Core types (everything without a feature flag) depend only on serde,
//! `serde_json`, and time, making them usable in any environment including
//! `wasm32`.

pub mod agent_card;
pub mod artifact;
#[cfg(feature = "axum")]
pub mod axum_integration;
#[cfg(feature = "client")]
pub mod client;
pub mod error;
pub mod jsonrpc;
#[cfg(feature = "jws")]
pub mod jws;
pub mod message;
pub mod operation;
pub mod part;
pub mod role;
mod serde_helpers;
#[cfg(feature = "server")]
pub mod server;
pub mod stream_event;
pub mod task;
pub mod task_state;

pub use agent_card::{
	AgentCapabilities, AgentCard, AgentCardRequired, AgentCardSignature, AgentExtension,
	AgentInterface, AgentProvider, AgentSkill, AuthorizationCodeOAuthFlow,
	ClientCredentialsOAuthFlow, DeviceCodeOAuthFlow, OAuthFlows, SecurityRequirement,
	SecurityScheme, StringList,
};
pub use artifact::Artifact;
pub use error::A2AError;
pub use jsonrpc::{JsonRpcRequest, JsonRpcResponse};
pub use message::Message;
pub use operation::{
	AuthenticationInfo, CancelTaskParams, DeleteTaskPushNotificationConfigParams,
	GetExtendedAgentCardParams, GetTaskParams, GetTaskPushNotificationConfigParams,
	ListTaskPushNotificationConfigsParams, ListTaskPushNotificationConfigsResponse,
	ListTasksParams, ListTasksResponse, SendMessageConfiguration, SendMessageParams,
	SendMessageResult, SubscribeToTaskParams, TaskPushNotificationConfig,
};
pub use part::{Part, PartContent};
pub use role::Role;
pub use stream_event::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
pub use task::{Task, TaskStatus};
pub use task_state::TaskState;
pub use time::OffsetDateTime;

#[cfg(feature = "jws")]
pub use jws::{JwsAlgorithm, VerificationError, VerificationKey, parse_jwks, verify_detached_jws};

#[cfg(feature = "client")]
pub use client::{A2AClient, ClientEventStream, discover_agent};

#[cfg(feature = "axum")]
pub use axum_integration::a2a_router;

#[cfg(feature = "server")]
pub use server::{
	A2AHandler, ApiKey, BearerToken, CorrelationId, DispatchResult, EventStream, Extensions,
	RequestContext, dispatch,
};
