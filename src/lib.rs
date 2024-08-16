#![warn(clippy::pedantic, clippy::nursery, clippy::all, clippy::cargo)]
#![allow(clippy::multiple_crate_versions, clippy::module_name_repetitions)]

use anyhow::{bail, Result};
use arti_client::config::TorClientConfigBuilder;
use arti_client::{DataStream, TorClient, TorClientConfig};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::http;
use hyper::http::uri::Scheme;
use hyper::{Request, Uri};
use hyper_util::rt::tokio::TokioExecutor;
use hyper_util::rt::TokioIo;
use lazy_static::lazy_static;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex as TokioMutex;
use tokio_native_tls::native_tls::TlsConnector;
use tor_rtcompat::PreferredRuntime;

lazy_static! {
	static ref TOR_CONFIG: TorClientConfig = {
		let mut default_config = TorClientConfigBuilder::default();

		default_config.address_filter().allow_onion_addrs(true);

		default_config.build().unwrap()
	};
	static ref TOR_CLIENT: TokioMutex<Option<TorClient<PreferredRuntime>>> = TokioMutex::new(None);
}

#[derive(Error, Debug)]
pub enum Error {
	#[error("Invalid URI")]
	InvalidUri,

	#[error("Faild to connect to the Tor network: {0}")]
	TorError(#[from] arti_client::Error),

	#[error("TLS Error: {0}")]
	TlsError(#[from] tokio_native_tls::native_tls::Error),

	#[error("HTTP Error: {0}")]
	HyperError(#[from] hyper::Error),

	#[error("HTTP Error: {0}")]
	HttpError(#[from] http::Error),

	#[error("Unkown Error: {0}")]
	Unkown(String),
}

/// # Errors
/// todo: add errors
pub async fn get(uri: &str) -> Result<String> {
	let Ok(uri) = uri.parse::<Uri>() else { bail!(Error::InvalidUri) };
	let Some(host) = uri.host() else { bail!(Error::InvalidUri) };
	let https = uri.scheme() == Some(&Scheme::HTTPS);
	let port = match uri.port_u16() {
		Some(port) => port,
		_ if https => 443,
		_ => 80,
	};

	let t_c = TOR_CLIENT.lock().await.clone();

	let mut tor_client = if let Some(ref tor_client) = t_c {
		tor_client.clone()
	} else {
		let tor_client = match TorClient::create_bootstrapped(TOR_CONFIG.clone()).await {
			Ok(tor_client) => tor_client,
			Err(e) => bail!(Error::TorError(e)),
		};
		*TOR_CLIENT.lock().await = Some(tor_client.clone());
		tor_client
	};

	let max_attempts = 5;
	let mut attempts = 0;
	let mut stream: Option<Result<DataStream, arti_client::Error>> = None;
	let mut error: Option<arti_client::Error> = None;

	while attempts < max_attempts {
		stream = Some(tor_client.connect((host, port)).await);
		let Some(stream_ref) = stream.as_ref() else {
			attempts += 1;
			tokio::time::sleep(std::time::Duration::from_secs(5)).await;

			let tc = match TorClient::create_bootstrapped(TOR_CONFIG.clone()).await {
				Ok(tc) => tc,
				Err(e) => bail!(Error::TorError(e)),
			};
			tor_client = tc.clone();
			*TOR_CLIENT.lock().await = Some(tc.clone());

			continue;
		};
		if stream_ref.as_ref().is_ok() {
			break;
		} else if let Some(e) = stream_ref.as_ref().err() {
			error = Some(e.clone());
		}
		attempts += 1;

		if attempts == max_attempts {
			if let Some(error) = error {
				bail!(Error::TorError(error));
			}
			bail!(Error::Unkown("Maximum attempts to connect to the tor network have been reached. Please try again later.".to_string()));
		}

		tokio::time::sleep(std::time::Duration::from_secs(5)).await;

		let tc = match TorClient::create_bootstrapped(TOR_CONFIG.clone()).await {
			Ok(tc) => tc,
			Err(e) => bail!(Error::TorError(e)),
		};
		tor_client = tc.clone();
		*TOR_CLIENT.lock().await = Some(tc.clone());

		continue;
	}
	let Some(stream) = stream else { bail!(Error::Unkown("No stream found".to_string())) };
	let stream = match stream {
		Ok(stream) => stream,
		Err(e) => bail!(Error::TorError(e)),
	};

	if https {
		let alpn_protocols = vec!["h2", "http/1.1", "http/1.0"];
		let cx = match TlsConnector::builder().request_alpns(&alpn_protocols).build() {
			Ok(cx) => cx,
			Err(e) => bail!(Error::TlsError(e)),
		};
		let cx = tokio_native_tls::TlsConnector::from(cx);
		let stream = match cx.connect(host, stream).await {
			Ok(stream) => stream,
			Err(e) => bail!(Error::TlsError(e)),
		};
		make_request(&uri.to_string(), stream).await
	} else {
		make_request(&uri.to_string(), stream).await
	}
}

async fn make_request(uri: &str, stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static) -> Result<String> {
	let executor = TokioExecutor::new();
	let (mut sender, connection) = match hyper::client::conn::http2::handshake(executor, TokioIo::new(stream)).await {
		Ok((sender, connection)) => (sender, connection),
		Err(e) => bail!(Error::HyperError(e)),
	};

	tokio::task::spawn(async move {
		if let Err(e) = connection.await {
			eprintln!("Error: {e:?}");
		}
	});

	let upstream_request = match Request::builder().uri(uri).header("user-agent", "hyper-client-http2").version(hyper::Version::HTTP_2).body(Empty::<Bytes>::new()) {
		Ok(upstream_request) => upstream_request,
		Err(e) => bail!(Error::HttpError(e)),
	};
	let res = match sender.send_request(upstream_request).await {
		Ok(res) => res,
		Err(e) => bail!(Error::HyperError(e)),
	};
	let body = match res.collect().await {
		Ok(body) => body.to_bytes(),
		Err(e) => bail!(Error::HyperError(e)),
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
		assert!(body.contains("BTC.BTC"));
	}
}
