use std::collections::HashMap;

use anyhow::{bail, Result};
use http_body_util::BodyExt;
use hyper::client::conn::http2::handshake;
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

pub struct MakeRequest<'a> {
	pub uri: Uri,
	pub body: Option<String>,
	pub headers: Option<HashMap<&'a str, &'a str>>,
	pub method: hyper::Method,
	pub version: hyper::Version,
}

pub async fn make_request(mut request: MakeRequest<'_>, stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static) -> Result<Response> {
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

	request.headers = Some(construct_headers(request.headers.unwrap_or_default()));

	let mut request_builder = hyper::Request::builder().method(request.method).uri(request.uri.to_string()).version(request.version);

	if let Some(headers) = request.headers {
		for (key, value) in &headers {
			request_builder = request_builder.header(*key, *value);
		}
	}

	let body = request.body.map_or_else(String::new, |body| body);

	request_builder = request_builder.header("content-length", body.len().to_string());
	request_builder = request_builder.version(hyper::Version::HTTP_2);
	let request_builder = match request_builder.body(body.clone()) {
		Ok(upstream_request) => upstream_request,
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

pub async fn make_local_request(request: MakeRequest<'_>) -> Result<Response> {
	let body = request.body.map_or_else(String::new, |body| body);
	let headers = construct_headers(request.headers.unwrap_or_default());
	let headers: HashMap<String, String> = headers.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
	let headers: HeaderMap = HeaderMap::try_from(&headers)?;

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

fn construct_headers<'a>(mut headers: HashMap<&'a str, &'a str>) -> HashMap<&'a str, &'a str> {
	if !headers.contains_key("user-agent") {
		headers.insert("user-agent", "artiqwest");
	}

	if headers.contains_key("content-length") {
		headers.remove("content-length");
	}

	if !headers.contains_key("content-type") {
		headers.insert("content-type", "text/html; charset=utf-8");
	}

	headers
}
