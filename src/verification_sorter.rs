use crate::current_user::CurrentUser;
use crate::file_event;
use nostr_sdk::prelude::*;
use std::collections::HashSet;

#[derive(Debug, PartialEq)]
pub struct Source<'a> {
    pub event: Option<&'a Event>,
    pub is_trusted: bool,
    pub agreement_count: usize,
}

pub struct SortedVerifications<'a> {
    pub trusted_events: Vec<&'a Event>,
    pub untrusted_events: Vec<&'a Event>,
    pub source: Source<'a>,
    pub verdict: Verdict,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    VerifiedByYou,
    Trusted,
    Untrusted,
    Unverified,
}

// events must be sorted oldest first.
pub fn sort<'a>(
    events: &'a [Event],
    current_user: Option<&CurrentUser>,
) -> SortedVerifications<'a> {
    let verifier_events = first_event_per_pubkey(events);

    let (trusted_events, untrusted_events): (Vec<_>, Vec<_>) =
        verifier_events.iter().partition(|event| {
            current_user.is_some_and(|current_user| current_user.has_trusted(&event.pubkey))
        });

    let source = pick_source(&verifier_events, &trusted_events);
    let verdict = pick_verdict(&trusted_events, &source, current_user);

    SortedVerifications {
        trusted_events,
        untrusted_events,
        source,
        verdict,
    }
}

fn pick_verdict(
    trusted_events: &[&Event],
    source: &Source,
    current_user: Option<&CurrentUser>,
) -> Verdict {
    let has_current_user_verified = current_user.is_some_and(|current_user| {
        trusted_events
            .iter()
            .any(|event| event.pubkey == current_user.public_key)
    });

    if has_current_user_verified {
        return Verdict::VerifiedByYou;
    }

    match source {
        Source { event: None, .. } => Verdict::Unverified,
        Source {
            is_trusted: true, ..
        } => Verdict::Trusted,
        _ => Verdict::Untrusted,
    }
}

// someone can verify the same file more than once, so only their first
// verification counts.
fn first_event_per_pubkey(events: &[Event]) -> Vec<&Event> {
    let mut seen_pubkeys = HashSet::new();

    events
        .iter()
        .filter(|event| seen_pubkeys.insert(event.pubkey))
        .collect()
}

