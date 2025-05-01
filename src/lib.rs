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
//!         let response = get("https://httpbin.org/get").await.unwrap();
//!         assert_eq!(response.status(), 200);
//!
//!         // Make a POST request to a hidden service
//!         let body = r#"{"test": "testing"}"#;
//!         let headers = vec![("Content-Type", "application/json")];
//!         let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", body, Some(headers)).await.unwrap();
//!         assert_eq!(response.to_string(), body);
//! }
//! ```

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Result;
use arti_client::DataStream;
use arti_client::config::TorClientConfigBuilder;
use arti_client::{TorClient, TorClientConfig};
use error::Error;
use futures_util::StreamExt;
use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use make_request::MakeRequest;
use make_request::{make_local_request, make_request};
pub use response::Response;
pub(crate) use response::{UpstreamRequest, UpstreamResponse};
use streams::{create_http_stream, https_upgrade};
use tokio::sync::Mutex as TokioMutex;
use tokio_native_tls::TlsStream;
use tokio_tungstenite::WebSocketStream;
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
///         let response = get("https://httpbin.org/get").await.unwrap();
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
pub async fn get(uri: &str) -> Result<Response> {
	let get_span = span!(Level::INFO, "get");
	let _guard = get_span.enter();

	event!(Level::INFO, "Making a GET request to {}", uri);

	let uri = parse_uri(uri)?;
	let m_r = MakeRequest { uri: uri.clone(), headers: Option::default(), body: Option::default(), method: hyper::Method::GET, version: hyper::Version::HTTP_2 };

	if uri.is_local {
		return make_local_request(m_r).await;
	}

	event!(Level::INFO, "Creating a stream to {}", uri.to_string());
	let stream = match create_http_stream(&uri, 5).await {
		Ok(s) => s,
		Err(e) => {
			event!(Level::ERROR, "Failed to create a stream: {}", e);
			return Err(e);
		}
	};

	if uri.is_https {
		let stream = https_upgrade(&uri, stream).await?;
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
///         let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", body, Some(headers)).await.unwrap();
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
pub async fn post(uri: &str, body: &str, headers: Option<Vec<(&str, &str)>>) -> Result<Response> {
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

	let stream = create_http_stream(&uri, 5).await?;

	if uri.is_https {
		let stream = https_upgrade(&uri, stream).await?;
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
///
/// #[tokio::main]
/// async fn main() {
///     let (mut write, read) = ws("wss://ydrkehoqxt2q5atkmiyw7gmphvrmp6fkaufvt525cjr4hma3pb75nyid.onion/events").await.unwrap();
///     let write_messages = {
///         async {
///             loop {
///                 write.send(Message::Text("Hello WebSocket".to_string())).await.unwrap();
///                 tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
///             }
///         }
///     };
///
///     let read_messages = {
///         read.for_each(|message| async {
///             let data = message.unwrap().into_data();
///             let text = String::from_utf8(data).unwrap();
///             println!("Received: {text}");
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
pub async fn ws(uri: &str) -> Result<(SplitSink<WebSocketStream<TlsStream<DataStream>>, Message>, SplitStream<WebSocketStream<TlsStream<DataStream>>>)> {
	let ws_span = span!(Level::INFO, "ws");
	let _guard = ws_span.enter();

	let uri = crate::parse_uri(uri)?;
	if !uri.is_https {
		return Err(Error::InvalidUri.into());
	}
	let stream = crate::create_http_stream(&uri, 5).await?;
	let tls_stream = crate::https_upgrade(&uri, stream).await?;
	let (ws_stream, _) = match tokio_tungstenite::client_async(&uri.to_string(), tls_stream).await {
		Ok((ws_stream, response)) => (ws_stream, response),
		Err(e) => return Err(Error::Tungstenite(e).into()),
	};

	let (write, read) = ws_stream.split();
	Ok((write, read))
}

#[cfg(test)]
mod tests {
	use futures_util::SinkExt;
	use futures_util::future;
	use futures_util::pin_mut;
	use serde_json::json;

	use super::*;

	#[tokio::test]
	async fn test_get() {

		let tracing_subscriber = tracing_subscriber::fmt::Subscriber::builder().with_max_level(tracing::Level::INFO).finish();
		tracing::subscriber::set_global_default(tracing_subscriber).unwrap();

		let response = get("https://zbn3fwwpmf4xrfysoj2tvm6ay5dlxnpunoakdmgvq5mh7lcjpxqijgyd.onion").await.unwrap();
		println!("response: {}", json!(response));
		assert!(response.to_string().contains("World"));

		println!("");

		println!("Do headers exist when making a local connection?\n");
		let response = match get("http://localhost:8225/status").await {
			Ok(r) => r,
			Err(e) => {
				println!("error: {}", e);
				return;
			}
		};

		println!("response: {}", json!(response));

		println!("");

		println!("Do headers exist when making a tor connection to an outside server?\n");
		let response = get("https://echo.free.beeceptor.com").await.unwrap();
		println!("response: {}", json!(response));
	}

	#[tokio::test]
	async fn test_post() {
		let post_body = r#"{"test":"testing"}"#;
		let response = post("https://ydrkehoqxt2q5atkmiyw7gmphvrmp6fkaufvt525cjr4hma3pb75nyid.onion/echo", post_body, None).await.unwrap();
		println!("body: {}", response);
		assert!(response.to_string().contains("test"));

		let post_body = r#"{"test":"test"}"#;
		let body = post("https://echo.free.beeceptor.com", post_body, None).await.unwrap();
		println!("body: {}", body);
		assert!(body.to_string().contains("test"));

		let post_body = r#"{"test":"testing"}"#;
		let response = match post("http://localhost:8225/echo", post_body, None).await {
			Ok(r) => r,
			Err(e) => {
				println!("error: {}", e);
				return;
			}
		};
		println!("body: {}", response);
		assert!(response.to_string().contains("test"));
	}

	#[tokio::test]
	async fn test_ws() {
		let (mut write, read) = ws("wss://pxfbgtzpodrswbozru2cu6jzzrntdxomdma5sdojkq4i37jdfylidmad.onion/events").await.unwrap();
		let write_messages = {
			async {
				loop {
					write.send(Message::Text("Hello WebSocket".to_string().into())).await.unwrap();
					tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
				}
			}
		};

		let read_messages = {
			read.for_each(|message| async {
				let data = message.unwrap().into_data();
				let text = String::from_utf8(data.to_vec()).unwrap();
				println!("Received: {text}");
			})
		};

		pin_mut!(read_messages, write_messages);

		future::select(read_messages, write_messages).await;
	}
}
