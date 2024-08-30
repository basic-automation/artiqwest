use std::collections::HashMap;

use anyhow::{bail, Result};
use http_body_util::BodyExt;
use hyper::client::conn::http2::handshake;
use hyper::header;
use hyper::header::HeaderName;
use hyper::HeaderMap;
use hyper::Uri as HyperUri;
use hyper_util::rt::tokio::TokioExecutor;
use hyper_util::rt::TokioIo;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

use crate::Error;
use crate::Response;
use crate::UpstreamRequest;
use crate::UpstreamResponse;
use crate::Uri;

pub struct MakeRequest {
	pub uri: Uri,
	pub body: Option<String>,
	pub headers: Option<HashMap<String, String>>,
	pub method: hyper::Method,
	pub version: hyper::Version,
}

impl MakeRequest {
	#[allow(dead_code)]
	pub const fn new(uri: Uri, body: Option<String>, headers: Option<HashMap<String, String>>, method: hyper::Method, version: hyper::Version) -> Self {
		Self { uri, body, headers, method, version }
	}

	pub fn headers(&self) -> HeaderMap {
		construct_headers(&self.headers.clone().unwrap_or_default())
	}
}

pub async fn make_request(request: MakeRequest, stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static) -> Result<Response> {
	let executor = TokioExecutor::new();
	let (mut sender, connection) = match handshake(executor, TokioIo::new(stream)).await {
		Ok((sender, connection)) => (sender, connection),
		Err(e) => bail!(Error::Hyper(e)),
	};

	tokio::task::spawn(async move {
		if let Err(e) = connection.await {
			eprintln!("Error: {e:?}");
		}
	});

	let mut request_builder = hyper::Request::builder().method(request.method.clone()).uri(request.uri.to_string()).version(request.version);

	for (key, value) in request.headers() {
		request_builder = request_builder.header(key.unwrap(), value.clone());
	}

	let body = request.body.map_or_else(String::new, |body| body);

	request_builder = request_builder.header("content-length", body.len().to_string());
	request_builder = request_builder.version(request.version);
	let request_builder = match request_builder.body(body.clone()) {
		Ok(request_builder) => request_builder,
		Err(e) => bail!(Error::Http(e)),
	};

	let upstream_request = UpstreamRequest { body, headers: request_builder.headers().clone(), method: request_builder.method().clone(), uri: request_builder.uri().clone(), version: request_builder.version() };

	let response = match sender.send_request(request_builder).await {
		Ok(response) => response,
		Err(e) => bail!(Error::Hyper(e)),
	};

	let res_status = response.status();
	let res_headers = response.headers().clone();
	let res_version = response.version();
	let res_body = match response.collect().await {
		Ok(body) => body.to_bytes(),
		Err(e) => bail!(Error::Hyper(e)),
	};

	let upstream_res = UpstreamResponse { status: res_status, headers: res_headers, body: res_body, version: res_version };

	Ok(Response { request: upstream_request, response: upstream_res })
}

pub async fn make_local_request(request: MakeRequest) -> Result<Response> {
	let body = request.body.map_or_else(String::new, |body| body);
	let headers = construct_headers(&request.headers.unwrap_or_default());

	match request.method {
		hyper::Method::GET => {
			let client = reqwest::Client::new();
			let req = client.get(request.uri.to_string()).headers(headers).build()?;
			let uri: HyperUri = req.url().clone().as_str().parse()?;
			let upstream_request = UpstreamRequest { body, headers: req.headers().clone(), method: req.method().clone(), uri, version: req.version() };
			let response = client.execute(req).await?;
			let res_status = response.status();
			let res_headers = response.headers().clone();
			let res_version = response.version();
			let res_body = response.text().await?;
			let upstream_response = UpstreamResponse { status: res_status, headers: res_headers, body: res_body.into_bytes().into(), version: res_version };
			Ok(Response { request: upstream_request, response: upstream_response })
		}
		hyper::Method::POST => {
			let client = reqwest::Client::new();
			let req = client.post(request.uri.to_string()).headers(headers).body(body.clone()).build()?;
			let uri: HyperUri = req.url().clone().as_str().parse()?;
			let upstream_request = UpstreamRequest { body, headers: req.headers().clone(), method: req.method().clone(), uri, version: req.version() };
			let response = client.execute(req).await?;
			let res_status = response.status();
			let res_headers = response.headers().clone();
			let res_version = response.version();
			let res_body = response.text().await?;
			let upstream_response = UpstreamResponse { status: res_status, headers: res_headers, body: res_body.into_bytes().into(), version: res_version };
			Ok(Response { request: upstream_request, response: upstream_response })
		}
		_ => bail!(Error::Reqwest("Unsupported method".to_string())),
	}
}

