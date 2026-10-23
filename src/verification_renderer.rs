use crate::current_user::CurrentUser;
use crate::file_event;
use crate::file_hasher::FileHash;
use crate::name_formatter;
use crate::spinner;
use crate::text_sanitizer::sanitize;
use crate::verification_sorter::{self, SortedVerifications, Source, Verdict};
use crate::verifications::Verifications;
use jiff::fmt::strtime;
use jiff::tz::TimeZone;
use nostr_sdk::prelude::*;
use yansi::{Color, Paint};

const EMPTY_VALUE: &str = "—";
// like the web page's connect button.
const USER_HINT: &str = "connect: set HASHEUS_PUBLIC_KEY or pass --user <npub> to see if anyone in your web of trust has verified this file";
// like the web page's "add your verification" link.
const PUBLISH_HINT: &str = "add your verification: hasheus publish --url <url> <file>";
const DATE_FORMAT: &str = "%b %-d, %Y, %-I:%M %P %Z";

pub fn render(file_hash: &FileHash, verifications: &Verifications, spinner_frame: usize) -> String {
    let Verifications {
        current_user,
        is_pending,
        ..
    } = verifications;
    let current_user = current_user.as_ref();
    let sorted = verification_sorter::sort(&verifications.file_events, current_user);
    let empty_value = render_empty_value(*is_pending, spinner_frame);

    let sections = [
        render_status(&sorted, current_user, *is_pending),
        vec!["file hash".to_string(), file_hash.hash.bold().to_string()],
        render_description(&sorted.source, &empty_value),
        render_url(&sorted.source, &empty_value),
        render_source(&sorted.source, verifications, &empty_value),
        render_verifiers(
            "trusted verifiers",
            &sorted.trusted_events,
            verifications,
            &empty_value,
        ),
        render_verifiers(
            "untrusted verifiers",
            &sorted.untrusted_events,
            verifications,
            &empty_value,
        ),
        render_actions(&sorted, file_hash, current_user, verifications),
    ];

    sections
        .iter()
        .filter(|lines| !lines.is_empty())
        .map(|lines| lines.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_empty_value(is_pending: bool, spinner_frame: usize) -> String {
    let value = if is_pending {
        spinner::frame(spinner_frame)
    } else {
        EMPTY_VALUE
    };

    value.bold().to_string()
}

// the verdict can change until the file events and your profile and follow
// list are in, so only a verification of your own shows before then.
fn render_status(
    sorted: &SortedVerifications,
    current_user: Option<&CurrentUser>,
    is_pending: bool,
) -> Vec<String> {
    let (status, color) = match sorted.verdict {
        Verdict::VerifiedByYou => ("you have verified this file!".to_string(), Color::Green),
        _ if is_pending => return Vec::new(),
        Verdict::Unverified => (
            "this file has not been verified!".to_string(),
            Color::Magenta,
        ),
        Verdict::Untrusted => {
            let reason = if current_user.is_some() {
                "no one in your web of trust has verified this file"
            } else {
                "connect to see if anyone in your web of trust has verified this file"
            };

            (
                format!("{reason}. its source and url are untrusted."),
                Color::Magenta,
            )
        }
        Verdict::Trusted => return Vec::new(),
    };

    vec![format!(" {status} ").bold().bg(color).to_string()]
}

// labeled whenever it shows, even while loading, so an untrusted source is
// never shown unlabeled.
fn is_untrusted(source: &Source) -> bool {
    source.event.is_some() && !source.is_trusted
}

fn render_description(source: &Source, empty_value: &str) -> Vec<String> {
    let description = source
        .event
        .map(|event| sanitize(&event.content).trim().to_string())
        .filter(|content| !content.is_empty())
        .map_or_else(
            || empty_value.to_string(),
            |content| content.bold().to_string(),
        );

    vec!["description".to_string(), description]
}

// the url keeps an untrusted tag, since that's what people follow, and counts
// the other trusted verifiers who agree on it.
fn render_url(source: &Source, empty_value: &str) -> Vec<String> {
    let url = source.event.and_then(file_event::url);
    let tags = [
        is_untrusted(source).then(render_untrusted_tag),
        render_agreement_tag(source.agreement_count),
    ];

    vec![
        render_key("url", tags),
        url.as_ref().map_or_else(
            || empty_value.to_string(),
            |url| url.as_str().bold().to_string(),
        ),
    ]
}

// trust belongs to whoever published the verification, so the source carries
// the trusted or untrusted tag.
fn render_source(source: &Source, verifications: &Verifications, empty_value: &str) -> Vec<String> {
    let tags = [
        is_untrusted(source).then(render_untrusted_tag),
        source.is_trusted.then(render_trusted_tag),
    ];
    let verifier = source.event.map_or_else(
        || empty_value.to_string(),
        |event| render_verifier(event, verifications),
    );

    vec![render_key("source", tags), verifier]
}

fn render_verifiers(
    key: &str,
    events: &[&Event],
    verifications: &Verifications,
    empty_value: &str,
) -> Vec<String> {
    let mut lines = vec![key.to_string()];
    lines.extend(render_verifier_list(events, verifications, empty_value));

    lines
}

// like the web page's buttons at the bottom: add your verification, unless you
// already have or there's no file to publish, then connect or who's connected.
fn render_actions(
    sorted: &SortedVerifications,
    file_hash: &FileHash,
    current_user: Option<&CurrentUser>,
    verifications: &Verifications,
) -> Vec<String> {
    let mut lines = Vec::new();

    if file_hash.has_file && sorted.verdict != Verdict::VerifiedByYou {
        lines.push(PUBLISH_HINT.dim().to_string());
    }

    let Some(current_user) = current_user else {
        lines.push(USER_HINT.dim().to_string());
        return lines;
    };

    let name = display_name(&current_user.public_key, verifications);
    lines.push(format!("{} {}", "connected:".dim(), name.dim()));

    lines
}

fn render_key<const N: usize>(key: &str, tags: [Option<String>; N]) -> String {
    let tags: Vec<String> = tags.into_iter().flatten().collect();

    if tags.is_empty() {
        return key.to_string();
    }

    format!("{key} {}", tags.join(" "))
}

fn render_untrusted_tag() -> String {
    " untrusted ".on_magenta().to_string()
}

fn render_trusted_tag() -> String {
    " trusted ".on_green().to_string()
}

// the source is one of the agreeing verifiers, so only the others are counted.
fn render_agreement_tag(agreement_count: usize) -> Option<String> {
    let other_count = agreement_count.checked_sub(1).filter(|count| *count > 0)?;

    Some(format!(" +{other_count} trusted ").on_green().to_string())
}

fn render_verifier_list(
    events: &[&Event],
    verifications: &Verifications,
    empty_value: &str,
) -> Vec<String> {
    if events.is_empty() {
        return vec![empty_value.to_string()];
    }

    events
        .iter()
        .map(|event| render_verifier(event, verifications))
        .collect()
}

fn render_verifier(event: &Event, verifications: &Verifications) -> String {
    format!(
        "{} {}",
        display_name(&event.pubkey, verifications).bold(),
        format_timestamp(event.created_at).dim()
    )
}

fn display_name(public_key: &PublicKey, verifications: &Verifications) -> String {
    verifications
        .profile(public_key)
        .and_then(name_formatter::format_profile_name)
        .unwrap_or_else(|| name_formatter::truncate_npub(public_key))
}

fn format_timestamp(timestamp: Timestamp) -> String {
    format_timestamp_in(timestamp, TimeZone::system())
}

// anyone can set an event's time, so one past what jiff supports (the year
// 9999) falls back to the raw unix timestamp.
fn format_timestamp_in(timestamp: Timestamp, time_zone: TimeZone) -> String {
    i64::try_from(timestamp.as_secs())
        .ok()
        .and_then(|seconds| jiff::Timestamp::from_second(seconds).ok())
        .and_then(|timestamp| strtime::format(DATE_FORMAT, &timestamp.to_zoned(time_zone)).ok())
        .map(|date| date.to_lowercase())
        .unwrap_or_else(|| timestamp.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::{
        HASH, build_event, build_file_event, build_follow_list, build_profile,
    };
    use crate::verification_fetcher::Update;

    fn build_file_hash() -> FileHash {
        FileHash {
            hash: HASH.to_string(),
            has_file: true,
        }
    }

    fn new_york() -> TimeZone {
        TimeZone::get("America/New_York").unwrap()
    }

    fn build_verifications(is_pending: bool) -> Verifications {
        yansi::disable();

        let mut verifications = Verifications::new(None);
        verifications.is_pending = is_pending;

        verifications
    }

    fn add_file_event(verifications: &mut Verifications) {
        let event = build_file_event(&Keys::generate(), 1, None);

        verifications.apply_update(Update::FileEvent(event));
    }

    #[test]
    fn render_shows_pending_values_while_pending() {
        let rendered = render(&build_file_hash(), &build_verifications(true), 2);

        assert!(rendered.contains("source\n⠹"));
        assert!(!rendered.contains("this file has not been verified!"));
    }

    #[test]
    fn render_waits_for_every_relay_before_calling_a_source_untrusted() {
        let mut verifications = build_verifications(true);
        add_file_event(&mut verifications);

        let rendered = render(&build_file_hash(), &verifications, 0);

        assert!(rendered.contains("url  untrusted "));
        assert!(rendered.contains("source  untrusted "));
        assert!(!rendered.contains("its source and url are untrusted"));
    }

    #[test]
    fn render_cycles_through_the_spinner_frames() {
        let verifications = build_verifications(true);

        let sources: Vec<String> = (0..11)
            .map(|frame| {
                let rendered = render(&build_file_hash(), &verifications, frame);
                let source_index = rendered.find("source\n").unwrap() + "source\n".len();
                rendered[source_index..].lines().next().unwrap().to_string()
            })
            .collect();

        assert_eq!(
            sources,
            ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠋"]
        );
    }

    #[test]
    fn render_shows_the_verdict_once_complete() {
        let mut verifications = build_verifications(false);
        add_file_event(&mut verifications);

        let rendered = render(&build_file_hash(), &verifications, 0);

        assert!(rendered.starts_with(
            " connect to see if anyone in your web of trust has verified this file. its source and url are untrusted. "
        ));
    }

    #[test]
    fn render_shows_empty_values_once_complete() {
        let rendered = render(&build_file_hash(), &build_verifications(false), 2);

        assert!(rendered.starts_with(" this file has not been verified! \n\nfile hash"));
        assert!(rendered.contains("source\n—"));
    }

    // a viewer who follows alice and bob, who both verified the same url.
    fn build_trusted_verifications() -> Verifications {
        let (viewer, alice, bob) = (Keys::generate(), Keys::generate(), Keys::generate());
        let mut verifications = build_verifications(false);
        verifications.current_user = Some(CurrentUser::new(viewer.public_key()));

        for update in [
            Update::Profile(build_profile(&viewer, 1, "viewer")),
            Update::Profile(build_profile(&alice, 1, "alice")),
            Update::FollowList(build_follow_list(
                &viewer,
                1,
                &[alice.public_key(), bob.public_key()],
            )),
            Update::FileEvent(build_file_event(
                &alice,
                1,
                Some("https://example.com/file.iso"),
            )),
            Update::FileEvent(build_file_event(
                &bob,
                2,
                Some("https://example.com/file.iso"),
            )),
        ] {
            verifications.apply_update(update);
        }

        verifications
    }

    #[test]
    fn render_tags_a_trusted_source_and_shows_no_verdict() {
        let rendered = render(&build_file_hash(), &build_trusted_verifications(), 0);

        assert!(rendered.starts_with("file hash"));
        assert!(rendered.contains("description\ndescription"));
        assert!(rendered.contains("source  trusted \nalice "));
        assert!(!rendered.contains(&render_untrusted_tag()));
        assert!(!rendered.contains("are untrusted"));
    }

    #[test]
    fn render_counts_the_other_trusted_verifiers_on_the_url() {
        let rendered = render(&build_file_hash(), &build_trusted_verifications(), 0);

        assert!(rendered.contains("url  +1 trusted \nhttps://example.com/file.iso"));
        assert!(!rendered.contains("linked by"));
    }

    // a viewer who follows alice, and alice's verification, with or without a url.
    fn build_lone_trusted_verifications(url: Option<&str>) -> Verifications {
        let (viewer, alice) = (Keys::generate(), Keys::generate());
        let mut verifications = build_verifications(false);
        verifications.current_user = Some(CurrentUser::new(viewer.public_key()));
        verifications.apply_update(Update::FollowList(build_follow_list(
            &viewer,
            1,
            &[alice.public_key()],
        )));
        verifications.apply_update(Update::FileEvent(build_file_event(&alice, 1, url)));

        verifications
    }

    #[test]
    fn render_leaves_out_the_agreement_tag_for_a_lone_trusted_verifier() {
        let verifications = build_lone_trusted_verifications(Some("https://example.com/file.iso"));

        let rendered = render(&build_file_hash(), &verifications, 0);

        assert!(rendered.contains("url\nhttps://example.com/file.iso"));
        assert!(rendered.contains("source  trusted "));
    }

    #[test]
    fn render_tags_a_trusted_source_without_a_url() {
        let rendered = render(
            &build_file_hash(),
            &build_lone_trusted_verifications(None),
            0,
        );

        assert!(rendered.contains("url\n—"));
        assert!(rendered.contains("source  trusted "));
    }

    #[test]
    fn render_agreement_tag_leaves_out_the_source() {
        yansi::disable();

        assert_eq!(render_agreement_tag(0), None);
        assert_eq!(render_agreement_tag(1), None);
        assert_eq!(render_agreement_tag(3).as_deref(), Some(" +2 trusted "));
    }

    #[test]
    fn render_lists_trusted_verifiers_and_who_is_connected() {
        let rendered = render(&build_file_hash(), &build_trusted_verifications(), 0);

        assert!(rendered.contains("trusted verifiers\nalice "));
        assert!(rendered.ends_with(&format!(
            "untrusted verifiers\n—\n\n{PUBLISH_HINT}\nconnected: viewer"
        )));
    }

    #[test]
    fn render_congratulates_a_user_who_has_verified_the_file() {
        let viewer = Keys::generate();
        let mut verifications = build_verifications(true);
        verifications.current_user = Some(CurrentUser::new(viewer.public_key()));
        verifications.apply_update(Update::FileEvent(build_file_event(&viewer, 1, None)));

        let rendered = render(&build_file_hash(), &verifications, 0);

        assert!(rendered.starts_with(" you have verified this file! "));
        assert!(!rendered.contains(PUBLISH_HINT));
    }

    #[test]
    fn render_warns_when_no_one_trusted_has_verified_the_file() {
        let mut verifications = build_verifications(false);
        verifications.current_user = Some(CurrentUser::new(Keys::generate().public_key()));
        add_file_event(&mut verifications);

        let rendered = render(&build_file_hash(), &verifications, 0);

        assert!(rendered.starts_with(
            " no one in your web of trust has verified this file. its source and url are untrusted. "
        ));
    }

    #[test]
    fn render_hints_at_passing_a_user_without_one() {
        let rendered = render(&build_file_hash(), &build_verifications(false), 0);

        assert!(rendered.contains("trusted verifiers\n—"));
        assert!(rendered.ends_with(&format!("{PUBLISH_HINT}\n{USER_HINT}")));
    }

    #[test]
    fn render_leaves_out_the_publish_hint_without_a_file() {
        let file_hash = FileHash {
            has_file: false,
            ..build_file_hash()
        };

        let rendered = render(&file_hash, &build_verifications(false), 0);

        assert!(!rendered.contains(PUBLISH_HINT));
        assert!(rendered.ends_with(&format!("untrusted verifiers\n—\n\n{USER_HINT}")));
    }

    #[test]
    fn format_timestamp_in_formats_in_the_time_zone() {
        assert_eq!(
            format_timestamp_in(Timestamp::from_secs(1_769_686_573), new_york()),
            "jan 29, 2026, 6:36 am est"
        );
    }

    #[test]
    fn format_timestamp_in_follows_daylight_saving_time() {
        assert_eq!(
            format_timestamp_in(Timestamp::from_secs(1_783_000_000), new_york()),
            "jul 2, 2026, 9:46 am edt"
        );
    }

    #[test]
    fn format_timestamp_in_falls_back_to_the_unix_timestamp_past_the_year_9999() {
        assert_eq!(
            format_timestamp_in(Timestamp::from_secs(253_402_300_800), new_york()),
            "253402300800"
        );
        assert_eq!(
            format_timestamp_in(Timestamp::from_secs(u64::MAX), new_york()),
            u64::MAX.to_string()
        );
    }

    fn build_named_verifications(keys: &Keys, profile_content: &str) -> Verifications {
        let mut verifications = Verifications::new(None);
        let profile = build_event(keys, Kind::Metadata, 1, profile_content, Vec::new());
        verifications.apply_update(Update::Profile(profile));

        verifications
    }

    #[test]
    fn display_name_uses_the_profile_name() {
        let keys = Keys::generate();
        let verifications = build_named_verifications(&keys, r#"{"name":"alice"}"#);

        assert_eq!(display_name(&keys.public_key(), &verifications), "alice");
    }

    #[test]
    fn display_name_falls_back_to_the_truncated_npub() {
        let public_key = Keys::generate().public_key();
        let Ok(npub) = public_key.to_bech32();

        let name = display_name(&public_key, &Verifications::new(None));

        assert_eq!(name, format!("{}…{}", &npub[..8], &npub[npub.len() - 8..]));
    }

    #[test]
    fn render_trims_the_description() {
        let mut verifications = build_verifications(false);
        let x = Tag::parse(["x", HASH]).unwrap();
        let event = build_event(
            &Keys::generate(),
            Kind::FileMetadata,
            1,
            "  padded  ",
            vec![x],
        );
        verifications.apply_update(Update::FileEvent(event));

        let rendered = render(&build_file_hash(), &verifications, 0);

        assert!(rendered.contains("description\npadded\n"));
    }
}
