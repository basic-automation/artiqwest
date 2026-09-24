use std::collections::HashMap;

use bytes::Bytes;
use serde::Serialize;
use serde::ser::SerializeStruct;

#[derive(Debug, Clone)]
pub struct UpstreamResponse {
	pub status: hyper::StatusCode,
	pub headers: hyper::HeaderMap,
	pub body: Bytes,
	pub version: hyper::Version,
}

impl Serialize for UpstreamResponse {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		let mut state = serializer.serialize_struct("UpstreamResponse", 4)?;
		state.serialize_field("status", &self.status.as_str())?;
		let headers: HashMap<String, String> = self.headers.iter().map(|(key, value)| (key.as_str().to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned())).collect();
		state.serialize_field("headers", &headers)?;
		let body = String::from_utf8_lossy(&self.body).to_string();
		state.serialize_field("body", &body)?;
		let version = format!("{:?}", self.version);
		state.serialize_field("version", &version)?;
		state.end()
	}
}

#[derive(Debug, Clone)]
pub struct UpstreamRequest {
	pub body: String,
	pub headers: hyper::HeaderMap,
	pub method: hyper::Method,
	pub uri: hyper::Uri,
	pub version: hyper::Version,
}

impl Serialize for UpstreamRequest {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		let mut state = serializer.serialize_struct("UpstreamRequest", 5)?;
		state.serialize_field("body", &self.body)?;
		let headers: HashMap<String, String> = self.headers.iter().map(|(key, value)| (key.as_str().to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned())).collect();
		state.serialize_field("headers", &headers)?;
		state.serialize_field("method", &self.method.as_str())?;
		state.serialize_field("uri", &self.uri.to_string())?;
		let version = format!("{:?}", self.version);
		state.serialize_field("version", &version)?;
		state.end()
	}
}

#[cfg(test)]
mod tests {
	use hyper::header::{HeaderMap, HeaderName, HeaderValue};

	use super::*;

	/// A header value made of opaque octets: legal on the wire (RFC 9110 obs-text)
	/// but not valid UTF-8, so `HeaderValue::to_str` refuses it. Serializing a
	/// response carrying one must not panic.
	fn non_utf8_headers() -> HeaderMap {
		let mut headers = HeaderMap::new();
		headers.insert(HeaderName::from_static("x-opaque"), HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap());
		headers
	}

	#[test]
	fn response_with_non_utf8_header_serializes_without_panicking() {
		let response = UpstreamResponse { status: hyper::StatusCode::OK, headers: non_utf8_headers(), body: Bytes::from_static(b"hi"), version: hyper::Version::HTTP_11 };

		let json = serde_json::to_value(&response).unwrap();
		assert_eq!(json["headers"]["x-opaque"], "\u{fffd}\u{fffd}");
		assert_eq!(json["body"], "hi");
	}

	#[test]
	fn request_with_non_utf8_header_serializes_without_panicking() {
		let request = UpstreamRequest { body: "hi".to_string(), headers: non_utf8_headers(), method: hyper::Method::GET, uri: "http://example.com/".parse().unwrap(), version: hyper::Version::HTTP_11 };

		let json = serde_json::to_value(&request).unwrap();
		assert_eq!(json["headers"]["x-opaque"], "\u{fffd}\u{fffd}");
		assert_eq!(json["method"], "GET");
	}
}
