use std::collections::HashMap;

use anyhow::{Result, bail};
use http_body_util::BodyExt;
use hyper::HeaderMap;
use hyper::Uri as HyperUri;
use hyper::client::conn::http2::handshake;
use hyper::header;
use hyper::header::HeaderName;
use hyper::header::HeaderValue;
use hyper_util::rt::TokioIo;
use hyper_util::rt::tokio::TokioExecutor;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tracing::{Level, event, span};

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
	pub fn headers(&self) -> Result<HeaderMap> {
		let make_request_headers_span = span!(Level::INFO, "artiqwest::MakeRequest::headers");
		let _guard = make_request_headers_span.enter();
		match construct_headers(&self.headers.clone().unwrap_or_default()) {
			Ok(headers) => Ok(headers),
			Err(e) => {
				event!(Level::ERROR, "Failed to construct headers: {}", e);
				bail!(e)
			}
		}
	}
}

pub async fn make_request(request: MakeRequest, stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static) -> Result<Response> {
	let make_request_span = span!(Level::INFO, "make_request");
	let _guard = make_request_span.enter();

	let executor = TokioExecutor::new();
	let (mut sender, connection) = match handshake(executor, TokioIo::new(stream)).await {
		Ok((sender, connection)) => (sender, connection),
		Err(e) => {
			event!(Level::ERROR, "Failed to handshake with the server: {}", e);
			bail!(Error::Hyper(e))
		}
	};

	tokio::task::spawn(async move {
		if let Err(e) = connection.await {
			event!(Level::ERROR, "Failed to establish connection: {}", e);
		}
	});

	let mut request_builder = hyper::Request::builder().method(request.method.clone()).uri(request.uri.to_string()).version(request.version);

	let request_headers = match request.headers() {
		Ok(headers) => headers,
		Err(e) => {
			event!(Level::ERROR, "Failed to get headers: {}", e);
			bail!(e)
		}
	};

	for (key, value) in request_headers {
		request_builder = request_builder.header(key.unwrap(), value.clone());
	}

	let body = request.body.unwrap_or_default();

	request_builder = request_builder.header("content-length", body.len().to_string());
	request_builder = request_builder.version(request.version);
	let request_builder = match request_builder.body(body.clone()) {
		Ok(request_builder) => request_builder,
		Err(e) => {
			event!(Level::ERROR, "Failed to set body: {}", e);
			bail!(Error::Http(e))
		}
	};

	let upstream_request = UpstreamRequest { body, headers: request_builder.headers().clone(), method: request_builder.method().clone(), uri: request_builder.uri().clone(), version: request_builder.version() };

	let response = match sender.send_request(request_builder).await {
		Ok(response) => response,
		Err(e) => {
			event!(Level::ERROR, "Failed to send request: {}", e);
			bail!(Error::Hyper(e))
		}
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
	let make_local_request_span = span!(Level::INFO, "make_local_request");
	let _guard = make_local_request_span.enter();

	let body = request.body.unwrap_or_default();
	let headers = match construct_headers(&request.headers.unwrap_or_default()) {
		Ok(headers) => headers,
		Err(e) => {
			event!(Level::ERROR, "Failed to construct headers: {}", e);
			bail!(e)
		}
	};

	match request.method {
		hyper::Method::GET => {
			event!(Level::INFO, "Making a GET request to {}", request.uri.to_string());
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
			event!(Level::INFO, "Making a POST request to {}", request.uri.to_string());
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
		_ => {
			event!(Level::ERROR, "Unsupported method");
			bail!(Error::Reqwest("Unsupported method".to_string()))
		}
	}
}

fn construct_headers(headers: &HashMap<String, String>) -> Result<HeaderMap> {
	let construct_headers_span = span!(Level::INFO, "artiqwest::construct_headers");
	let _guard = construct_headers_span.enter();

	let mut header_map = HeaderMap::new();

	for (key, value) in headers {
		let key: HeaderName = match key.parse() {
			Ok(key) => key,
			Err(e) => {
				event!(Level::ERROR, "Invalid header key: {}", e);
				bail!(Error::Header(e.to_string()))
			}
		};

		let value: HeaderValue = match value.parse() {
			Ok(value) => value,
			Err(e) => {
				event!(Level::ERROR, "Invalid header value: {}", e);
				bail!(Error::Header(e.to_string()))
			}
		};

		header_map.insert(key, value);
	}

	if header_map.contains_key(header::CONTENT_LENGTH) {
		header_map.remove(header::CONTENT_LENGTH);
	}

	if !header_map.contains_key(header::USER_AGENT) {
		let value: HeaderValue = match "artiqwest".parse() {
			Ok(value) => value,
			Err(e) => {
				event!(Level::ERROR, "Invalid header value: {}", e);
				bail!(Error::Header(e.to_string()))
			}
		};
		header_map.insert(header::USER_AGENT, value);
	}

	if !header_map.contains_key(header::CONTENT_TYPE) {
		let value: HeaderValue = match "text/html; charset=utf-8".parse() {
			Ok(value) => value,
			Err(e) => {
				event!(Level::ERROR, "Invalid header value: {}", e);
				bail!(Error::Header(e.to_string()))
			}
		};
		header_map.insert(header::CONTENT_TYPE, value);
	}

	if !header_map.contains_key(header::UPGRADE) {
		let value: HeaderValue = match "HTTP/2.0".parse() {
			Ok(value) => value,
			Err(e) => {
				event!(Level::ERROR, "Invalid header value: {}", e);
				bail!(Error::Header(e.to_string()))
			}
		};
		header_map.insert(header::UPGRADE, value);
	}

	Ok(header_map)
}