// anyone can backdate a verification, so the description and url come from
// the url most trusted verifiers agree on, with ties going to the earliest.
fn pick_source<'a>(verifier_events: &[&'a Event], trusted_events: &[&'a Event]) -> Source<'a> {
    if trusted_events.is_empty() {
        return Source {
            event: verifier_events.first().copied(),
            is_trusted: false,
            agreement_count: 0,
        };
    }

    let mut events_by_url: Vec<(Url, Vec<&Event>)> = Vec::new();

    for &event in trusted_events {
        let Some(url) = file_event::url(event) else {
            continue;
        };

        match events_by_url
            .iter_mut()
            .find(|(group_url, _)| *group_url == url)
        {
            Some((_, events)) => events.push(event),
            None => events_by_url.push((url, vec![event])),
        }
    }

    let mut agreeing_events: Vec<&Event> = Vec::new();

    for (_, events) in events_by_url {
        if events.len() > agreeing_events.len() {
            agreeing_events = events;
        }
    }

    Source {
        event: agreeing_events.first().or(trusted_events.first()).copied(),
        is_trusted: true,
        agreement_count: agreeing_events.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::{build_file_event, build_follow_list};

    struct Users {
        me: Keys,
        trusted_a: Keys,
        trusted_b: Keys,
        trusted_c: Keys,
        stranger: Keys,
    }

    impl Users {
        fn new() -> Self {
            Self {
                me: Keys::generate(),
                trusted_a: Keys::generate(),
                trusted_b: Keys::generate(),
                trusted_c: Keys::generate(),
                stranger: Keys::generate(),
            }
        }

        fn current_user(&self, follows: &[&Keys]) -> CurrentUser {
            let follows: Vec<PublicKey> = follows.iter().map(|keys| keys.public_key()).collect();
            let follow_list = build_follow_list(&self.me, 1, &follows);

            let mut current_user = CurrentUser::new(self.me.public_key());
            current_user.set_follow_list(follow_list);
            current_user
        }
    }

    #[test]
    fn returns_no_source_when_there_are_no_events() {
        let sorted = sort(&[], None);

        assert!(sorted.trusted_events.is_empty());
        assert!(sorted.untrusted_events.is_empty());
        assert_eq!(
            sorted.source,
            Source {
                event: None,
                is_trusted: false,
                agreement_count: 0,
            }
        );
    }

    #[test]
    fn keeps_only_the_first_event_per_pubkey() {
        let users = Users::new();
        let events = [
            build_file_event(&users.stranger, 1, None),
            build_file_event(&users.stranger, 2, None),
        ];

        let sorted = sort(&events, None);

        assert_eq!(sorted.untrusted_events, [&events[0]]);
    }

    #[test]
    fn treats_everyone_as_untrusted_without_a_current_user() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, None),
            build_file_event(&users.stranger, 2, None),
        ];

        let sorted = sort(&events, None);

        assert!(sorted.trusted_events.is_empty());
        assert_eq!(sorted.untrusted_events, [&events[0], &events[1]]);
    }

    #[test]
    fn splits_events_by_whether_the_current_user_trusts_their_author() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, None),
            build_file_event(&users.me, 2, None),
            build_file_event(&users.stranger, 3, None),
        ];
        let current_user = users.current_user(&[&users.trusted_a]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(sorted.trusted_events, [&events[0], &events[1]]);
        assert_eq!(sorted.untrusted_events, [&events[2]]);
    }

    #[test]
    fn falls_back_to_the_earliest_event_untrusted_without_trusted_verifiers() {
        let users = Users::new();
        let events = [
            build_file_event(&users.stranger, 1, Some("https://evil.example/file")),
            build_file_event(&users.trusted_a, 2, Some("https://good.example/file")),
        ];

        let sorted = sort(&events, None);

        assert_eq!(
            sorted.source,
            Source {
                event: Some(&events[0]),
                is_trusted: false,
                agreement_count: 0,
            }
        );
    }

    #[test]
    fn ignores_a_backdated_untrusted_event_when_a_trusted_verifier_exists() {
        let users = Users::new();
        let events = [
            build_file_event(&users.stranger, 1, Some("https://evil.example/file")),
            build_file_event(&users.trusted_a, 2, Some("https://good.example/file")),
        ];
        let current_user = users.current_user(&[&users.trusted_a]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(
            sorted.source,
            Source {
                event: Some(&events[1]),
                is_trusted: true,
                agreement_count: 1,
            }
        );
    }

    #[test]
    fn picks_the_url_most_trusted_verifiers_agree_on() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, Some("https://mirror.example/file")),
            build_file_event(&users.trusted_b, 2, Some("https://official.example/file")),
            // a differently cased host is the same url, so it joins the same group.
            build_file_event(&users.trusted_c, 3, Some("https://OFFICIAL.example/file")),
        ];
        let current_user =
            users.current_user(&[&users.trusted_a, &users.trusted_b, &users.trusted_c]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(
            sorted.source,
            Source {
                event: Some(&events[1]),
                is_trusted: true,
                agreement_count: 2,
            }
        );
    }

    #[test]
    fn breaks_ties_in_agreement_by_the_earliest_verifier() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, Some("https://a.example/file")),
            build_file_event(&users.trusted_b, 2, Some("https://b.example/file")),
        ];
        let current_user = users.current_user(&[&users.trusted_a, &users.trusted_b]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(sorted.source.event, Some(&events[0]));
    }

    #[test]
    fn doesnt_let_trusted_events_without_a_url_outvote_one_with_a_url() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, None),
            build_file_event(&users.trusted_b, 2, None),
            build_file_event(&users.trusted_c, 3, Some("https://real.example/file")),
        ];
        let current_user =
            users.current_user(&[&users.trusted_a, &users.trusted_b, &users.trusted_c]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(
            sorted.source,
            Source {
                event: Some(&events[2]),
                is_trusted: true,
                agreement_count: 1,
            }
        );
    }

    #[test]
    fn doesnt_let_trusted_events_with_a_non_web_url_outvote_one_with_a_web_url() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, Some("javascript:alert(1)")),
            build_file_event(&users.trusted_b, 2, Some("javascript:alert(1)")),
            build_file_event(&users.trusted_c, 3, Some("https://real.example/file")),
        ];
        let current_user =
            users.current_user(&[&users.trusted_a, &users.trusted_b, &users.trusted_c]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(sorted.source.event, Some(&events[2]));
        assert_eq!(sorted.source.agreement_count, 1);
    }

    #[test]
    fn falls_back_to_the_earliest_trusted_event_when_none_have_a_url() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, None),
            build_file_event(&users.trusted_b, 2, None),
        ];
        let current_user = users.current_user(&[&users.trusted_a, &users.trusted_b]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(
            sorted.source,
            Source {
                event: Some(&events[0]),
                is_trusted: true,
                agreement_count: 0,
            }
        );
    }

    #[test]
    fn calls_a_file_without_events_unverified() {
        assert_eq!(sort(&[], None).verdict, Verdict::Unverified);
    }

    #[test]
    fn calls_a_source_without_trusted_verifiers_untrusted() {
        let users = Users::new();
        let events = [build_file_event(&users.stranger, 1, None)];
        let current_user = users.current_user(&[&users.trusted_a]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(sorted.verdict, Verdict::Untrusted);
    }

    #[test]
    fn calls_a_source_with_a_trusted_verifier_trusted() {
        let users = Users::new();
        let events = [build_file_event(&users.trusted_a, 1, None)];
        let current_user = users.current_user(&[&users.trusted_a]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(sorted.verdict, Verdict::Trusted);
    }

    #[test]
    fn notices_when_the_user_has_verified_the_file() {
        let users = Users::new();
        let events = [
            build_file_event(&users.trusted_a, 1, None),
            build_file_event(&users.me, 2, None),
        ];
        let current_user = users.current_user(&[&users.trusted_a]);

        let sorted = sort(&events, Some(&current_user));

        assert_eq!(sorted.verdict, Verdict::VerifiedByYou);
    }
}
