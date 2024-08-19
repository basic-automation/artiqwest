use anyhow::{bail, Result};
use arti_client::DataStream;
use tokio_native_tls::{native_tls::TlsConnector, TlsStream};

use crate::error::Error;
use crate::get_or_refresh;
use crate::uri::Uri;
use crate::TOR_CLIENT;

pub async fn create_http_stream(uri: &Uri, max_attempts: u32) -> Result<DataStream> {
	let mut attempts = 0;
	let mut stream: Option<Result<DataStream, arti_client::Error>> = None;
	let mut error: Option<arti_client::Error> = None;
	let mut tor_client = get_or_refresh().await?;

	while attempts < max_attempts {
		stream = Some(tor_client.connect((uri.host.clone(), uri.port)).await);

		let Some(stream_ref) = stream.as_ref() else {
			attempts += 1;
			*TOR_CLIENT.lock().await = None;
			tor_client = get_or_refresh().await?;
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
				bail!(Error::Tor(error));
			}
			bail!(Error::Unkown("Maximum attempts to connect to the tor network have been reached. Please try again later.".to_string()));
		}

		*TOR_CLIENT.lock().await = None;
		tor_client = get_or_refresh().await?;
		continue;
	}

	let Some(stream) = stream else { bail!(Error::Unkown("No stream found".to_string())) };
	let stream = match stream {
		Ok(stream) => stream,
		Err(e) => bail!(Error::Tor(e)),
	};

	Ok(stream)
}

pub async fn https_upgrade(uri: &Uri, stream: DataStream) -> Result<TlsStream<DataStream>> {
	let alpn_protocols = vec!["h2", "http/1.1", "http/1.0"];
	let cx = match TlsConnector::builder().request_alpns(&alpn_protocols).build() {
		Ok(cx) => cx,
		Err(e) => bail!(Error::Tls(e)),
	};
	let cx = tokio_native_tls::TlsConnector::from(cx);
	let stream = match cx.connect(&uri.host, stream).await {
		Ok(stream) => stream,
		Err(e) => bail!(Error::Tls(e)),
	};

	Ok(stream)
}