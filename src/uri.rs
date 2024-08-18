use std::fmt::Display;

use anyhow::{bail, Result};
use hyper::http::uri::Scheme;
use hyper::Uri as HyperUri;

use crate::error::Error;

#[derive(Debug, Clone)]
pub struct Uri {
	pub full: String,
	pub host: String,
	pub port: u16,
	pub path: String,
	pub is_https: bool,
	pub query: Option<String>,
}

impl Display for Uri {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.full)
	}
}

pub fn parse_uri(uri: &str) -> Result<Uri> {
	let Ok(uri) = uri.parse::<HyperUri>() else { bail!(Error::InvalidUri) };
	let full = uri.to_string();

	let Some(host) = uri.host() else { bail!(Error::InvalidUri) };
	let host = host.to_string();

	let is_https = uri.scheme() == Some(&Scheme::HTTPS);
	let port = match uri.port_u16() {
		Some(port) => port,
		_ if is_https => 443,
		_ => 80,
	};
	let path = uri.path().to_string();
	let query = uri.query().map(std::string::ToString::to_string);

	Ok(Uri { full, host, port, path, is_https, query })
}
