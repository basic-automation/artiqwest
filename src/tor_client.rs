use anyhow::{Result, bail};
use arti_client::TorClient;
use tor_rtcompat::PreferredRuntime;
use tracing::{Level, event, span};

use crate::{Error, TOR_CLIENT, TOR_CONFIG};

pub async fn get_or_refresh() -> Result<TorClient<PreferredRuntime>> {
	let get_or_refresh_span = span!(Level::INFO, "get_or_refresh");
	let _guard = get_or_refresh_span.enter();

	event!(Level::INFO, "Getting new the tor client");

	let t_c = TOR_CLIENT.lock().await.clone();

	let tor_client = if let Some(ref tor_client) = t_c {
		tor_client.clone()
	} else {
		let tor_client = match TorClient::create_bootstrapped(TOR_CONFIG.clone()).await {
			Ok(tor_client) => tor_client,
			Err(e) => {
				event!(Level::ERROR, "Failed to create a tor client: {}", e);
				bail!(Error::Tor(e))
			}
		};
		*TOR_CLIENT.lock().await = Some(tor_client.clone());
		tokio::time::sleep(std::time::Duration::from_secs(5)).await;
		tor_client
	};

	Ok(tor_client)
}
