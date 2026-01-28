#![warn(clippy::pedantic, clippy::nursery, clippy::all, clippy::cargo)]
#![allow(clippy::multiple_crate_versions, clippy::module_name_repetitions)]

//! # Artiqwest
//! Artiqwest is a simple HTTP client that routes all requests through the Tor network using the `arti_client` and `hyper`.
//! It provides two basic primitives: `get` and `post` functions.
//!
//! ## Example
//! ```rust
//! use artiqwest::get;
//! use artiqwest::post;
//!
//! #[tokio::main]
//! async fn main() {
//!         // Make a GET request to httpbin.org
//!         let response = get("https://httpbin.org/get", None).await.unwrap();
//!         assert_eq!(response.status(), 200);
//!
//!         // Make a POST request to a hidden service
//!         let body = r#"{"test": "testing"}"#;
//!         let headers = vec![("Content-Type", "application/json")];
//!         let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", body, Some(headers), None).await.unwrap();
//!         assert_eq!(response.to_string(), body);
//! }
//! ```

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{Result, bail};
use arti_client::config::TorClientConfigBuilder;
use arti_client::{TorClient, TorClientConfig};
use error::Error;
use futures_util::StreamExt;
use make_request::MakeRequest;
use make_request::{make_local_request, make_request};
pub use response::Response;
pub(crate) use response::{UpstreamRequest, UpstreamResponse};
use streams::{create_http_stream, https_upgrade};
use tokio::sync::Mutex as TokioMutex;
use tokio_tungstenite::tungstenite::Message;
use tor_client::get_or_refresh;
use tor_rtcompat::PreferredRuntime;
use tracing::{Level, event, span};
use uri::Uri;
use uri::parse_uri;
use uuid::Uuid;

mod error;
mod make_request;
mod response;
mod streams;
mod tor_client;
mod uri;

static INSTANCE_ID: LazyLock<Uuid> = LazyLock::new(Uuid::new_v4); // Initialize TOR_CONFIG here

static TOR_CONFIG: LazyLock<TorClientConfig> = LazyLock::new(|| {
	let mut default_config = TorClientConfigBuilder::from_directories("./tor/state/".to_owned() + &INSTANCE_ID.to_string(), "./tor/cache/".to_owned() + &INSTANCE_ID.to_string());
	//let mut default_config = TorClientConfigBuilder::default();
	default_config.address_filter().allow_onion_addrs(true);
	default_config.build().unwrap()
});

static TOR_CLIENT: LazyLock<TokioMutex<Option<TorClient<PreferredRuntime>>>> = LazyLock::new(|| TokioMutex::new(None));

/// Send `GET` request to the specified URI over the TOR network.
///
/// # Example
/// ```rust
/// use artiqwest::get;
///
/// #[tokio::main]
/// async fn main() {
///         let response = get("https://httpbin.org/get", None).await.unwrap();
///         assert!(response.status().is_success());
/// }
/// ```
///
/// # Errors
/// 1. If the URI is invalid.
/// 2. If the stream cannot be created.
/// 3. If the request cannot be made.
/// 4. If the request cannot be made over HTTPS.
/// 5. If handshake with server fails.
/// 6. If the TOR connection is dropped.
pub async fn get(uri: &str, existing_client: Option<TorClient<PreferredRuntime>>) -> Result<Response> {
	let get_span = span!(Level::INFO, "get");
	let _guard = get_span.enter();

	event!(Level::INFO, "Making a GET request to {}", uri);

	let uri = parse_uri(uri)?;
	let m_r = MakeRequest { uri: uri.clone(), headers: Option::default(), body: Option::default(), method: hyper::Method::GET, version: hyper::Version::HTTP_2 };

	if uri.is_local {
		return make_local_request(m_r).await;
	}

	event!(Level::INFO, "Creating a stream to {}", uri.to_string());
	let stream = match create_http_stream(&uri, 5, existing_client).await {
		Ok(s) => s,
		Err(e) => {
			event!(Level::ERROR, "Failed to create a stream: {}", e);
			return Err(e);
		}
	};

	if uri.is_https {
		let stream = https_upgrade(&uri, stream, &["h2", "http/1.1"]).await?;
		make_request(m_r, stream).await
	} else {
		make_request(m_r, stream).await
	}
}

