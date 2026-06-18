use std::sync::Arc;

use anyhow::{Result, bail};
use arti_client::TorClient;
use arti_client::config::TorClientConfigBuilder;
use tor_rtcompat::PreferredRuntime;
use tracing::{Level, event, span};

use crate::{Error, TOR_CLIENT};

/// Build the Tor client configuration.
///
/// Uses a single, stable arti-specific directory (`./tor/arti/state`,
/// `./tor/arti/cache`) rather than a fresh per-instance directory. A stable
/// directory is reused across runs — so the cache no longer grows without
/// bound — while staying isolated from any other arti instance on the machine
/// that uses the shared `TorClientConfig::default()` location.
fn tor_config() -> Result<arti_client::TorClientConfig> {
	let mut builder = TorClientConfigBuilder::from_directories("./tor/arti/state", "./tor/arti/cache");
	builder.address_filter().allow_onion_addrs(true);
	match builder.build() {
		Ok(config) => Ok(config),
		Err(e) => {
			event!(Level::ERROR, "Failed to build the tor client config: {}", e);
			bail!(Error::Unkown(e.to_string()))
		}
	}
}

pub async fn get_or_refresh() -> Result<Arc<TorClient<PreferredRuntime>>> {
	let get_or_refresh_span = span!(Level::INFO, "get_or_refresh");
	let _guard = get_or_refresh_span.enter();

	event!(Level::INFO, "Getting new the tor client");

	let t_c = TOR_CLIENT.lock().await.clone();

	let tor_client = if let Some(ref tor_client) = t_c {
		tor_client.clone()
	} else {
		let tor_client = match TorClient::create_bootstrapped(tor_config()?).await {
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
