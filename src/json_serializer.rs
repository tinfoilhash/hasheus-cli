use crate::file_event;
use crate::hasheus_error::HasheusError;
use crate::name_formatter;
use crate::verification_sorter::{self, Verdict};
use crate::verifications::Verifications;
use nostr_sdk::prelude::*;
use serde::Serialize;

#[derive(Serialize)]
struct VerificationEnvelope<'a> {
    verification: VerificationJSON<'a>,
}

#[derive(Serialize)]
struct VerificationJSON<'a> {
    hash: &'a str,
    status: &'static str,
    source: Option<SourceJSON>,
    trusted_verifiers: Vec<VerifierJSON>,
    unknown_verifiers: Vec<VerifierJSON>,
    user: Option<UserJSON>,
}

#[derive(Serialize)]
struct SourceJSON {
    description: Option<String>,
    url: Option<String>,
    is_trusted: bool,
    agreement_count: usize,
    verifier: VerifierJSON,
}

#[derive(Serialize)]
struct VerifierJSON {
    pubkey: String,
    npub: String,
    name: Option<String>,
    created_at: Option<String>,
}

#[derive(Serialize)]
struct UserJSON {
    pubkey: String,
    npub: String,
    name: Option<String>,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorJSON,
}

#[derive(Serialize)]
struct ErrorJSON {
    code: &'static str,
    message: String,
}

// json escapes control characters, so relay-supplied text is passed through
// as is (trimmed), unlike the terminal output, which has to sanitize it.
pub fn serialize_verifications(hash: &str, verifications: &Verifications) -> String {
    let current_user = verifications.current_user.as_ref();
    let sorted = verification_sorter::sort(&verifications.file_events, current_user);
    let build_verifiers = |events: &[&Event]| -> Vec<VerifierJSON> {
        events
            .iter()
            .map(|event| build_verifier(event, verifications))
            .collect()
    };

    let verification = VerificationJSON {
        hash,
        status: serialize_verdict(sorted.verdict),
        source: sorted.source.event.map(|event| SourceJSON {
            description: non_blank(&event.content),
            url: file_event::url(event).map(String::from),
            is_trusted: sorted.source.is_trusted,
            agreement_count: sorted.source.agreement_count,
            verifier: build_verifier(event, verifications),
        }),
        trusted_verifiers: build_verifiers(&sorted.trusted_events),
        unknown_verifiers: build_verifiers(&sorted.unknown_events),
        user: current_user.map(|current_user| UserJSON {
            pubkey: current_user.public_key.to_hex(),
            npub: encode_npub(&current_user.public_key),
            name: profile_name(&current_user.public_key, verifications),
        }),
    };

    to_json(&VerificationEnvelope { verification })
}

pub fn serialize_error(error: &HasheusError) -> String {
    to_json(&ErrorEnvelope {
        error: ErrorJSON {
            code: error.code(),
            message: error.to_string(),
        },
    })
}

fn serialize_verdict(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::VerifiedByYou => "verified_by_you",
        Verdict::Trusted => "trusted",
        Verdict::Unknown => "unknown",
        Verdict::Unverified => "unverified",
    }
}

fn build_verifier(event: &Event, verifications: &Verifications) -> VerifierJSON {
    VerifierJSON {
        pubkey: event.pubkey.to_hex(),
        npub: encode_npub(&event.pubkey),
        name: profile_name(&event.pubkey, verifications),
        created_at: format_timestamp(event.created_at),
    }
}

fn profile_name(public_key: &PublicKey, verifications: &Verifications) -> Option<String> {
    verifications
        .profile(public_key)
        .and_then(|profile| name_formatter::profile_names(profile).next())
        .map(|name| name.trim().to_string())
}

fn encode_npub(public_key: &PublicKey) -> String {
    let Ok(npub) = public_key.to_bech32();

    npub
}

// anyone can set an event's time, so one past what jiff supports (the year
// 9999) is null rather than a misleading date.
fn format_timestamp(timestamp: Timestamp) -> Option<String> {
    let seconds = i64::try_from(timestamp.as_secs()).ok()?;

    Some(jiff::Timestamp::from_second(seconds).ok()?.to_string())
}

fn non_blank(text: &str) -> Option<String> {
    let text = text.trim();

    (!text.is_empty()).then(|| text.to_string())
}

