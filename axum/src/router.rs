// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Axum router construction and request handlers.
//!
//! The router exposes two endpoints:
//!
//!   POST / —the JSON-RPC endpoint. Accepts a `JsonRpcRequest` body,
//!             dispatches it through the handler, and returns either a
//!             JSON response or an SSE stream depending on the method.
//!
//!   GET /.well-known/agent.json—serves the agent's public card as
//!             static JSON. No authentication required per the spec.
//!
//! The POST handler inspects the dispatch result to decide the
//! response format. Request-response operations return a regular
//! JSON body. Streaming operations (`SendStreamingMessage`, `SubscribeToTask`)
//! return an SSE stream where each event carries a JSON-encoded
//! `StreamResponse`.
//!
//! The handler is stored in axum's State as an Arc<dyn SendHandler>,
//! where `SendHandler` is a trait alias for `A2AHandler` + Send + Sync.
//! This lets callers pass any handler implementation without the
//! router needing to be generic over the handler type.

use std::convert::Infallible;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};

use a2a::agent_card::AgentCard;
use a2a::error::A2AError;
use a2a::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use a2a_server::context::RequestContext;
use a2a_server::dispatch::{DispatchResult, dispatch};
use a2a_server::handler::A2AHandler;

/// Shared state for the axum router.
///
/// Holds the handler implementation and the public agent card.
/// Wrapped in Arc so it can be cheaply cloned across request tasks.
struct A2AState<H: A2AHandler> {
	handler: H,
	agent_card: AgentCard,
}

/// Build an axum Router that serves an A2A agent.
///
/// The router exposes:
///   - POST /—JSON-RPC endpoint
///   - GET /.well-known/agent.json—agent card discovery
///
/// The handler must implement `A2AHandler` (which requires Send + Sync).
/// The `AgentCard` is served as static JSON at the well-known path.
///
/// A default body size limit of 2 MiB is applied to the POST endpoint.
/// Callers that need a larger or smaller limit can override it by adding
/// their own `DefaultBodyLimit` layer after merging this router.
///
/// Callers using `#[tokio::main]` must enable the `macros` and
/// `rt-multi-thread` features on their own `tokio` dependency.
/// The `axum` feature provides only `rt` and `sync` to keep
/// the mandatory feature set minimal.
///
/// Callers can nest this router into a larger axum application:
///
///   let app = `Router::new()`
///       .`merge(a2a_router(my_handler`, `my_card`));
pub fn a2a_router<H: A2AHandler + 'static>(handler: H, agent_card: AgentCard) -> Router {
	let state = Arc::new(A2AState {
		handler,
		agent_card,
	});

	// The well-known path /.well-known/agent.json is used by the A2A
	// specification v1.0 for agent discovery. If the spec changes this
	// path in a future revision, this route and the path in
	// client/discovery.rs must both be updated to match.
	//
	// The 2 MiB body limit guards against runaway request bodies on the
	// JSON-RPC endpoint. Most A2A messages are text—2 MiB is generous
	// for interoperability while preventing trivial resource exhaustion.
	Router::new()
		.route("/", post(handle_rpc::<H>))
		.route("/.well-known/agent.json", get(handle_agent_card::<H>))
		.layer(DefaultBodyLimit::max(2 * 1024 * 1024))
		.with_state(state)
}

