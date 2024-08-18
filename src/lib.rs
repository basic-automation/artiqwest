#![warn(clippy::pedantic, clippy::nursery, clippy::all, clippy::cargo)]
#![allow(clippy::multiple_crate_versions, clippy::module_name_repetitions)]

use std::collections::HashMap;

use anyhow::{bail, Result};
use arti_client::config::TorClientConfigBuilder;
use arti_client::{TorClient, TorClientConfig};
use bytes::Bytes;
use error::Error;
use http_body_util::{BodyExt, Empty};
use hyper::Request;
use hyper_util::rt::tokio::TokioExecutor;
use hyper_util::rt::TokioIo;
use lazy_static::lazy_static;
use streams::{create_http_stream, https_upgrade};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex as TokioMutex;
use tor_client::get_or_refresh;
use tor_rtcompat::PreferredRuntime;
use uri::parse_uri;
pub use uri::Uri;

mod error;
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

/// # Errors
/// todo: add errors
pub async fn get(uri: &str) -> Result<String> {
	let uri = parse_uri(uri)?;
	let stream = create_http_stream(&uri, 5).await?;

	if uri.is_https {
		let stream = https_upgrade(&uri, stream).await?;
		make_get_request(&uri.to_string(), stream).await
	} else {
		make_get_request(&uri.to_string(), stream).await
	}
}

/// # Errors
/// todo: add errors
pub async fn post(uri: &str, body: &str, headers: Option<Vec<(&str, &str)>>) -> Result<String> {
	let uri = parse_uri(uri)?;
	let stream = create_http_stream(&uri, 5).await?;

	let headers = headers.unwrap_or_default();
	let headers: HashMap<_, _> = headers.into_iter().collect();

	if uri.is_https {
		let stream = https_upgrade(&uri, stream).await?;
		make_post_request(&uri.to_string(), body, headers, stream).await
	} else {
		make_post_request(&uri.to_string(), body, headers, stream).await
	}
}

async fn make_get_request(uri: &str, stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static) -> Result<String> {
	let executor = TokioExecutor::new();
	let (mut sender, connection) = match hyper::client::conn::http2::handshake(executor, TokioIo::new(stream)).await {
		Ok((sender, connection)) => (sender, connection),
		Err(e) => bail!(Error::Hyper(e)),
	};

	tokio::task::spawn(async move {
		if let Err(e) = connection.await {
			eprintln!("Error: {e:?}");
		}
	});

	let upstream_request = match Request::builder().method("GET").uri(uri).header("user-agent", "artiqwest").version(hyper::Version::HTTP_2).body(Empty::<Bytes>::new()) {
		Ok(upstream_request) => upstream_request,
		Err(e) => bail!(Error::Http(e)),
	};
	let res = match sender.send_request(upstream_request).await {
		Ok(res) => res,
		Err(e) => bail!(Error::Hyper(e)),
	};
	let body = match res.collect().await {
		Ok(body) => body.to_bytes(),
		Err(e) => bail!(Error::Hyper(e)),
	};
	Ok(String::from_utf8_lossy(&body).to_string())
}

async fn make_post_request(uri: &str, body: &str, mut headers: HashMap<&str, &str>, stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static) -> Result<String> {
	let executor = TokioExecutor::new();
	let (mut sender, connection) = match hyper::client::conn::http2::handshake(executor, TokioIo::new(stream)).await {
		Ok((sender, connection)) => (sender, connection),
		Err(e) => bail!(Error::Hyper(e)),
	};

	tokio::task::spawn(async move {
		if let Err(e) = connection.await {
			eprintln!("Error: {e:?}");
		}
	});

	if !headers.contains_key("user-agent") {
		headers.insert("user-agent", "artiqwest");
	}

	if headers.contains_key("content-length") {
		headers.remove("content-length");
	}

	if !headers.contains_key("content-type") {
		headers.remove("content-type");
	}

	let mut request_builder = Request::builder().method("POST").uri(uri);
	for (key, value) in headers {
		request_builder = request_builder.header(key, value);
	}

	request_builder = request_builder.header("content-length", body.len().to_string());
	request_builder = request_builder.header("content-type", "text/html; charset=utf-8");
	request_builder = request_builder.version(hyper::Version::HTTP_2);
	let request_builder = request_builder.body(body.to_string());

	let upstream_request = match request_builder {
		Ok(upstream_request) => upstream_request,
		Err(e) => bail!(Error::Http(e)),
	};
	let res = match sender.send_request(upstream_request).await {
		Ok(res) => res,
		Err(e) => bail!(Error::Hyper(e)),
	};
	let body = match res.collect().await {
		Ok(body) => body.to_bytes(),
		Err(e) => bail!(Error::Hyper(e)),
	};
	Ok(String::from_utf8_lossy(&body).to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_get() {
		let body = get("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/status").await.unwrap();
		println!("body: {}", body);
		assert!(body.contains("ok"));

		let body = get("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/product/BTC.BTC").await.unwrap();
		println!("body: {}", body);

		let body = get("https://www.facebookwkhpilnemxj7asaniu7vnjjbiltxjqhye3mhbshg7kx5tfyd.onion").await.unwrap();
		println!("body: {}", body);
		assert!(body.contains("facebook"));
	}

	#[tokio::test]
	async fn test_post() {
		let post_body = r#"{"username":"admin","password":"admin"}"#;
		let body = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", post_body, None).await.unwrap();
		println!("body: {}", body);
		assert!(body.contains("admin"));
	}
}