/// Send `POST` request to the specified URI over the TOR network.
///
/// # Example
/// ```rust
/// use artiqwest::post;
///
/// #[tokio::main]
/// async fn main() {
///         let body = r#"{"test": "testing"}"#;
///         let headers = vec![("Content-Type", "application/json")];
///         let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", body, Some(headers), None).await.unwrap();
///         assert_eq!(response.to_string(), body);
/// }
/// ```
///
/// # Errors
/// 1. If the URI is invalid.
/// 2. If the stream cannot be created.
/// 3. If the request cannot be made.
/// 4. If the request cannot be made over HTTPS.
/// 5. If handshake with server fails.
/// 6. If the TOR connection is dropped.
pub async fn post(uri: &str, body: &str, headers: Option<Vec<(&str, &str)>>, existing_client: Option<TorClient<PreferredRuntime>>) -> Result<Response> {
	let post_span = span!(Level::INFO, "post");
	let _guard = post_span.enter();

	event!(Level::INFO, "Making a POST request to {}", uri);

	let uri = parse_uri(uri)?;
	let headers = headers.unwrap_or_default();
	let headers: HashMap<String, String> = headers.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect();
	let m_r = MakeRequest { uri: uri.clone(), headers: Some(headers), body: Some(body.to_string()), method: hyper::Method::POST, version: hyper::Version::HTTP_2 };

	if uri.is_local {
		return make_local_request(m_r).await;
	}

	let stream = create_http_stream(&uri, 5, existing_client).await?;

	if uri.is_https {
		let stream = https_upgrade(&uri, stream, &["h2", "http/1.1"]).await?;
		make_request(m_r, stream).await
	} else {
		make_request(m_r, stream).await
	}
}

/// Get a websocket connection to the specified URI over the TOR network.
///
/// # Example
/// ```rust
/// use artiqwest::ws;
/// use futures_util::future;
/// use futures_util::pin_mut;
/// use futures_util::SinkExt;
/// use futures_util::StreamExt;
/// use tokio_tungstenite::tungstenite::Message;
///
/// #[tokio::main]
/// async fn main() {
///     let (mut write, mut read) = ws("wss://ydrkehoqxt2q5atkmiyw7gmphvrmp6fkaufvt525cjr4hma3pb75nyid.onion/events", None).await.unwrap();
///     let write_messages = {
///         async {
///             for i in 1..=5 {
///                 write.send(Message::Text(format!("Hello WebSocket #{}", i))).await.unwrap();
///                 println!("Sending message {}: Hello WebSocket #{}", i, i);
///                 tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
///             }
///             println!("Finished sending 5 messages after 5 seconds");
///             // Add close
///             write.close().await.unwrap();
///         }
///     };
///
///     let read_messages = {
///         read.for_each(|message| async {
///             if let Ok(msg) = message {
///                 if msg.is_close() { return; }
///                 let data = msg.into_data();
///                 let text = String::from_utf8(data).unwrap();
///                 println!("Received: {}", text);
///             }
///         })
///     };
///
///     pin_mut!(write_messages, read_messages);
///     future::select(write_messages, read_messages).await;
/// }
/// ```
///
/// # Errors
/// 1. If the URI is invalid.
/// 2. If the stream cannot be created.
/// 3. If the request cannot be made.
/// 4. If the request cannot be made over HTTPS.
/// 5. If handshake with server fails.
pub async fn ws(uri: &str, existing_client: Option<TorClient<PreferredRuntime>>) -> Result<(Box<dyn futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Send + Unpin>, Box<dyn futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Send + Unpin>)> {
	let ws_span = span!(Level::INFO, "ws");
	let _guard = ws_span.enter();

	let uri = crate::parse_uri(uri)?;

	// Handle local addresses directly without going through Tor (similar to get/post functions)
	if uri.is_local {
		return ws_local(&uri).await;
	}

	let stream = crate::create_http_stream(&uri, 5, existing_client).await?;

	if uri.is_https {
		let tls_stream = crate::https_upgrade(&uri, stream, &["http/1.1"]).await?;
		let (ws_stream, _) = match tokio_tungstenite::client_async(&uri.to_string(), tls_stream).await {
			Ok((ws_stream, response)) => (ws_stream, response),
			Err(e) => {
				event!(Level::ERROR, "Failed to connect to the websocket server: {}", e);
				bail!(Error::Tungstenite(e))
			}
		};

		let (write, read) = ws_stream.split();
		Ok((Box::new(write), Box::new(read)))
	} else {
		let (ws_stream, _) = match tokio_tungstenite::client_async(&uri.to_string(), stream).await {
			Ok((ws_stream, response)) => (ws_stream, response),
			Err(e) => return Err(Error::Tungstenite(e).into()),
		};

		let (write, read) = ws_stream.split();
		Ok((Box::new(write), Box::new(read)))
	}
}

