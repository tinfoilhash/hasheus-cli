use crate::relay_connector;
use nostr_sdk::prelude::*;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time;

// a name is a nicety, so whatever's waiting on it doesn't wait long: past this,
// it goes with what it has.
const PROFILE_TIMEOUT: Duration = Duration::from_secs(3);

// the newest profile of the user that the relays return in time. each relay is
// asked on its own, so one that's slow to connect doesn't hide another's answer.
pub async fn fetch(public_key: PublicKey, relay_urls: &[RelayUrl]) -> Option<Event> {
    let client = Client::default();
    let filter = Filter::new()
        .kind(Kind::Metadata)
        .author(public_key)
        .limit(1);
    let mut fetches = JoinSet::new();

    for url in relay_urls {
        let (client, url, filter) = (client.clone(), url.clone(), filter.clone());
        fetches.spawn(async move {
            let relay = relay_connector::connect(&client, &url).await?;

            // events come back newest first.
            relay
                .fetch_events(vec![filter])
                .await
                .ok()?
                .into_iter()
                .next()
        });
    }

    let deadline = time::Instant::now() + PROFILE_TIMEOUT;
    let mut newest: Option<Event> = None;

    while let Ok(Some(result)) = time::timeout_at(deadline, fetches.join_next()).await {
        let Ok(Some(profile)) = result else {
            continue;
        };

        if newest
            .as_ref()
            .is_none_or(|newest| newest.created_at < profile.created_at)
        {
            newest = Some(profile);
        }
    }

    newest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::build_profile;
    use crate::test_relays::{DEAD_RELAY_URL, relay_url, run_relay};

    #[tokio::test]
    async fn fetches_the_newest_profile() {
        let keys = Keys::generate();
        let newer = build_profile(&keys, 2, "newer");
        let (_relay, url) = run_relay(&[build_profile(&keys, 1, "older"), newer.clone()]).await;

        let profile = fetch(keys.public_key(), &[relay_url(&url)]).await;

        assert_eq!(profile, Some(newer));
    }

    #[tokio::test]
    async fn has_none_without_a_profile() {
        let (_relay, url) = run_relay(&[build_profile(&Keys::generate(), 1, "someone")]).await;

        assert_eq!(
            fetch(Keys::generate().public_key(), &[relay_url(&url)]).await,
            None
        );
    }

    #[tokio::test]
    async fn uses_the_relays_that_answer_when_another_doesnt_connect() {
        let keys = Keys::generate();
        let profile = build_profile(&keys, 1, "alice");
        let (_relay, url) = run_relay(std::slice::from_ref(&profile)).await;

        assert_eq!(
            fetch(
                keys.public_key(),
                &[relay_url(DEAD_RELAY_URL), relay_url(&url)]
            )
            .await,
            Some(profile)
        );
    }

    #[tokio::test]
    async fn has_none_when_no_relay_connects() {
        assert_eq!(
            fetch(Keys::generate().public_key(), &[relay_url(DEAD_RELAY_URL)]).await,
            None
        );
    }
}
