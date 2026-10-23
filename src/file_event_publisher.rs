use crate::hasheus_error::HasheusError;
use crate::relay_config::RelayConfig;
use crate::relay_connector;
use crate::text_sanitizer::sanitize;
use nostr_sdk::prelude::*;
use std::time::Duration;

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

pub struct FileVerification<'a> {
    pub hash: &'a str,
    pub mime_type: &'a str,
    pub size: u64,
    pub url: &'a str,
    pub description: &'a str,
}

// the connected file relays, kept open from checking for an existing
// verification through publishing.
pub struct FileEventPublisher {
    client: Client,
    relay_urls: Vec<RelayUrl>,
}

impl FileEventPublisher {
    pub async fn connect(relay_config: &RelayConfig) -> Result<Self, HasheusError> {
        let client = Client::default();
        let relay_urls = relay_connector::connect_all(&client, &relay_config.file_relay_urls).await;

        if relay_urls.is_empty() {
            return Err(HasheusError::FileRelaysUnreachable);
        }

        Ok(Self { client, relay_urls })
    }

    // a lookup that fails counts as not verified, since publishing twice does no
    // harm: only someone's first verification of a file counts.
    pub async fn has_verified(&self, hash: &str, public_key: PublicKey) -> bool {
        let filter = Filter::new()
            .kind(Kind::FileMetadata)
            .author(public_key)
            .custom_tag(SingleLetterTag::LOWERCASE_X, hash)
            .limit(1);
        let target = ReqTarget::manual(self.relay_urls.iter().map(|url| (url, filter.clone())));

        self.client
            .fetch_events(target)
            .timeout(FETCH_TIMEOUT)
            .await
            .is_ok_and(|events| !events.is_empty())
    }

    // like the web page, it only needs to reach one file relay, and if none
    // accept it, the first relay's reason says more than a generic error.
    pub async fn publish(&self, event: &Event) -> Result<(), HasheusError> {
        let output = self
            .client
            .send_event(event)
            .to(&self.relay_urls)
            .await
            .map_err(|_| HasheusError::FileRelaysUnreachable)?;

        if !output.success.is_empty() {
            return Ok(());
        }

        let message = self
            .relay_urls
            .iter()
            .find_map(|url| output.failed.get(url))
            .map(|message| sanitize(message))
            .unwrap_or_default();

        Err(HasheusError::PublishRejected { message })
    }
}

// the same tags the web page publishes, so either one's verifications look the
// same to everyone else.
pub fn build_event(verification: &FileVerification, keys: &Keys) -> Event {
    let tags = [
        Tag::custom("x", [verification.hash]),
        Tag::custom("m", [verification.mime_type]),
        Tag::custom("size", [verification.size.to_string()]),
        Tag::custom("url", [verification.url]),
    ];

    EventBuilder::new(Kind::FileMetadata, verification.description)
        .tags(tags)
        .finalize(keys)
        .expect("signing with a parsed secret key always succeeds")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::HASH;
    use crate::test_relays::{DEAD_RELAY_URL, build_relay_config, run_relay};

    fn build_verification() -> FileVerification<'static> {
        FileVerification {
            hash: HASH,
            mime_type: "application/x-iso9660-image",
            size: 1024,
            url: "https://example.com/file.iso",
            description: "a file",
        }
    }

    fn tag_value<'a>(event: &'a Event, kind: &str) -> Option<&'a str> {
        event
            .tags
            .iter()
            .find(|tag| tag.kind() == kind)
            .and_then(|tag| tag.content())
    }

    #[test]
    fn builds_a_signed_file_event() {
        let keys = Keys::generate();

        let event = build_event(&build_verification(), &keys);

        assert_eq!(event.kind, Kind::FileMetadata);
        assert_eq!(event.pubkey, keys.public_key());
        assert_eq!(event.content, "a file");
        assert_eq!(tag_value(&event, "x"), Some(HASH));
        assert_eq!(tag_value(&event, "m"), Some("application/x-iso9660-image"));
        assert_eq!(tag_value(&event, "size"), Some("1024"));
        assert_eq!(
            tag_value(&event, "url"),
            Some("https://example.com/file.iso")
        );
        assert!(event.verify().is_ok());
    }

    #[tokio::test]
    async fn publishes_and_then_knows_its_been_verified() {
        let (_relay, url) = run_relay(&[]).await;
        let keys = Keys::generate();
        let publisher = FileEventPublisher::connect(&build_relay_config(&[&url], &[]))
            .await
            .unwrap();

        assert!(!publisher.has_verified(HASH, keys.public_key()).await);

        publisher
            .publish(&build_event(&build_verification(), &keys))
            .await
            .unwrap();

        assert!(publisher.has_verified(HASH, keys.public_key()).await);
    }

    #[tokio::test]
    async fn doesnt_count_someone_elses_verification() {
        let (_relay, url) = run_relay(&[]).await;
        let publisher = FileEventPublisher::connect(&build_relay_config(&[&url], &[]))
            .await
            .unwrap();

        publisher
            .publish(&build_event(&build_verification(), &Keys::generate()))
            .await
            .unwrap();

        assert!(
            !publisher
                .has_verified(HASH, Keys::generate().public_key())
                .await
        );
    }

    #[tokio::test]
    async fn skips_relays_that_dont_connect() {
        let (_relay, url) = run_relay(&[]).await;
        let publisher =
            FileEventPublisher::connect(&build_relay_config(&[DEAD_RELAY_URL, &url], &[]))
                .await
                .unwrap();

        let result = publisher
            .publish(&build_event(&build_verification(), &Keys::generate()))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn fails_when_no_file_relay_connects() {
        let result = FileEventPublisher::connect(&build_relay_config(&[DEAD_RELAY_URL], &[])).await;

        assert!(matches!(result, Err(HasheusError::FileRelaysUnreachable)));
    }
}
