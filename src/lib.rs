#![warn(clippy::pedantic, clippy::nursery, clippy::all, clippy::cargo)]
#![allow(clippy::multiple_crate_versions, clippy::module_name_repetitions)]

//! # Artiquest
//! Artiquest is a simple HTTP client that routes all requests through the Tor network using the `arti_client` and `hyper`.
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

use anyhow::Result;
use arti_client::config::TorClientConfigBuilder;
use arti_client::{TorClient, TorClientConfig};
use error::Error;
use lazy_static::lazy_static;
use make_request::make_request;
use make_request::MakeRequest;
pub use response::Response;
pub(crate) use response::{UpstreamRequest, UpstreamResponse};
use streams::{create_http_stream, https_upgrade};
use tokio::sync::Mutex as TokioMutex;
use tor_client::get_or_refresh;
use tor_rtcompat::PreferredRuntime;
use uri::parse_uri;
use uri::Uri;

mod error;
mod make_request;
mod response;
mod streams;
mod tor_client;
mod uri;

lazy_static! {
	static ref TOR_CONFIG: TorClientConfig = {
		let mut default_config = TorClientConfigBuilder::default();
		default_config.address_filter().allow_onion_addrs(true);
		default_config.build().unwrap()
	};
	static ref TOR_CLIENT: TokioMutex<Option<TorClient<PreferredRuntime>>> = TokioMutex::new(None);
}

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
	let uri = parse_uri(uri)?;
	let stream = create_http_stream(&uri, 5).await?;

	let m_r = MakeRequest { uri: uri.clone(), headers: Option::default(), body: Option::default(), method: hyper::Method::GET, version: hyper::Version::HTTP_2 };

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
	let uri = parse_uri(uri)?;
	let stream = create_http_stream(&uri, 5).await?;

	let headers = headers.unwrap_or_default();
	let headers: HashMap<_, _> = headers.into_iter().collect();

	let m_r = MakeRequest { uri: uri.clone(), headers: Some(headers), body: Some(body.to_string()), method: hyper::Method::POST, version: hyper::Version::HTTP_2 };

	if uri.is_https {
		let stream = https_upgrade(&uri, stream).await?;
		make_request(m_r, stream).await
	} else {
		make_request(m_r, stream).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_get() {
		let response = get("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/status").await.unwrap();
		println!("body: {}", response);
		assert!(response.to_string().contains("ok"));

		let response = get("https://www.facebookwkhpilnemxj7asaniu7vnjjbiltxjqhye3mhbshg7kx5tfyd.onion").await.unwrap();
		println!("body: {}", response);
		if !response.to_string().is_empty() {
			assert!(response.to_string().contains("facebook"));
		}
	}

	#[tokio::test]
	async fn test_post() {
		let post_body = r#"{"test":"testing"}"#;
		let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", post_body, None).await.unwrap();
		println!("body: {}", response);
		assert!(response.to_string().contains("test"));

		let post_body = r#"{"test":"test"}"#;
		let body = post("https://echo.free.beeceptor.com", post_body, None).await.unwrap();
		println!("body: {}", body);
		assert!(body.to_string().contains("test"));
	}
}