/// Handle a JSON-RPC POST request.
///
/// Dispatches the request through the handler and returns either
/// a JSON response or an SSE stream. The Content-Type header is
/// set automatically by axum's response types.
///
/// For streaming responses, the Arc<A2AState> is moved into the
/// stream so the handler outlives the SSE connection. The dispatch
/// borrow is scoped to the initial await; the stream itself only
/// holds owned `StreamEvent` values.
async fn handle_rpc<H: A2AHandler + 'static>(
	State(state): State<Arc<A2AState<H>>>,
	headers: axum::http::HeaderMap,
	body: Bytes,
) -> Response {
	// Parse the raw bytes manually so that a malformed body produces a
	// JSON-RPC PARSE_ERROR response (HTTP 200) rather than axum's default
	// 422 Unprocessable Entity. The JSON-RPC spec requires parse failures
	// to be reported through the protocol envelope, not the HTTP layer.
	//
	// serde_json enforces a default recursion limit of 128 nesting levels,
	// protecting against stack overflow from adversarial deeply-nested payloads.
	let request: JsonRpcRequest = match serde_json::from_slice(&body) {
		Ok(req) => req,
		Err(error) => {
			return Json(JsonRpcResponse::error(
				A2AError::parse_error(error),
				serde_json::Value::Null,
			))
			.into_response();
		}
	};

	// Promote HTTP headers into a RequestContext so handlers can
	// access auth tokens and tracing IDs without touching HTTP types.
	let context = build_request_context(&headers);

	// Determine if this is a streaming method before dispatching,
	// so we can handle the borrow lifetime correctly.
	let is_streaming = a2a_server::dispatch::is_streaming_method(&request.method);

	if is_streaming {
		// For streaming methods, we need the handler to outlive the
		// SSE stream. We spawn a channel-based bridge: dispatch into
		// the handler (borrowing from the Arc), then forward events
		// through a tokio channel that owns the data.
		handle_streaming_rpc(state, context, request).await
	} else {
		// For request-response methods, the dispatch completes before
		// we return, so the borrow is straightforward.
		match dispatch(&state.handler, &context, request).await {
			DispatchResult::Response(rpc_response) => Json(rpc_response).into_response(),
			DispatchResult::Stream { .. } => Json(JsonRpcResponse::error(
				A2AError::internal_error("non-streaming method returned a stream"),
				serde_json::Value::Null,
			))
			.into_response(),
		}
	}
}

/// Construct a `RequestContext` from the incoming HTTP headers.
///
/// Extracts well-known authentication and tracing headers and inserts
/// them as typed values into the context's extension map. Handler
/// implementations retrieve these values by type without ever touching
/// the HTTP layer directly.
///
/// Recognised headers:
///   - `Authorization: Bearer <token>` → `BearerToken`
///   - `X-API-Key: <key>` → `ApiKey`
///   - `X-Request-Id` or `X-Correlation-Id` → `CorrelationId`
///     (X-Request-Id takes precedence when both are present)
fn build_request_context(headers: &axum::http::HeaderMap) -> RequestContext {
	use a2a_server::context::{ApiKey, BearerToken, CorrelationId};

	const BEARER_PREFIX: &str = "Bearer ";

	let mut context = RequestContext::new();

	// RFC 7235 §2.1 and RFC 6750 §2.1 specify that the auth-scheme
	// token ("Bearer") is case-insensitive. Many HTTP libraries and
	// proxies normalise it to lowercase, so a case-sensitive check
	// would silently reject valid credentials from those callers.
	if let Some(auth) = headers.get(axum::http::header::AUTHORIZATION)
		&& let Ok(value) = auth.to_str()
		&& value.len() > BEARER_PREFIX.len()
		&& value[..BEARER_PREFIX.len()].eq_ignore_ascii_case(BEARER_PREFIX)
	{
		context
			.extensions_mut()
			.insert(BearerToken(value[BEARER_PREFIX.len()..].to_owned()));
	}

	if let Some(key) = headers.get("x-api-key")
		&& let Ok(value) = key.to_str()
	{
		context.extensions_mut().insert(ApiKey(value.to_owned()));
	}

	if let Some(id) = headers
		.get("x-request-id")
		.or_else(|| headers.get("x-correlation-id"))
		&& let Ok(value) = id.to_str()
	{
		context
			.extensions_mut()
			.insert(CorrelationId(value.to_owned()));
	}

	context
}

/// Serialise a value to JSON for an SSE data frame.
///
/// Falls back to a static `A2AError` JSON string if serialisation itself
/// fails. The fallback is a valid `A2AError` object (`code` + `message`)
/// that the client can parse as an error item in the stream. This path
/// can only fire if `serde_json::to_string` fails on a type that
/// derives Serialize — essentially unreachable, but the fallback
/// ensures the SSE stream never emits a frame that silently disappears.
fn serialise_or_error(value: &impl serde::Serialize) -> String {
	serde_json::to_string(value).unwrap_or_else(|_| {
		// Don't include the serde error text—it may contain Rust type
		// names and internal structure paths that shouldn't reach the wire.
		r#"{"code":-32603,"message":"internal error: serialisation failed"}"#.to_string()
	})
}

