use nostr_sdk::prelude::*;
use std::time::Duration;
use tokio::task::JoinSet;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// like the web page, an event that doesn't match what was asked for is dropped,
// so a relay can't pass off another file's verification as this file's.
pub fn build_client() -> Client {
    Client::builder().verify_subscriptions(true).build()
}

// a relay that fails to connect isn't retried, so callers only use the ones
// that connect.
pub async fn connect(client: &Client, url: &RelayUrl) -> Option<Relay> {
    client.add_relay(url).await.ok()?;
    let relay = client.relay(url).await.ok()??;
    relay.try_connect().timeout(CONNECT_TIMEOUT).await.ok()?;

    Some(relay)
}

// connects to every relay at once, so a slow one only holds up itself. returns
// the ones that connected, in the given order without repeats, so "the first
// relay" means the same thing every time.
pub async fn connect_all(client: &Client, urls: &[RelayUrl]) -> Vec<RelayUrl> {
    let mut connections = JoinSet::new();

    for url in urls {
        let (client, url) = (client.clone(), url.clone());
        connections.spawn(async move { connect(&client, &url).await.map(|_| url) });
    }

    let connected: Vec<RelayUrl> = connections.join_all().await.into_iter().flatten().collect();
    let mut connected_in_order: Vec<RelayUrl> = Vec::new();

    for url in urls {
        if connected.contains(url) && !connected_in_order.contains(url) {
            connected_in_order.push(url.clone());
        }
    }

    connected_in_order
}
