use anyhow::{Result, bail};
use arti_client::DataStream;
use arti_client::TorClient;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio_native_tls::{TlsStream, native_tls::TlsConnector};
use tor_rtcompat::PreferredRuntime;
use tracing::{Level, event, span};

use crate::TOR_CLIENT;
use crate::error::Error;
use crate::get_or_refresh;
use crate::uri::Uri;

pub async fn create_http_stream(uri: &Uri, max_attempts: u32, tor_client: Option<TorClient<PreferredRuntime>>) -> Result<DataStream> {
	let create_http_stream_span = span!(Level::INFO, "create_http_stream");
	let _guard = create_http_stream_span.enter();

	let mut attempts = 0;
	let mut stream: Option<Result<DataStream, arti_client::Error>> = None;
	let mut error: Option<arti_client::Error> = None;

	let mut tor_client = match tor_client {
		Some(client) => client,
		None => match get_or_refresh().await {
			Ok(tor_client) => tor_client,
			Err(e) => {
				event!(Level::ERROR, "Failed to get new the tor client: {}", e);
				bail!(e)
			}
		},
	};

	while attempts < max_attempts {
		stream = Some(tor_client.connect((uri.host.clone(), uri.port)).await);

		let Some(stream_ref) = stream.as_ref() else {
			attempts += 1;
			*TOR_CLIENT.lock().await = None;
			tor_client = match get_or_refresh().await {
				Ok(tor_client) => tor_client,
				Err(e) => {
					event!(Level::ERROR, "Failed to get new the tor client: {}", e);
					bail!(e)
				}
			};
			continue;
		};

		if stream_ref.as_ref().is_ok() {
			break;
		} else if let Some(e) = stream_ref.as_ref().err() {
			error = Some(e.clone());
		}
		attempts += 1;

		if attempts == max_attempts {
			if let Some(error) = error {
				event!(Level::ERROR, "Failed to connect to the tor network: {}", error);
				bail!(Error::Tor(error));
			}

			event!(Level::ERROR, "Maximum attempts to connect to the tor network have been reached. Please try again later.");
			bail!(Error::Unkown("Maximum attempts to connect to the tor network have been reached. Please try again later.".to_string()));
		}

		*TOR_CLIENT.lock().await = None;
		tor_client = match get_or_refresh().await {
			Ok(tor_client) => tor_client,
			Err(e) => {
				event!(Level::ERROR, "Failed to get new the tor client: {}", e);
				bail!(e)
			}
		};
	}

	let Some(stream) = stream else {
		event!(Level::ERROR, "No stream found");
		bail!(Error::Unkown("No stream found".to_string()))
	};
	let stream = match stream {
		Ok(stream) => stream,
		Err(e) => {
			event!(Level::ERROR, "Failed to connect to the tor network: {}", e);
			bail!(Error::Tor(e))
		}
	};

	Ok(stream)
}

pub async fn https_upgrade<S>(uri: &Uri, stream: S, alpn: &[&str]) -> Result<TlsStream<S>>
where
	S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
	let https_upgrade_span = span!(Level::INFO, "https_upgrade");
	let _guard = https_upgrade_span.enter();

	event!(Level::INFO, "Upgrading the stream to HTTPS with ALPN: {:?}", alpn);

	let cx = match TlsConnector::builder().request_alpns(alpn).danger_accept_invalid_certs(true).build() {
		Ok(cx) => cx,
		Err(e) => {
			event!(Level::ERROR, "Failed to create a TLS connector: {}", e);
			bail!(Error::Tls(e))
		}
	};
	let cx = tokio_native_tls::TlsConnector::from(cx);
	let stream = match cx.connect(&uri.host, stream).await {
		Ok(stream) => stream,
		Err(e) => {
			event!(Level::ERROR, "Failed to connect to the server: {}", e);
			bail!(Error::Tls(e))
		}
	};

	Ok(stream)
}