/// Handle a streaming JSON-RPC request.
///
/// Dispatches the request and inspects the result. If the handler
/// returned a stream, bridges it through a tokio channel into an SSE
/// response. If the handler returned a single response (e.g. an error
/// like `UnsupportedOperation` before streaming could begin), returns
/// it as a regular JSON-RPC response instead of starting an SSE stream.
/// This ensures the client receives the original error code and message
/// through the standard JSON-RPC envelope rather than losing it in a
/// bare SSE data frame that cannot be parsed as a `StreamResponse`.
async fn handle_streaming_rpc<H: A2AHandler + 'static>(
	state: Arc<A2AState<H>>,
	context: RequestContext,
	request: JsonRpcRequest,
) -> Response {
	// Clone the request ID before moving the request into the spawned
	// task. If the task panics or is cancelled, this clone lets the
	// recovery path echo the correct ID back to the client.
	let request_id = request.id.clone();
	let (result_sender, result_receiver) = tokio::sync::oneshot::channel();

	tokio::spawn(async move {
		let result = dispatch(&state.handler, &context, request).await;
		match result {
			DispatchResult::Response(rpc_response) => {
				// Pre-stream error (e.g. UnsupportedOperation, invalid params).
				// Send back as a regular JSON-RPC response so the client
				// receives the original error code and message.
				let _ = result_sender.send(Err(rpc_response));
			}
			DispatchResult::Stream { events, .. } => {
				// Successful stream setup. Create a channel and forward
				// events through it for the SSE response.
				let (event_sender, event_receiver) = tokio::sync::mpsc::channel(32);
				let _ = result_sender.send(Ok(event_receiver));
				forward_stream_events(events, &event_sender).await;
			}
		}
	});

	match result_receiver.await {
		Ok(Ok(receiver)) => {
			// Stream path: convert channel receiver into SSE events.
			let sse_stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
				let data = receiver.recv().await?;
				Some((Ok::<_, Infallible>(Event::default().data(data)), receiver))
			});
			Sse::new(sse_stream)
				.keep_alive(KeepAlive::default())
				.into_response()
		}
		Ok(Err(rpc_response)) => {
			// Error path: return a regular JSON-RPC response.
			Json(rpc_response).into_response()
		}
		Err(_) => {
			// The spawned task panicked or was cancelled.
			Json(JsonRpcResponse::error(
				A2AError::internal_error("streaming dispatch task failed"),
				request_id,
			))
			.into_response()
		}
	}
}

/// Drive a stream of events, serialising each one and sending it through
/// the channel. Stops when the stream ends or the receiver disconnects.
async fn forward_stream_events(
	events: a2a_server::handler::EventStream<'_>,
	sender: &tokio::sync::mpsc::Sender<String>,
) {
	use futures_util::StreamExt;
	let mut events = events;
	while let Some(event) = events.next().await {
		let serialised = match event {
			Ok(ref stream_event) => serialise_or_error(stream_event),
			Err(ref error) => serialise_or_error(error),
		};
		if sender.send(serialised).await.is_err() {
			break;
		}
	}
}