/// Handle WebSocket connections to local addresses without going through Tor
async fn ws_local(uri: &Uri) -> Result<(Box<dyn futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Send + Unpin>, Box<dyn futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Send + Unpin>)> {
	event!(Level::INFO, "Creating local WebSocket connection to {}", uri.to_string());

	// For local connections, use tokio-tungstenite's connect_async directly
	// This handles the HTTP upgrade and WebSocket handshake automatically
	let url = uri.to_string();

	let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await.map_err(Error::Tungstenite)?;

	let (write, read) = ws_stream.split();
	Ok((Box::new(write), Box::new(read)))
}

#[cfg(test)]
mod tests {
	// Remove unused imports:
	// use std::ops::ControlFlow;
	// use bytes::Bytes;

	// Keep only necessary imports:
	use axum::extract::ws::WebSocketUpgrade;
	use axum::response::IntoResponse;
	use axum::routing::{get as axum_get, post as axum_post};
	use axum::serve as axum_serve;
	use futures_util::SinkExt;
	use onyums::Router;
	use onyums::get_onion_name;
	use onyums::serve;
	use rand::Rng; // Keep this import
	use serde_json::json;
	use serial_test::serial; // Add this import
	use tokio_tungstenite::tungstenite::Message;

	use super::*;

	#[tokio::test]
	#[serial] // Add this attribute
	async fn test_get() {
		let tracing_subscriber = tracing_subscriber::fmt::Subscriber::builder().with_max_level(tracing::Level::ERROR).finish();
		let _ = tracing::subscriber::set_global_default(tracing_subscriber);

		// test onion get
		println!("Testing onion get request");
		let onion_name = create_onyums_server().await;
		println!("Onion address: {onion_name}");

		let response = get(&format!("https://{onion_name}"), None).await.unwrap();
		println!("response: {}", json!(response));
		assert!(response.to_string().contains("World"));

		// test local get
		println!("Testing local get request");
		let local_server = spawn_axum_server().await;
		println!("Local server address: {local_server}");

		let response = get(&format!("http://{local_server}/"), None).await.unwrap();
		println!("response: {}", json!(response));
		assert!(response.to_string().contains("World"));

		// test external get
		println!("Testing external get request");

		let response = get("https://httpbin.org/get", None).await.unwrap();
		println!("response: {}", json!(response));
		assert!(response.status().is_success());
	}