// pretty-printed for people reading it in a terminal; it's still a single json
// document for programs.
fn to_json(value: &impl Serialize) -> String {
    serde_json::to_string_pretty(value).expect("plain structs always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::{
        HASH, build_event, build_file_event, build_follow_list, build_profile,
    };
    use crate::verification_fetcher::Update;
    use serde_json::{Value, json};

    fn serialize(verifications: &Verifications) -> Value {
        serde_json::from_str(&serialize_verifications(HASH, verifications)).unwrap()
    }

    fn build_verifications(
        user_public_key: Option<PublicKey>,
        updates: Vec<Update>,
    ) -> Verifications {
        let mut verifications = Verifications::new(user_public_key);

        for update in updates {
            verifications.apply_update(update);
        }

        verifications
    }

    #[test]
    fn serializes_a_file_no_one_has_verified() {
        let json = serialize(&build_verifications(None, Vec::new()));

        assert_eq!(
            json,
            json!({
                "verification": {
                    "hash": HASH,
                    "status": "unverified",
                    "source": null,
                    "trusted_verifiers": [],
                    "unknown_verifiers": [],
                    "user": null,
                }
            })
        );
    }

    #[test]
    fn serializes_an_unknown_source_with_its_raw_description() {
        let stranger = Keys::generate();
        let x = Tag::parse(["x", HASH]).unwrap();
        let url = Tag::parse(["url", "javascript:alert(1)"]).unwrap();
        let event = build_event(
            &stranger,
            Kind::FileMetadata,
            1_230_940_800,
            "  official\n\u{1b}[32mdownload  ",
            vec![x, url],
        );

        let json = serialize(&build_verifications(None, vec![Update::FileEvent(event)]));

        let verification = &json["verification"];
        assert_eq!(verification["status"], "unknown");
        assert_eq!(
            verification["source"]["description"],
            "official\n\u{1b}[32mdownload"
        );
        assert_eq!(verification["source"]["url"], Value::Null);
        assert_eq!(verification["source"]["is_trusted"], false);
        assert_eq!(
            verification["source"]["verifier"]["created_at"],
            "2009-01-03T00:00:00Z"
        );
        assert_eq!(
            verification["unknown_verifiers"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn serializes_a_trusted_source_and_the_user() {
        let (viewer, alice) = (Keys::generate(), Keys::generate());
        let verifications = build_verifications(
            Some(viewer.public_key()),
            vec![
                Update::Profile(build_profile(&viewer, 1, "viewer")),
                Update::Profile(build_profile(&alice, 1, "alice")),
                Update::FollowList(build_follow_list(&viewer, 1, &[alice.public_key()])),
                Update::FileEvent(build_file_event(
                    &alice,
                    1,
                    Some("https://example.com/a.iso"),
                )),
            ],
        );

        let json = serialize(&verifications);

        let verification = &json["verification"];
        assert_eq!(verification["status"], "trusted");
        assert_eq!(verification["source"]["url"], "https://example.com/a.iso");
        assert_eq!(verification["source"]["is_trusted"], true);
        assert_eq!(verification["source"]["agreement_count"], 1);
        assert_eq!(
            verification["trusted_verifiers"],
            json!([{
                "pubkey": alice.public_key().to_hex(),
                "npub": encode_npub(&alice.public_key()),
                "name": "alice",
                "created_at": "1970-01-01T00:00:01Z",
            }])
        );
        assert_eq!(
            verification["user"],
            json!({
                "pubkey": viewer.public_key().to_hex(),
                "npub": encode_npub(&viewer.public_key()),
                "name": "viewer",
            })
        );
    }

    #[test]
    fn serializes_a_file_the_user_has_verified() {
        let viewer = Keys::generate();
        let verifications = build_verifications(
            Some(viewer.public_key()),
            vec![Update::FileEvent(build_file_event(&viewer, 1, None))],
        );

        assert_eq!(
            serialize(&verifications)["verification"]["status"],
            "verified_by_you"
        );
    }

    #[test]
    fn leaves_out_names_and_dates_it_cant_show() {
        let stranger = Keys::generate();
        let verifications = build_verifications(
            None,
            vec![Update::FileEvent(build_file_event(
                &stranger,
                u64::MAX,
                None,
            ))],
        );

        let verifier = &serialize(&verifications)["verification"]["unknown_verifiers"][0];

        assert_eq!(verifier["name"], Value::Null);
        assert_eq!(verifier["created_at"], Value::Null);
    }

    #[test]
    fn serializes_an_error_with_its_code() {
        let json: Value =
            serde_json::from_str(&serialize_error(&HasheusError::FileRelaysUnreachable)).unwrap();

        assert_eq!(
            json,
            json!({
                "error": {
                    "code": "file_relays_unreachable",
                    "message": "unable to connect to hasheus server",
                }
            })
        );
    }
}
