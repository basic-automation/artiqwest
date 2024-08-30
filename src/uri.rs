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
	pub is_https: bool,
	pub is_local: bool,
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

	let is_https = uri.scheme() == Some(&Scheme::HTTPS) || uri.scheme_str().unwrap().contains("wss");
	let port = match uri.port_u16() {
		Some(port) => port,
		_ if is_https => 443,
		_ => 80,
	};

	let is_local = host.contains("localhost");

	Ok(Uri { full, host, port, is_https, is_local })
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_uri() {
		let uri = "http://localhost:8225/status";
		let uri = parse_uri(uri).unwrap();
		assert_eq!(uri.host, "localhost");
		assert_eq!(uri.port, 8225);
		assert_eq!(uri.is_https, false);
		assert_eq!(uri.is_local, true);

		let uri = "https://facebookwkhpilnemxj7asaniu7vnjjbiltxjqhye3mhbshg7kx5tfyd.onion";
		let uri = parse_uri(uri).unwrap();
		assert_eq!(uri.host, "facebookwkhpilnemxj7asaniu7vnjjbiltxjqhye3mhbshg7kx5tfyd.onion");
		assert_eq!(uri.port, 443);
		assert_eq!(uri.is_https, true);
		assert_eq!(uri.is_local, false);

		let uri = "http://vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion/status";
		let uri = parse_uri(uri).unwrap();
		assert_eq!(uri.host, "vpns6exmqmg5znqmgxa5c6rgzpt6imy5yzrbsoszovgfipdjypnchpyd.onion");
		assert_eq!(uri.port, 80);
		assert_eq!(uri.is_https, false);
		assert_eq!(uri.is_local, false);

		let uri = "wss://localhost:8225/events";
		let uri = parse_uri(uri).unwrap();
		assert_eq!(uri.host, "localhost");
		assert_eq!(uri.port, 8225);
		assert_eq!(uri.is_https, true);
		assert_eq!(uri.is_local, true);
	}
}