	#[tokio::test]
	#[serial] // Add this attribute
	async fn test_post() {
		let tracing_subscriber = tracing_subscriber::fmt::Subscriber::builder().with_max_level(tracing::Level::ERROR).finish();
		let _ = tracing::subscriber::set_global_default(tracing_subscriber);

		// test onion post
		println!("Testing onion post request");
		let onion_name = create_onyums_server().await;
		println!("Onion address: {onion_name}");

		let post_body = r#"{"test":"testing"}"#;
		let response = post(&format!("https://{onion_name}/echo"), post_body, None, None).await.unwrap();
		println!("body: {response}");
		assert!(response.to_string().contains("test"));

		// test local post
		println!("Testing local post request");
		let local_server = spawn_axum_server().await;
		println!("Local server address: {local_server}");

		let post_body = r#"{"test":"testing"}"#;
		let response = post(&format!("http://{local_server}/echo"), post_body, None, None).await.unwrap();
		println!("body: {response}");
		assert!(response.to_string().contains("test"));

		// test external post
		println!("Testing external post request");
		let post_body = r#"{"test":"testing"}"#;
		let response = post("https://httpbin.org/post", post_body, None, None).await.unwrap();
		println!("body: {response}");
		assert!(response.to_string().contains("test"));
	}

	#[tokio::test]
	#[serial] // Add this attribute
	async fn test_ws() {
		let tracing_subscriber = tracing_subscriber::fmt::Subscriber::builder().with_max_level(tracing::Level::ERROR).finish();
		let _ = tracing::subscriber::set_global_default(tracing_subscriber);

		// test onion websocket
		println!("Testing onion websocket");
		let onion_name = create_onyums_server().await;
		println!("Onion address: {onion_name}");

		let (mut write, mut read) = ws(&format!("wss://{onion_name}/events"), None).await.unwrap();
		println!("WebSocket connection established successfully");

		let write_messages = async {
			let mut sent = 0;
			for i in 1..=5 {
				write.send(Message::Text(format!("Hello WebSocket #{i}").into())).await.unwrap();
				println!("Sending message {i}: Hello WebSocket #{i}");
				sent += 1;
				tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
			}
			println!("Finished sending 5 messages after 5 seconds");
			write.close().await.unwrap();
			sent
		};

		let read_messages = async {
			let mut received = 0;
			while let Some(message) = read.next().await {
				if let Ok(msg) = message {
					if msg.is_close() {
						break;
					}
					let data = msg.into_data();
					let text = String::from_utf8(data.to_vec()).unwrap();
					println!("Received message {}: {}", received + 1, text);
					received += 1;
				}
			}
			received
		};

		let (sent_count, received_count) = tokio::join!(write_messages, read_messages);
		println!("Finished sending {sent_count} messages");
		println!("Finished receiving {received_count} messages");

		// test local websocket
		println!("Testing local websocket");
		let local_server = spawn_axum_server_for_ws().await;
		println!("Local server address: {local_server}");

		// Verify HTTP endpoint works first
		let client = reqwest::Client::new();
		let response = client.get(format!("http://{local_server}/")).send().await.unwrap();
		assert!(response.status().is_success());
		println!("HTTP endpoint verified");

		// Then try WebSocket
		let (mut write, mut read) = ws(&format!("ws://{local_server}/events"), None).await.expect("Failed to connect WebSocket");
		println!("WebSocket connection established successfully");

		let write_messages = async {
			let mut sent = 0;
			for i in 1..=5 {
				write.send(Message::Text(format!("Hello WebSocket #{i}").into())).await.unwrap();
				println!("Sending message {i}: Hello WebSocket #{i}");
				sent += 1;
				tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
			}
			println!("Finished sending 5 messages after 5 seconds");
			write.close().await.unwrap();
			sent
		};

		let read_messages = async {
			let mut received = 0;
			while let Some(message) = read.next().await {
				if let Ok(msg) = message {
					if msg.is_close() {
						break;
					}
					let data = msg.into_data();
					let text = String::from_utf8(data.to_vec()).unwrap();
					println!("Received message {}: {}", received + 1, text);
					received += 1;
				}
			}
			received
		};

		let (sent_count, received_count) = tokio::join!(write_messages, read_messages);
		println!("Finished sending {sent_count} messages");
		println!("Finished receiving {received_count} messages");

		// test external websocket
		println!("Testing external websocket");
		// let (mut write, mut read) = ws("wss://echo.websocket.org", None).await.unwrap();

		// For the external websocket test, let's use a different service that definitely supports WebSocket over HTTP/1.1:
		let (mut write, read) = ws("wss://ws.postman-echo.com/raw", None).await.unwrap();

		let write_messages = {
			async {
				let start_time = tokio::time::Instant::now();
				let duration = tokio::time::Duration::from_secs(5); // Reduced to 5 seconds
				let mut count = 0;

				println!("Starting to send messages...");
				while start_time.elapsed() < duration {
					count += 1;
					let msg = format!("Hello WebSocket #{count}");
					println!("Sending message {count}: {msg}");

					if write.send(Message::Text(msg.into())).await.is_err() {
						println!("Failed to send message {count}");
						break;
					}
					tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
				}
				println!("Finished sending {count} messages after 5 seconds");
				count
			}
		};

		let read_messages = {
			async {
				println!("Starting to read messages...");
				let mut count = 0;

				// Use take(25) to limit the number of messages and prevent infinite loop
				let mut limited_read = read.take(25);

				while let Some(message_result) = limited_read.next().await {
					match message_result {
						Ok(message) => {
							count += 1;
							let data = message.into_data();
							let text = String::from_utf8(data.to_vec()).unwrap_or_else(|_| "Invalid UTF-8".to_string());
							println!("Received message {count}: {text}");
						}
						Err(e) => {
							println!("Error receiving message: {e}");
							break;
						}
					}
				}
				println!("Finished receiving {count} messages");
				count
			}
		};

		let (write_count, read_count) = tokio::join!(write_messages, read_messages);
		println!("Finished sending {write_count} messages");
		println!("Finished receiving {read_count} messages");
	}

