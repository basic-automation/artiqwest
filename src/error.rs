use hyper::http;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
	#[error("Invalid URI")]
	InvalidUri,

	#[error("Faild to connect to the Tor network: {0}")]
	Tor(#[from] arti_client::Error),

	#[error("TLS Error: {0}")]
	Tls(#[from] tokio_native_tls::native_tls::Error),

	#[error("HTTP Error: {0}")]
	Hyper(#[from] hyper::Error),

	#[error("HTTP Error: {0}")]
	Http(#[from] http::Error),

	#[error("Deserialization Error: {0}")]
	Deserialization(String),

	#[error("Reqwest Error: {0}")]
	Reqwest(String),

	#[error("Tungstenite Error: {0}")]
	Tungstenite(#[from] tokio_tungstenite::tungstenite::error::Error),

	#[error("Header Error: {0}")]
	Header(String),

	#[error("Unkown Error: {0}")]
	Unkown(String),
}
