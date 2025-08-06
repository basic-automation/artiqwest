use std::fmt::Display;
use std::net::IpAddr;

use anyhow::{Result, bail};
use hyper::Uri as HyperUri;
use hyper::http::uri::Scheme;
use tracing::{Level, event, span};

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
	let parse_uri_span = span!(Level::INFO, "parse_uri");
	let _guard = parse_uri_span.enter();

	event!(Level::INFO, "Parsing URI: {}", uri);

	let uri = match uri.parse::<HyperUri>() {
		Ok(uri) => uri,
		Err(err) => {
			event!(Level::ERROR, "Invalid URI: {}", err);
			bail!(Error::InvalidUri)
		}
	};

	let full = uri.to_string();

	let Some(host) = uri.host() else {
		event!(Level::ERROR, "Invalid URI: no host found.");
		bail!(Error::InvalidUri)
	};

	let host = host.to_string();

	let is_https = uri.scheme() == Some(&Scheme::HTTPS) || uri.to_string().contains("wss://");
	let port = match uri.port_u16() {
		Some(port) => port,
		_ if is_https => 443,
		_ => 80,
	};

	let is_local = is_local(&host);

	event!(Level::INFO, "Successfully parsed URI: {}", full);
	Ok(Uri { full, host, port, is_https, is_local })
}

pub fn is_local(host: &str) -> bool {
	let host_lower = host.to_lowercase();

	if host_lower == "localhost" {
		return true;
	}

	if let Ok(ip) = host_lower.parse::<IpAddr>() {
		if ip.is_loopback() {
			return true;
		}
	} else if host_lower.starts_with('[') && host_lower.ends_with(']') {
		if let Ok(ip) = (&host_lower[1..host_lower.len() - 1]).parse::<IpAddr>() {
			if ip.is_loopback() {
				return true;
			}
		}
	}

	false
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

	#[test]
	fn test_is_local() {
		// Loopback hostnames
		assert!(is_local("localhost"));
		assert!(is_local("LOCALHOST")); // Case insensitive

		// IPv4 loopback addresses
		assert!(is_local("127.0.0.1"));
		assert!(is_local("127.1.2.3")); // Any 127.x.x.x

		// IPv6 loopback addresses
		assert!(is_local("::1"));
		assert!(is_local("[::1]"));

		// Non-local addresses
		assert!(!is_local("example.com"));
		assert!(!is_local("localhost.com")); // Not exact match
		assert!(!is_local("192.168.1.1")); // Private, but not loopback
		assert!(!is_local("8.8.8.8")); // Public
		assert!(!is_local("[2001:db8::1]")); // Non-loopback IPv6
		assert!(!is_local("invalid"));
	}
}
