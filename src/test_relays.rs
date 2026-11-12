use crate::relay_config::RelayConfig;
use nostr_sdk::prelude::*;
use std::time::Duration;

// relays for tests: one running in the test process, seeded with events, one
// that answers with events that weren't asked for, and one that can't be
// reached.

// nothing listens on port 1, so connecting fails right away.
pub const DEAD_RELAY_URL: &str = "ws://127.0.0.1:1";

// the url comes back as a string, so it can go straight into
// build_relay_config.
pub async fn run_relay(events: &[Event]) -> (MockRelay, String) {
    let relay = MockRelay::run().await.unwrap();
    let url = relay.url().await;
    let client = Client::default();
    client.add_relay(&url).await.unwrap();
    client
        .try_connect_relay(&url, Duration::from_secs(3))
        .await
        .unwrap();

    for event in events {
        client.send_event(event).await.unwrap();
    }

    (relay, url.to_string())
}

// a relay that answers every request with events that don't match it.
pub async fn run_mismatching_relay() -> (MockRelay, String) {
    let relay = MockRelay::run_with_opts(LocalRelayTestOptions {
        unresponsive_connection: None,
        send_random_events: true,
    })
    .await
    .unwrap();
    let url = relay.url().await;

    (relay, url.to_string())
}

pub fn relay_url(url: &str) -> RelayUrl {
    RelayUrl::parse(url).unwrap()
}

pub fn build_relay_config(file_relay_urls: &[&str], user_relay_urls: &[&str]) -> RelayConfig {
    RelayConfig {
        file_relay_urls: file_relay_urls.iter().map(|url| relay_url(url)).collect(),
        user_relay_urls: user_relay_urls.iter().map(|url| relay_url(url)).collect(),
    }
}
