use nostr_sdk::prelude::*;

// builds signed nostr events for tests, like the web page's event_builders.

pub const HASH: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

pub fn build_event(
    keys: &Keys,
    kind: Kind,
    created_at: u64,
    content: &str,
    tags: Vec<Tag>,
) -> Event {
    EventBuilder::new(kind, content)
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .finalize(keys)
        .unwrap()
}

pub fn build_file_event(keys: &Keys, created_at: u64, url: Option<&str>) -> Event {
    let mut tags = vec![Tag::parse(["x", HASH]).unwrap()];
    tags.extend(url.map(|url| Tag::parse(["url", url]).unwrap()));

    build_event(keys, Kind::FileMetadata, created_at, "description", tags)
}

pub fn build_profile(keys: &Keys, created_at: u64, name: &str) -> Event {
    let content = format!(r#"{{"name":"{name}"}}"#);

    build_event(keys, Kind::Metadata, created_at, &content, Vec::new())
}

pub fn build_follow_list(keys: &Keys, created_at: u64, follows: &[PublicKey]) -> Event {
    let tags = follows.iter().copied().map(Tag::public_key).collect();

    build_event(keys, Kind::ContactList, created_at, "", tags)
}
