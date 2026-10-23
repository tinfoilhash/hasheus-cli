use nostr_sdk::prelude::*;

// anyone can publish this event, so only web urls are shown, matching the web
// page.
pub fn url(event: &Event) -> Option<Url> {
    let tag = event.tags.iter().find(|tag| tag.kind() == "url")?;
    let url = Url::parse(tag.content()?).ok()?;

    matches!(url.scheme(), "https" | "http").then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::build_file_event;

    fn build_event(url: Option<&str>) -> Event {
        build_file_event(&Keys::generate(), 1, url)
    }

    #[test]
    fn returns_a_web_url() {
        let url = url(&build_event(Some("https://example.com/file.iso"))).unwrap();

        assert_eq!(url.as_str(), "https://example.com/file.iso");
    }

    #[test]
    fn ignores_a_url_that_isnt_http() {
        assert_eq!(url(&build_event(Some("javascript:alert(1)"))), None);
    }

    #[test]
    fn ignores_an_unparsable_url() {
        assert_eq!(url(&build_event(Some("not a url"))), None);
    }

    #[test]
    fn returns_none_without_a_url_tag() {
        assert_eq!(url(&build_event(None)), None);
    }
}