/// Serve the agent's public card at /.well-known/agent.json.
///
/// Returns the `AgentCard` as JSON with no authentication required.
/// This is the discovery endpoint that callers use to learn about
/// the agent's capabilities before sending requests.
async fn handle_agent_card<H: A2AHandler + 'static>(
	State(state): State<Arc<A2AState<H>>>,
) -> Json<AgentCard> {
	Json(state.agent_card.clone())
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::body::Body;
	use axum::http::{Request, StatusCode};
	use tower::ServiceExt;

	use a2a::agent_card::{AgentCapabilities, AgentCard, AgentCardRequired};
	use a2a::error;
	use a2a::jsonrpc::JsonRpcResponse;
	use a2a::message::Message;
	use a2a::operation::{SendMessageParams, SendMessageResult};
	use a2a::role::Role;
	use a2a_server::context::RequestContext;

	// A minimal handler that uses all defaults—rejects every
	// operation with UnsupportedOperation. Good enough for testing
	// the router plumbing without implementing real agent logic.
	struct StubHandler;
	impl A2AHandler for StubHandler {}

	// An echo handler that returns the caller's text as an agent
	// message. Used to test the success path through the full
	// HTTP pipeline—request body → JSON-RPC dispatch → handler →
	// result serialisation → HTTP response body.
	struct EchoHandler;
	impl A2AHandler for EchoHandler {
		fn message_send(
			&self,
			_context: &RequestContext,
			params: SendMessageParams,
		) -> impl std::future::Future<Output = Result<SendMessageResult, a2a::error::A2AError>> + Send + '_
		{
			async move {
				let reply = Message::text(
					format!("reply-to-{}", params.message.message_id),
					Role::Agent,
					format!("echo: {}", params.message.parts.first().unwrap()),
				);
				Ok(SendMessageResult::Message(reply))
			}
		}
	}

	// Helper to build a minimal agent card for tests using the
	// builder pattern. Only the required fields are populated.
	// Uses the v1.0 supported_interfaces field instead of the
	// removed top-level url field.
	fn test_agent_card() -> AgentCard {
		use a2a::agent_card::AgentInterface;
		AgentCard::new(AgentCardRequired {
			name: "test-agent".into(),
			description: "A test agent".into(),
			supported_interfaces: vec![AgentInterface::new(
				"https://test.example.com/a2a",
				"JSONRPC",
				"1.0",
			)],
			version: "1.0.0".into(),
			capabilities: AgentCapabilities::default(),
			skills: vec![],
			default_input_modes: vec!["text/plain".into()],
			default_output_modes: vec!["text/plain".into()],
		})
	}

	// The agent card endpoint should return the card as JSON with
	// a 200 status. This is the discovery path every A2A caller
	// hits first, so it must always work.
	#[tokio::test]
	async fn agent_card_endpoint_returns_card() {
		let app = a2a_router(StubHandler, test_agent_card());

		let response = app
			.oneshot(
				Request::builder()
					.method("GET")
					.uri("/.well-known/agent.json")
					.body(Body::empty())
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let card: AgentCard = serde_json::from_slice(&body).unwrap();
		assert_eq!(card.name, "test-agent");
	}

	// A valid JSON-RPC request to an unsupported method should return
	// a JSON-RPC error response (not an HTTP error). The router must
	// handle the dispatch error gracefully and wrap it in a proper
	// JSON-RPC envelope.
	#[tokio::test]
	async fn rpc_endpoint_returns_unsupported_error() {
		let app = a2a_router(StubHandler, test_agent_card());

		let request_body = serde_json::json!({
			"jsonrpc": "2.0",
			"method": "SendMessage",
			"params": {
				"message": {
					"messageId": "test-msg-1",
					"role": "user",
					"parts": [{"text": "hello"}]
				}
			},
			"id": "test-1"
		});

		let response = app
			.oneshot(
				Request::builder()
					.method("POST")
					.uri("/")
					.header("content-type", "application/json")
					.body(Body::from(serde_json::to_vec(&request_body).unwrap()))
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let rpc_response: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
		assert_eq!(rpc_response.id, serde_json::json!("test-1"));
		let err = rpc_response.a2a_error().unwrap();
		assert_eq!(err.code, error::code::UNSUPPORTED_OPERATION);
	}

	// An unknown method should return METHOD_NOT_FOUND through the
	// full HTTP pipeline. This tests the integration between axum's
	// JSON extraction, the dispatch layer, and the response encoding.
	#[tokio::test]
	async fn rpc_endpoint_returns_method_not_found() {
		let app = a2a_router(StubHandler, test_agent_card());

		let request_body = serde_json::json!({
			"jsonrpc": "2.0",
			"method": "bogus/method",
			"params": {},
			"id": "test-2"
		});

		let response = app
			.oneshot(
				Request::builder()
					.method("POST")
					.uri("/")
					.header("content-type", "application/json")
					.body(Body::from(serde_json::to_vec(&request_body).unwrap()))
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let rpc_response: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
		let err = rpc_response.a2a_error().unwrap();
		assert_eq!(err.code, error::code::METHOD_NOT_FOUND);
		assert!(err.message.contains("bogus/method"));
	}

	// The request ID must survive the full round-trip through HTTP.
	// Numeric IDs are especially important to test because JSON
	// serialisation can accidentally coerce numbers to strings.
	#[tokio::test]
	async fn numeric_request_id_preserved() {
		let app = a2a_router(StubHandler, test_agent_card());

		let request_body = serde_json::json!({
			"jsonrpc": "2.0",
			"method": "GetTask",
			"params": {"id": "task-1"},
			"id": 42
		});

		let response = app
			.oneshot(
				Request::builder()
					.method("POST")
					.uri("/")
					.header("content-type", "application/json")
					.body(Body::from(serde_json::to_vec(&request_body).unwrap()))
					.unwrap(),
			)
			.await
			.unwrap();

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let rpc_response: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
		assert_eq!(rpc_response.id, serde_json::json!(42));
	}

	// A successful SendMessage through the full HTTP pipeline.
	// This is the end-to-end success path: HTTP request → axum
	// JSON extraction → JSON-RPC dispatch → handler → result
	// serialisation → JSON-RPC response → HTTP response body.
	// The echo handler returns a Message variant, which the caller
	// deserialises back into a SendMessageResult.
	#[tokio::test]
	async fn successful_message_send_through_http() {
		let app = a2a_router(EchoHandler, test_agent_card());

		let request_body = serde_json::json!({
			"jsonrpc": "2.0",
			"method": "SendMessage",
			"params": {
				"message": {
					"messageId": "http-echo-1",
					"role": "user",
					"parts": [{"text": "hello through http"}]
				}
			},
			"id": "http-req-1"
		});

		let response = app
			.oneshot(
				Request::builder()
					.method("POST")
					.uri("/")
					.header("content-type", "application/json")
					.body(Body::from(serde_json::to_vec(&request_body).unwrap()))
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let rpc_response: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
		assert_eq!(rpc_response.id, serde_json::json!("http-req-1"));
		assert!(rpc_response.is_success());

		let send_result: SendMessageResult =
			serde_json::from_value(rpc_response.into_result().unwrap()).unwrap();
		match send_result {
			SendMessageResult::Message(message) => {
				assert_eq!(message.role, Role::Agent);
				assert_eq!(message.message_id, "reply-to-http-echo-1");
			}
			SendMessageResult::Task(_) => panic!("expected Message variant"),
			_ => panic!("unexpected SendMessageResult variant"),
		}
	}

	// Loopback integration tests spin up a real axum TCP listener on a
	// random OS-assigned port and exercise the full HTTP stack (TCP →
	// axum → JSON-RPC dispatch → handler → serialisation → HTTP
	// response). They require both the `axum` and `client` features
	// because they use reqwest directly. The outer `#[cfg(test)]` block
	// already gates on `axum`; the inner cfg narrows further to `client`.
	mod loopback {
		use super::*;
		use a2a::operation::SendMessageParams;
		use a2a_client::discovery::discover_agent;
		use a2a_client::rpc::A2AClient;

		// Loopback integration: spin up the axum server on a random port,
		// discover the agent card via HTTP, construct a client from it, and
		// send a message through the full HTTP stack. This exercises the
		// complete pipeline in both directions:
		//
		//   HTTP POST (reqwest) → TCP → axum routing → JSON-RPC dispatch
		//   → EchoHandler → result serialisation → HTTP 200 → reqwest
		//   → JSON-RPC deserialisation → SendMessageResult
		//
		// Using a random OS-assigned port (bind "127.0.0.1:0") avoids port
		// collisions when multiple test suites run in parallel. The spawned
		// task is not awaited—it runs until the test process exits, which is
		// acceptable for a loopback integration test that does not inspect
		// server-side shutdown behaviour.
		#[tokio::test]
		async fn loopback_discover_and_send_message() {
			let app = a2a_router(EchoHandler, test_agent_card());
			let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
				.await
				.expect("OS must be able to bind a loopback TCP listener on a random port");
			let loopback_address = listener
				.local_addr()
				.expect("bound listener must report its local address");

			tokio::spawn(async move {
				axum::serve(listener, app)
					.await
					.expect("axum server must not fail during the test");
			});

			// Discover the agent card over real HTTP. This exercises the
			// /.well-known/agent.json route and the AgentCard JSON schema
			// through the full network stack rather than tower oneshot.
			let http = reqwest::Client::new();
			let card = discover_agent(&http, &format!("http://{loopback_address}"))
				.await
				.expect("agent card discovery must succeed over loopback");
			assert_eq!(
				card.name, "test-agent",
				"discovered card name must match the name set in test_agent_card()"
			);

			// Build a client from the discovered card. The test card stores a
			// static https://test.example.com/a2a URL in supported_interfaces,
			// so we construct the client directly with the live loopback address
			// rather than relying on from_agent_card().
			let client = A2AClient::new(http, format!("http://{loopback_address}/"));

			// Send a message and verify the echo handler echoes it back as an
			// agent Message. This is the core success path exercised over real
			// TCP.
			let params = SendMessageParams::new(a2a::message::Message::text(
				"loopback-msg-1",
				a2a::role::Role::User,
				"hello loopback",
			));
			let result = client
				.message_send(&params)
				.await
				.expect("message_send must succeed over loopback");
			match result {
				a2a::operation::SendMessageResult::Message(msg) => {
					assert_eq!(
						msg.role,
						a2a::role::Role::Agent,
						"echo handler must reply with the Agent role"
					);
					assert_eq!(
						msg.message_id, "reply-to-loopback-msg-1",
						"echo handler must set the reply ID to reply-to-<original-id>"
					);
				}
				_ => {
					panic!("EchoHandler returns a Message, not a Task—expected Message variant")
				}
			}
		}

		// Loopback: an unrecognised JSON-RPC method name must return a
		// METHOD_NOT_FOUND error with HTTP 200. This validates that the
		// method-not-found path in the dispatch layer survives the full
		// HTTP round-trip—the JSON-RPC envelope must be correctly formed
		// even when no handler matches the requested method name. The
		// StubHandler (which rejects all known methods) is used here; an
		// unknown method name never reaches any handler—it fails at the
		// dispatch layer before the handler is consulted.
		#[tokio::test]
		async fn loopback_unknown_method_returns_error() {
			let app = a2a_router(StubHandler, test_agent_card());
			let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
				.await
				.expect("OS must be able to bind a loopback TCP listener on a random port");
			let loopback_address = listener
				.local_addr()
				.expect("bound listener must report its local address");

			tokio::spawn(async move {
				axum::serve(listener, app)
					.await
					.expect("axum server must not fail during the test");
			});

			let http = reqwest::Client::new();
			let resp = http
				.post(format!("http://{loopback_address}/"))
				.json(&serde_json::json!({
					"jsonrpc": "2.0",
					"method": "BogusMethod",
					"params": {},
					"id": "test-unknown"
				}))
				.send()
				.await
				.expect("raw HTTP POST to loopback server must succeed");

			// JSON-RPC requires HTTP 200 even for protocol-level errors.
			// The error is conveyed inside the response envelope, not via
			// the HTTP status code.
			assert_eq!(
				resp.status(),
				200,
				"JSON-RPC errors must be returned with HTTP 200, not an HTTP error status"
			);

			let rpc: a2a::jsonrpc::JsonRpcResponse = resp
				.json()
				.await
				.expect("response body must be valid JSON-RPC");
			let err = rpc
				.a2a_error()
				.expect("response for unknown method must carry a JSON-RPC error");
			assert_eq!(
				err.code,
				a2a::error::code::METHOD_NOT_FOUND,
				"unknown method must produce METHOD_NOT_FOUND (-32601)"
			);
		}

		// Loopback SSE streaming: send a SendStreamingMessage through the
		// full HTTP stack and read back individual SSE data frames from the
		// response body. This exercises the complete streaming pipeline:
		//
		//   HTTP POST (reqwest) → TCP → axum routing → dispatch →
		//   handle_streaming_rpc → tokio::spawn → forward_stream_events →
		//   serialise_or_error → mpsc channel → unfold → Sse → KeepAlive →
		//   HTTP chunked response → reqwest → SSE parsing → StreamResponse
		//
		// The StreamingHandler returns two status update events (working →
		// completed). We verify that both arrive as parseable SSE data
		// frames containing valid StreamResponse JSON.
		#[tokio::test]
		async fn loopback_streaming_message_returns_sse_events() {
			use a2a_client::sse::into_event_stream;
			use futures_util::StreamExt;

			let app = a2a_router(StreamingHandler, test_agent_card());
			let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
				.await
				.expect("OS must bind a loopback listener");
			let loopback_address = listener
				.local_addr()
				.expect("bound listener must report its address");

			tokio::spawn(async move {
				axum::serve(listener, app)
					.await
					.expect("axum server must not fail during the test");
			});

			let http = reqwest::Client::new();
			let response = http
				.post(format!("http://{loopback_address}/"))
				.json(&serde_json::json!({
					"jsonrpc": "2.0",
					"method": "SendStreamingMessage",
					"params": {
						"message": {
							"messageId": "sse-msg-1",
							"role": "user",
							"parts": [{"text": "stream me"}]
						}
					},
					"id": "sse-req-1"
				}))
				.send()
				.await
				.expect("POST to streaming endpoint must succeed");

			assert_eq!(
				response.status(),
				200,
				"streaming response must be HTTP 200"
			);

			let mut event_stream = into_event_stream(response);
			let mut events = Vec::new();
			while let Some(result) = event_stream.next().await {
				events.push(result.expect("each SSE frame must parse as a StreamResponse"));
			}

			assert_eq!(
				events.len(),
				2,
				"StreamingHandler produces exactly two events (working + completed)"
			);

			match &events[0] {
				a2a::stream_event::StreamResponse::StatusUpdate(update) => {
					assert_eq!(update.task_id, "sse-task-1");
					assert_eq!(update.status.state, a2a::task_state::TaskState::Working);
				}
				other => panic!("expected StatusUpdate for first event, got {other:?}"),
			}

			match &events[1] {
				a2a::stream_event::StreamResponse::StatusUpdate(update) => {
					assert_eq!(update.task_id, "sse-task-1");
					assert_eq!(update.status.state, a2a::task_state::TaskState::Completed);
				}
				other => panic!("expected StatusUpdate for second event, got {other:?}"),
			}
		}
	}

	// A handler that implements message_stream, returning a two-event
	// SSE stream (working → completed). Used to test the full streaming
	// pipeline through the axum router: HTTP POST → dispatch →
	// handle_streaming_rpc → forward_stream_events → SSE framing →
	// text/event-stream response body.
	struct StreamingHandler;
	impl A2AHandler for StreamingHandler {
		fn message_stream(
			&self,
			_context: &RequestContext,
			params: SendMessageParams,
		) -> impl std::future::Future<
			Output = Result<a2a_server::handler::EventStream<'_>, a2a::error::A2AError>,
		> + Send
		+ '_ {
			async move {
				let context_id = params
					.message
					.context_id
					.unwrap_or_else(|| "sse-ctx".into());
				let events: Vec<Result<a2a::stream_event::StreamResponse, a2a::error::A2AError>> = vec![
					Ok(a2a::stream_event::StreamResponse::StatusUpdate(
						a2a::stream_event::TaskStatusUpdateEvent::new(
							"sse-task-1",
							context_id.clone(),
							a2a::task::TaskStatus::working(),
						),
					)),
					Ok(a2a::stream_event::StreamResponse::StatusUpdate(
						a2a::stream_event::TaskStatusUpdateEvent::new(
							"sse-task-1",
							context_id,
							a2a::task::TaskStatus::completed(),
						),
					)),
				];
				Ok(Box::pin(futures_util::stream::iter(events))
					as a2a_server::handler::EventStream<'_>)
			}
		}
	}

	// A handler that captures the BearerToken from the RequestContext
	// and echoes it back in the reply text. This is used to verify
	// that the Authorization header extracted by build_request_context
	// actually reaches the handler unchanged through the full pipeline.
	//
	// The handler accesses the context extension map for the token and
	// falls back to "no-token" when the header is absent, making it
	// straightforward to assert on both the present and absent cases.
	struct AuthCapturingHandler;
	impl A2AHandler for AuthCapturingHandler {
		fn message_send(
			&self,
			context: &RequestContext,
			_params: SendMessageParams,
		) -> impl std::future::Future<Output = Result<SendMessageResult, a2a::error::A2AError>> + Send + '_
		{
			use a2a_server::context::BearerToken;
			let token_value = context
				.extensions()
				.get::<BearerToken>()
				.map(|t| t.0.clone())
				.unwrap_or_else(|| "no-token".into());
			async move {
				let reply = Message::text("auth-reply", Role::Agent, token_value);
				Ok(SendMessageResult::Message(reply))
			}
		}
	}

	// The Authorization header sent by the HTTP caller must reach the
	// handler as a BearerToken in the RequestContext extension map.
	//
	// This test exercises the full pipeline from HTTP header to handler:
	//   HTTP Authorization header → build_request_context → BearerToken
	//   inserted into Extensions → RequestContext passed to dispatch →
	//   handler reads BearerToken → echoes it in the reply text.
	//
	// Confirming end-to-end delivery is essential because the
	// build_request_context function is the only place the HTTP layer
	// and the handler layer meet. If the token fails to propagate, the
	// test will see "no-token" in the reply rather than the actual token.
	#[tokio::test]
	async fn authorization_header_reaches_handler() {
		let app = a2a_router(AuthCapturingHandler, test_agent_card());

		let request_body = serde_json::json!({
			"jsonrpc": "2.0",
			"method": "SendMessage",
			"params": {
				"message": {
					"messageId": "auth-test-msg-1",
					"role": "user",
					"parts": [{"text": "check my token"}]
				}
			},
			"id": "auth-req-1"
		});

		let response = app
			.oneshot(
				Request::builder()
					.method("POST")
					.uri("/")
					.header("content-type", "application/json")
					.header("authorization", "Bearer test-jwt-token-xyz")
					.body(Body::from(serde_json::to_vec(&request_body).unwrap()))
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let rpc_response: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
		assert_eq!(rpc_response.id, serde_json::json!("auth-req-1"));
		assert!(
			rpc_response.is_success(),
			"handler must succeed when a valid Bearer token is present"
		);

		let send_result: SendMessageResult =
			serde_json::from_value(rpc_response.into_result().unwrap()).unwrap();
		match send_result {
			SendMessageResult::Message(message) => {
				assert_eq!(message.role, Role::Agent, "reply must carry the Agent role");
				// The handler echoes the raw token value as the reply text.
				// Any value other than the exact token indicates the header
				// failed to propagate through the context pipeline.
				assert_eq!(
					message.parts.first().unwrap().to_string(),
					"test-jwt-token-xyz",
					"handler must receive the Bearer token from the Authorization header"
				);
			}
			_ => {
				panic!("expected Message variant from AuthCapturingHandler")
			}
		}
	}

	// A request body that is not valid JSON must produce a JSON-RPC
	// PARSE_ERROR response with HTTP 200—not a 422 or 400. The JSON-RPC
	// specification requires all protocol errors to be reported through
	// the response envelope rather than the HTTP status code. When no
	// request ID can be decoded from a malformed body, the spec dictates
	// that the response ID field is set to null.
	#[tokio::test]
	async fn invalid_json_body_returns_parse_error() {
		let app = a2a_router(StubHandler, test_agent_card());

		let response = app
			.oneshot(
				Request::builder()
					.method("POST")
					.uri("/")
					.header("content-type", "application/json")
					.body(Body::from(b"{ this is not valid json !!!".as_ref()))
					.unwrap(),
			)
			.await
			.unwrap();

		// The HTTP status must be 200—the error lives inside the envelope.
		assert_eq!(response.status(), StatusCode::OK);

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let rpc_response: JsonRpcResponse = serde_json::from_slice(&body).unwrap();

		// The response ID must be null when the request body could not be
		// parsed—there is no request ID to echo back.
		assert_eq!(rpc_response.id, serde_json::Value::Null);

		let err = rpc_response
			.a2a_error()
			.expect("response should carry an error");
		assert_eq!(
			err.code,
			error::code::PARSE_ERROR,
			"error code must be PARSE_ERROR (-32700)"
		);
		assert!(
			err.message.contains("parse error"),
			"error message should include the 'parse error' prefix"
		);
	}

	// RFC 7235 §2.1 specifies that the auth-scheme token is case-insensitive.
	// A caller sending "bearer" (lowercase) must have the token extracted and
	// delivered to the handler identically to "Bearer" (title case). This test
	// verifies that the case-insensitive prefix check in build_request_context
	// correctly handles the lowercase variant that many HTTP libraries produce.
	#[tokio::test]
	async fn lowercase_bearer_scheme_reaches_handler() {
		let app = a2a_router(AuthCapturingHandler, test_agent_card());

		let request_body = serde_json::json!({
			"jsonrpc": "2.0",
			"method": "SendMessage",
			"params": {
				"message": {
					"messageId": "auth-lower-1",
					"role": "user",
					"parts": [{"text": "check lowercase bearer"}]
				}
			},
			"id": "auth-lower-req-1"
		});

		let response = app
			.oneshot(
				Request::builder()
					.method("POST")
					.uri("/")
					.header("content-type", "application/json")
					.header("authorization", "bearer lowercase-token-xyz")
					.body(Body::from(serde_json::to_vec(&request_body).unwrap()))
					.unwrap(),
			)
			.await
			.unwrap();

		assert_eq!(response.status(), StatusCode::OK);

		let body = axum::body::to_bytes(response.into_body(), usize::MAX)
			.await
			.unwrap();
		let rpc_response: JsonRpcResponse = serde_json::from_slice(&body).unwrap();
		assert!(
			rpc_response.is_success(),
			"handler must succeed with lowercase bearer scheme"
		);

		let send_result: SendMessageResult =
			serde_json::from_value(rpc_response.into_result().unwrap()).unwrap();
		match send_result {
			SendMessageResult::Message(message) => {
				assert_eq!(
					message.parts.first().unwrap().to_string(),
					"lowercase-token-xyz",
					"handler must receive the token even when the scheme is lowercase"
				);
			}
			_ => {
				panic!("expected Message variant from AuthCapturingHandler")
			}
		}
	}
}
