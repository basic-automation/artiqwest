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
		let headers: HashMap<String, String> = self.headers.iter().map(|(key, value)| (key.as_str().to_string(), value.to_str().unwrap().to_string())).collect();
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
		let headers: HashMap<String, String> = self.headers.iter().map(|(key, value)| (key.as_str().to_string(), value.to_str().unwrap().to_string())).collect();
		state.serialize_field("headers", &headers)?;
		state.serialize_field("method", &self.method.as_str())?;
		state.serialize_field("uri", &self.uri.to_string())?;
		let version = format!("{:?}", self.version);
		state.serialize_field("version", &version)?;
		state.end()
	}
}