fn construct_headers(headers: &HashMap<String, String>) -> HeaderMap {
	let mut header_map = HeaderMap::new();
	let key: String = websocket::header::WebSocketKey::new().serialize();

	for (key, value) in headers {
		header_map.insert(key.parse::<HeaderName>().unwrap(), value.parse().unwrap());
	}

	if header_map.contains_key(header::CONTENT_LENGTH) {
		header_map.remove(header::CONTENT_LENGTH);
	}

	if !header_map.contains_key(header::USER_AGENT) {
		header_map.insert(header::USER_AGENT, "artiqwest".parse().unwrap());
	}

	if !header_map.contains_key(header::CONTENT_TYPE) {
		header_map.insert(header::CONTENT_TYPE, "text/html; charset=utf-8".parse().unwrap());
	}

	{
		if !header_map.contains_key(header::CONNECTION) {
			header_map.insert(header::CONNECTION, "upgrade".parse().unwrap());
		}

		if !header_map.contains_key(header::UPGRADE) {
			header_map.insert(header::UPGRADE, "websocket".parse().unwrap());
		}

		if !header_map.contains_key(header::SEC_WEBSOCKET_KEY) {
			header_map.insert(header::SEC_WEBSOCKET_KEY, key.parse().unwrap());
		}

		if !header_map.contains_key(header::SEC_WEBSOCKET_VERSION) {
			header_map.insert(header::SEC_WEBSOCKET_VERSION, "13".parse().unwrap());
		}

		header_map.insert(HeaderName::from_lowercase(b"proxy-connection").unwrap(), "keep-alive".parse().unwrap());
	}

	header_map
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use async_wsocket::{ConnectionMode, Url};
	use futures_util::StreamExt;
	use futures_util::{future, pin_mut, SinkExt};
	use tokio_tungstenite::{client_async, tungstenite::protocol::Message};

	#[tokio::test]
	async fn test_web_socket() {
		println!("ws: 1");

		env_logger::init();

		let uri_str = "http://qfnztqeav3f2pysymkf2bqknxlvwgzqfknifddkpfc4ea3vtxxle7did.onion";
		let uri = crate::parse_uri(uri_str).unwrap();

		println!("uri: {uri:?}");
		let stream = crate::create_http_stream(&uri, 5).await.unwrap();

		println!("ws: 2");

		let (ws_stream, response) = match client_async(uri_str, stream).await {
			Ok((ws_stream, response)) => (ws_stream, response),
			Err(e) => {
				println!("Error: {}", e.to_string());
				return;
			}
		};

		println!("ws: 3");

		println!("Status: {:?}", response.status());

		let (mut write, read) = ws_stream.split();

		println!("ws: 4");

		let write_messages = async {
			loop {
				write.send(Message::Text("Hello WebSocket".to_string())).await.unwrap();
				tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
			}
		};

		println!("ws: 5");

		let read_messages = read.for_each(|message| async {
			match message {
				Ok(message) => {
					println!("Received: {:?}", message);
				}
				Err(e) => {
					println!("Error: {:?}", e);
				}
			}
		});

		println!("ws: 6");

		pin_mut!(read_messages, write_messages);

		println!("ws: 7");

		future::select(read_messages, write_messages).await;

		println!("ws: 8");
	}

	#[tokio::test]
	async fn test_async_websocket() {
		println!("ws: 1");
		let uri_str = "ws://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/events";
		let uri = Url::parse(uri_str).unwrap();

		println!("ws: 2");

		let (mut write, read) = async_wsocket::connect(&uri, ConnectionMode::Tor, Duration::from_secs(120)).await.unwrap();

		println!("ws: 3");

		let write_messages = async {
			loop {
				write.send(Message::Text("Hello WebSocket".to_string())).await.unwrap();
				tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
			}
		};

		println!("ws: 5");

		let read_messages = read.for_each(|message| async {
			match message {
				Ok(message) => {
					println!("Received: {:?}", message);
				}
				Err(e) => {
					println!("Error: {:?}", e);
				}
			}
		});

		println!("ws: 6");

		pin_mut!(read_messages, write_messages);

		println!("ws: 7");

		future::select(read_messages, write_messages).await;

		println!("ws: 8");
	}
}
