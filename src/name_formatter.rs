use crate::text_sanitizer::sanitize;
use nostr_sdk::prelude::*;

const TRUNCATED_NPUB_SIDE_LENGTH: usize = 8;

// the display name, then the name, skipping blank ones, as published. each
// output picks from them, since one could be blank once it's sanitized.
pub fn profile_names(profile: &Metadata) -> impl Iterator<Item = &str> {
    [&profile.display_name, &profile.name]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .filter(|name| !name.trim().is_empty())
}

// the first profile name that's still there once it's sanitized for a terminal.
pub fn format_profile_name(profile: &Metadata) -> Option<String> {
    profile_names(profile)
        .map(|name| sanitize(name).trim().to_string())
        .find(|name| !name.is_empty())
}

// shortened like the web page shows it: the first and last 8 characters.
pub fn truncate_npub(public_key: &PublicKey) -> String {
    let Ok(npub) = public_key.to_bech32();
    let end = npub.len() - TRUNCATED_NPUB_SIDE_LENGTH;

    format!("{}…{}", &npub[..TRUNCATED_NPUB_SIDE_LENGTH], &npub[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_profile(content: &str) -> Metadata {
        Metadata::from_json(content).unwrap()
    }

    #[test]
    fn lists_the_display_name_before_the_name() {
        let profile = parse_profile(r#"{"display_name":"Alice","name":"alice"}"#);

        assert_eq!(
            profile_names(&profile).collect::<Vec<_>>(),
            ["Alice", "alice"]
        );
    }

    #[test]
    fn skips_a_blank_display_name() {
        let profile = parse_profile(r#"{"display_name":"  ","name":"alice"}"#);

        assert_eq!(profile_names(&profile).collect::<Vec<_>>(), ["alice"]);
    }

    #[test]
    fn has_no_names_without_any() {
        assert_eq!(profile_names(&parse_profile("{}")).next(), None);
    }

    #[test]
    fn sanitizes_and_trims_the_name() {
        let profile = parse_profile(r#"{"name":"  ali\u001bce  "}"#);

        assert_eq!(format_profile_name(&profile).as_deref(), Some("alice"));
    }

    #[test]
    fn skips_a_name_that_sanitizes_to_nothing() {
        let profile = parse_profile(r#"{"display_name":"\u202e","name":"alice"}"#);

        assert_eq!(format_profile_name(&profile).as_deref(), Some("alice"));
    }

    #[test]
    fn keeps_the_first_and_last_8_characters_of_the_npub() {
        let public_key = Keys::generate().public_key();
        let Ok(npub) = public_key.to_bech32();

        assert_eq!(
            truncate_npub(&public_key),
            format!("{}…{}", &npub[..8], &npub[npub.len() - 8..])
        );
    }
}