	async fn create_onyums_server() -> String {
		// spawn an onyums server on a new thread and return the onion address
		tokio::spawn(async {
			serve(create_router(), "my_onion").await.unwrap();
		});

		let mut onion_name = String::new();
		while onion_name.is_empty() {
			onion_name = get_onion_name();

			// wait for 200 milliseconds before checking again
			tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
		}
		onion_name
	}

	async fn spawn_axum_server() -> String {
		// Use updated rand API and fix ownership issue
		let port = rand::rng().random_range(1024..=65535);
		let address = format!("127.0.0.1:{port}");
		let address_clone = address.clone(); // Clone the address for the async block
		tokio::spawn(async move {
			let listener = tokio::net::TcpListener::bind(address_clone).await.unwrap();
			axum_serve(listener, create_router()).await.unwrap();
		});
		// Add a short delay to ensure the server is listening
		tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
		address
	}

	async fn spawn_axum_server_for_ws() -> String {
		// Use updated rand API and fix ownership issue
		let port = rand::rng().random_range(1024..=65535);
		let address = format!("127.0.0.1:{port}");
		println!("Starting test server on {address}");
		let address_clone = address.clone(); // Clone the address for the async block
		tokio::spawn(async move {
			let listener = tokio::net::TcpListener::bind(&address_clone).await.unwrap();
			println!("Test server listening on {address_clone}"); // Use address_clone here instead of address
			axum_serve(listener, create_router()).await.unwrap();
		});
		tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
		address // Now we can return the original address
	}

	fn create_router() -> Router {
		// get `/` -> "Hello, World!"
		// post `/echo` -> echo the request body
		Router::new().route("/", axum_get(|| async { "Hello, World!" })).route("/echo", axum_post(|body: String| async move { body })).route("/events", axum_get(ws_handler))
	}

	/// The handler for the HTTP request (this gets called when the HTTP GET lands at the start
	/// of websocket negotiation). After this completes, the actual switching from HTTP to
	/// websocket protocol will occur.
	/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
	/// as well as things from HTTP headers such as user-agent of the browser etc.
	async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
		println!("WebSocket upgrade requested");
		ws.on_upgrade(move |socket| async move {
			println!("WebSocket connection upgraded"); // Simple echo handler for testing
			let (mut sender, mut receiver) = socket.split();
			while let Some(Ok(msg)) = receiver.next().await {
				if let Err(e) = sender.send(msg).await {
					println!("Error sending message: {e}");
					break;
				}
			}
		})
	}
}
