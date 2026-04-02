// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A2A protocol implementation for Rust.
//!
//! This crate provides the protocol foundation—types, logic, JSON-RPC
//! envelopes, error codes, serialisation helpers, and all operation
//! param/result types needed to build A2A agents and clients. No async
//! runtime, no HTTP framework, no crypto. Depends only on serde,
//! `serde_json`, time, and base64.
//!
//! Transport adapters, handler traits, and cryptographic verification
//! live in companion crates: `a2a-server`, `a2a-client`, `a2a-axum`,
//! `a2a-jws`.

pub mod agent_card;
pub mod artifact;
pub mod error;
pub mod jsonrpc;
pub mod message;
pub mod operation;
pub mod part;
pub mod role;
pub(crate) mod serde_helpers;
pub mod stream_event;
pub mod task;
pub mod task_state;

pub use agent_card::{
	AgentCapabilities, AgentCard, AgentCardRequired, AgentCardSignature, AgentExtension,
	AgentInterface, AgentProvider, AgentSkill, AuthorizationCodeOAuthFlow,
	ClientCredentialsOAuthFlow, DeviceCodeOAuthFlow, ImplicitOAuthFlow, OAuthFlows,
	PasswordOAuthFlow, SecurityRequirement, SecurityScheme, StringList,
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
