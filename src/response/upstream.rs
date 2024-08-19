use bytes::Bytes;

#[derive(Debug, Clone)]
pub struct UpstreamResponse {
	pub status: hyper::StatusCode,
	pub headers: hyper::HeaderMap,
	pub body: Bytes,
	pub version: hyper::Version,
}

#[derive(Debug, Clone)]
pub struct UpstreamRequest {
	pub body: String,
	pub headers: hyper::HeaderMap,
	pub method: hyper::Method,
	pub uri: hyper::Uri,
	pub version: hyper::Version,
}
