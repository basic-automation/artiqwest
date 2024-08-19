use std::fmt::Display;

use anyhow::Result;
use hyper::StatusCode;
pub use upstream::{UpstreamRequest, UpstreamResponse};

use crate::Error;

mod upstream;

/// Response from the server with the request that was made included.
#[derive(Debug, Clone)]
pub struct Response {
	pub(crate) request: UpstreamRequest,
	pub(crate) response: UpstreamResponse,
}

impl Response {
	/// Deserialize the response body as JSON into the provided type.
	///
	/// # Example
	/// ```rust
	/// use serde::{Serialize, Deserialize};
	/// use artiqwest::post;
	///
	/// #[derive(Serialize, Deserialize)]
	/// struct MyResponse {
	///         key: String,
	/// }
	/// #[tokio::main]
	/// async fn main() -> () {
	///         let my_response = MyResponse {
	///                 key: "value".to_string(),
	///         };
	///
	///         let body = serde_json::to_string(&my_response).unwrap();
	///         let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", &body, None).await.unwrap();
	///         let response = response.from_json::<MyResponse>().unwrap();
	///         assert_eq!(response.key, "value");
	///         ()
	/// }
	/// ```
	///
	///
	/// # Errors
	/// 1. If the response body cannot be deserialized into the provided type.
	pub fn from_json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
		serde_json::from_slice(&self.response.body).map_err(|e| Error::Deserialization(e.to_string()).into())
	}

	/// Get the status code of the response.
	///
	/// An HTTP status code (status-code in RFC 7230 et al.).
	///
	/// Constants are provided for known status codes, including those in the IANA HTTP Status Code Registry.
	///
	/// Status code values in the range 100-999 (inclusive) are supported by this type. Values in the range 100-599 are semantically classified by the most significant digit. See `StatusCode::is_success`, etc. Values above 599 are unclassified but allowed for legacy compatibility, though their use is discouraged. Applications may interpret such values as protocol errors.
	pub const fn status(&self) -> StatusCode {
		self.response.status
	}

	/// Get the headers from the response.
	///
	/// A set of HTTP headers
	///
	/// `HeaderMap` is a multimap of `HeaderName` to values.
	pub const fn headers(&self) -> &hyper::HeaderMap {
		&self.response.headers
	}

	/// Get the HTTP version of the response.
	///
	/// Represents a version of the HTTP spec.
	pub const fn version(&self) -> hyper::Version {
		self.response.version
	}

	/// Deserialize the request body as JSON into the provided type.
	///
	/// # Example
	/// ```rust
	/// use serde::{Serialize, Deserialize};
	/// use artiqwest::post;
	///
	/// #[derive(Serialize, Deserialize)]
	/// struct MyResponse {
	///        key: String,
	/// }
	///
	/// #[tokio::main]
	/// async fn main() -> () {
	///         let my_response = MyResponse {
	///                 key: "value".to_string(),
	///         };
	///
	///         let body = serde_json::to_string(&my_response).unwrap();
	///         let response = post("http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/echo", &body, None).await.unwrap();
	///         let request = response.request_from_json::<MyResponse>().unwrap();
	///         assert_eq!(request.key, "value");
	///         ()
	/// }
	/// ```
	///
	/// # Errors
	/// 1. If the response body cannot be deserialized into the provided type.
	pub fn request_from_json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
		serde_json::from_str(&self.request.body).map_err(|e| Error::Deserialization(e.to_string()).into())
	}

	/// Get the request body as a string.
	pub fn request_to_string(&self) -> &str {
		&self.request.body
	}

	/// Get the headers from the request.
	///
	/// A set of HTTP headers
	///
	/// `HeaderMap` is a multimap of `HeaderName` to values.
	pub const fn request_headers(&self) -> &hyper::HeaderMap {
		&self.request.headers
	}

	/// Get the method of the request.
	///
	/// The Request Method (VERB)
	///
	/// This type also contains constants for a number of common HTTP methods such as GET, POST, etc.
	///
	/// Currently includes 8 variants representing the 8 methods defined in RFC 7230, plus PATCH, and an Extension variant for all extensions.
	pub const fn request_method(&self) -> &hyper::Method {
		&self.request.method
	}

	/// Get the URI of the request.
	///
	/// The URI component of a request.
	///
	/// For HTTP 1, this is included as part of the request line. From Section 5.3, Request Target:
	///
	/// Once an inbound connection is obtained, the client sends an HTTP request message (Section 3) with a request-target derived from the target URI. There are four distinct formats for the request-target, depending on both the method being requested and whether the request is to a proxy.
	/// ```text
	/// request-target = origin-form
	///                / absolute-form
	///                / authority-form
	///                / asterisk-form
	/// ```
	///
	/// The URI is structured as follows:
	/// ```text
	/// abc://username:password@example.com:123/path/data?key=value&key2=value2#fragid1
	/// |-|   |-------------------------------||--------| |-------------------| |-----|
	///  |                  |                       |               |              |
	/// scheme          authority                 path            query         fragment
	/// ```
	///
	/// For HTTP 2.0, the URI is encoded using pseudoheaders.
	pub const fn request_uri(&self) -> &hyper::Uri {
		&self.request.uri
	}

	/// Get the HTTP version of the request.
	///
	/// Represents a version of the HTTP spec.
	pub const fn request_version(&self) -> hyper::Version {
		self.request.version
	}
}

impl Display for Response {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let body = std::str::from_utf8(&self.response.body).unwrap_or_default();
		write!(f, "{body}")
	}
}
