use crate::hasheus_error::HasheusError;
use nostr_sdk::prelude::*;
use std::env;

const DEFAULT_FILE_RELAY_URLS: [&str; 1] = ["wss://relay.hasheus.com"];
const DEFAULT_USER_RELAY_URLS: [&str; 5] = [
    "wss://purplepag.es",
    "wss://relay.vertexlab.io",
    "wss://relay.damus.io",
    "wss://relay.primal.net",
    "wss://nos.lol",
];

pub struct RelayConfig {
    pub file_relay_urls: Vec<RelayUrl>,
    pub user_relay_urls: Vec<RelayUrl>,
}

impl RelayConfig {
    pub fn from_env() -> Result<Self, HasheusError> {
        Ok(Self {
            file_relay_urls: parse_urls(
                env::var("HASHEUS_FILE_RELAY_URLS").ok(),
                &DEFAULT_FILE_RELAY_URLS,
            )?,
            user_relay_urls: parse_urls(
                env::var("HASHEUS_USER_RELAY_URLS").ok(),
                &DEFAULT_USER_RELAY_URLS,
            )?,
        })
    }
}

fn parse_urls(value: Option<String>, defaults: &[&str]) -> Result<Vec<RelayUrl>, HasheusError> {
    let urls: Vec<&str> = value
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .collect();

    // a value without any urls (like "," or " ") would leave nothing to fetch
    // from, so it's treated as unset.
    let urls = if urls.is_empty() {
        defaults.to_vec()
    } else {
        urls
    };

    urls.into_iter()
        .map(|url| {
            RelayUrl::parse(url).map_err(|_| HasheusError::InvalidRelayURL {
                url: url.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: [&str; 2] = ["wss://a.example", "wss://b.example"];

    fn to_strings(urls: Vec<RelayUrl>) -> Vec<String> {
        urls.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn uses_the_defaults_when_unset() {
        let urls = parse_urls(None, &DEFAULTS).unwrap();

        assert_eq!(to_strings(urls), ["wss://a.example", "wss://b.example"]);
    }

    #[test]
    fn uses_the_defaults_when_blank() {
        let urls = parse_urls(Some(" ".to_string()), &DEFAULTS).unwrap();

        assert_eq!(to_strings(urls), ["wss://a.example", "wss://b.example"]);
    }

    #[test]
    fn uses_the_defaults_when_only_commas() {
        let urls = parse_urls(Some(" , ,".to_string()), &DEFAULTS).unwrap();

        assert_eq!(to_strings(urls), ["wss://a.example", "wss://b.example"]);
    }

    #[test]
    fn splits_and_trims_a_comma_separated_list() {
        let value = "ws://localhost:10547, wss://c.example,".to_string();

        let urls = parse_urls(Some(value), &DEFAULTS).unwrap();

        assert_eq!(
            to_strings(urls),
            ["ws://localhost:10547", "wss://c.example"]
        );
    }

    #[test]
    fn fails_on_an_invalid_url() {
        let error = parse_urls(Some("https://c.example".to_string()), &DEFAULTS).unwrap_err();

        assert_eq!(
            error.to_string(),
            "https://c.example isn't a valid relay url"
        );
    }
}
