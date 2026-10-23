use crate::hasheus_error::HasheusError;
use crate::relay_config::RelayConfig;
use crate::relay_connector;
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const UPDATE_CHANNEL_CAPACITY: usize = 256;

pub enum Update {
    FileEvent(Event),
    Profile(Event),
    FollowList(Event),
    // the file events and your profile and follow list are in. only other
    // profiles, which change names but not the verdict, may still be coming.
    Loaded,
}

// what a relay is asked for: file relays fetch file events, and user relays
// fetch profiles, plus your profile and follow list when there's a user to
// fetch.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Stage {
    FileEvents,
    CurrentUser,
    Profiles,
}

// a user given to verify needs a user relay to answer, so who they trust is
// known, and a profile, to catch a mistyped or unknown key. one who just
// published doesn't: their key just signed, and the verify after publishing
// still shows their own verification as trusted without a follow list.
#[derive(Clone, Copy, PartialEq)]
pub enum UserRequirement {
    Required,
    NotRequired,
}

enum RelayMessage {
    Update(Update),
    Finished { stage: Stage, has_answered: bool },
}

// the profiles to fetch grow as verifiers are found, and are final once every
// file relay has finished.
struct Authors {
    public_keys: HashSet<PublicKey>,
    is_final: bool,
}

struct RelayFetch {
    client: Client,
    url: RelayUrl,
    stages: Vec<Stage>,
    hash: String,
    user_to_fetch: Option<PublicKey>,
    authors: watch::Receiver<Authors>,
    sender: mpsc::UnboundedSender<RelayMessage>,
}

struct Progress {
    pending_count_by_stage: HashMap<Stage, usize>,
    answered_stages: HashSet<Stage>,
    user_requirement: UserRequirement,
}

impl Progress {
    fn new(
        stages_by_url: &HashMap<RelayUrl, Vec<Stage>>,
        user_requirement: UserRequirement,
    ) -> Self {
        let mut pending_count_by_stage = HashMap::new();

        for stage in stages_by_url.values().flatten() {
            *pending_count_by_stage.entry(*stage).or_insert(0) += 1;
        }

        Self {
            pending_count_by_stage,
            answered_stages: HashSet::new(),
            user_requirement,
        }
    }

    fn is_stage_finished(&self, stage: Stage) -> bool {
        self.pending_count_by_stage
            .get(&stage)
            .is_none_or(|count| *count == 0)
    }

    fn is_loaded(&self) -> bool {
        self.is_stage_finished(Stage::FileEvents) && self.is_stage_finished(Stage::CurrentUser)
    }

    // like the web page, a stage fails once none of its relays has answered.
    // missing profiles only cost names, so user relays are only required when a
    // follow list is needed to know who you trust, and the user is required.
    fn finish(&mut self, stage: Stage, has_answered: bool) -> Result<(), HasheusError> {
        if let Some(pending_count) = self.pending_count_by_stage.get_mut(&stage) {
            *pending_count -= 1;
        }

        if has_answered {
            self.answered_stages.insert(stage);
        }

        if !self.is_stage_finished(stage) || self.answered_stages.contains(&stage) {
            return Ok(());
        }

        match stage {
            Stage::FileEvents => Err(HasheusError::FileRelaysUnreachable),
            Stage::CurrentUser if self.user_requirement == UserRequirement::Required => {
                Err(HasheusError::UserRelaysUnreachable)
            }
            Stage::CurrentUser | Stage::Profiles => Ok(()),
        }
    }
}

// each relay connects and streams on its own, so a slow relay doesn't hold up
// the others. the channel closes once every relay has finished. a user to fetch
// has their profile and follow list fetched, and is required to have a profile.
pub fn fetch(
    hash: &str,
    user_to_fetch: Option<PublicKey>,
    relay_config: &RelayConfig,
    user_requirement: UserRequirement,
) -> mpsc::Receiver<Result<Update, HasheusError>> {
    let (sender, receiver) = mpsc::channel(UPDATE_CHANNEL_CAPACITY);

    tokio::spawn(coordinate(
        hash.to_string(),
        user_to_fetch,
        group_stages_by_url(relay_config, user_to_fetch.is_some()),
        user_requirement,
        sender,
    ));

    receiver
}

// a relay can be both a file and a user relay, so it only connects once.
fn group_stages_by_url(
    relay_config: &RelayConfig,
    has_user: bool,
) -> HashMap<RelayUrl, Vec<Stage>> {
    let mut stages_by_url: HashMap<RelayUrl, Vec<Stage>> = HashMap::new();

    let user_stages: &[Stage] = if has_user {
        &[Stage::Profiles, Stage::CurrentUser]
    } else {
        &[Stage::Profiles]
    };
    let urls_with_stages = relay_config
        .file_relay_urls
        .iter()
        .map(|url| (url, &[Stage::FileEvents][..]))
        .chain(
            relay_config
                .user_relay_urls
                .iter()
                .map(|url| (url, user_stages)),
        );

    // each relay finishes a stage once, so a url listed twice only gets each
    // stage once, or its extra copy would never finish.
    for (url, stages) in urls_with_stages {
        let url_stages = stages_by_url.entry(url.clone()).or_default();

        for stage in stages {
            if !url_stages.contains(stage) {
                url_stages.push(*stage);
            }
        }
    }

    stages_by_url
}

async fn coordinate(
    hash: String,
    user_to_fetch: Option<PublicKey>,
    stages_by_url: HashMap<RelayUrl, Vec<Stage>>,
    user_requirement: UserRequirement,
    sender: mpsc::Sender<Result<Update, HasheusError>>,
) {
    // the client shuts its relays down when dropped, so it's held until the end.
    let client = Client::default();
    let mut progress = Progress::new(&stages_by_url, user_requirement);
    let mut has_loaded = false;
    let mut has_found_user_profile = false;
    let (authors_sender, _) = watch::channel(Authors {
        public_keys: HashSet::new(),
        is_final: false,
    });
    let (relay_sender, mut relay_receiver) = mpsc::unbounded_channel();

    for (url, stages) in stages_by_url {
        tokio::spawn(fetch_from_relay(RelayFetch {
            client: client.clone(),
            url,
            stages,
            hash: hash.clone(),
            user_to_fetch,
            authors: authors_sender.subscribe(),
            sender: relay_sender.clone(),
        }));
    }
    drop(relay_sender);

    while let Some(message) = relay_receiver.recv().await {
        let update = match message {
            RelayMessage::Update(update) => update,
            RelayMessage::Finished {
                stage,
                has_answered,
            } => {
                let result = progress.finish(stage, has_answered).and_then(|()| {
                    // like the web page, a user without a profile is treated
                    // as a mistyped or unknown key rather than trusting no one.
                    let is_user_finished = stage == Stage::CurrentUser
                        && progress.is_stage_finished(Stage::CurrentUser);

                    let is_user_required = user_requirement == UserRequirement::Required;

                    if is_user_required && is_user_finished && !has_found_user_profile {
                        return Err(HasheusError::UserProfileNotFound);
                    }

                    Ok(())
                });

                if let Err(error) = result {
                    let _ = sender.send(Err(error)).await;
                    return;
                }

                if progress.is_stage_finished(Stage::FileEvents) {
                    authors_sender.send_if_modified(|authors| {
                        !std::mem::replace(&mut authors.is_final, true)
                    });
                }

                if has_loaded || !progress.is_loaded() {
                    continue;
                }

                has_loaded = true;
                Update::Loaded
            }
        };

        match &update {
            Update::FileEvent(event) => {
                authors_sender.send_if_modified(|authors| authors.public_keys.insert(event.pubkey));
            }
            Update::Profile(event) if Some(event.pubkey) == user_to_fetch => {
                has_found_user_profile = true;
            }
            _ => {}
        }

        if sender.send(Ok(update)).await.is_err() {
            return;
        }
    }
}

async fn fetch_from_relay(fetch: RelayFetch) {
    let relay = relay_connector::connect(&fetch.client, &fetch.url).await;

    let run_stage = async |stage: Stage| {
        if !fetch.stages.contains(&stage) {
            return;
        }

        let has_answered = match &relay {
            Some(relay) => match stage {
                Stage::FileEvents => stream_file_events(relay, &fetch).await,
                Stage::CurrentUser => stream_current_user(relay, &fetch).await,
                Stage::Profiles => stream_profiles(relay, &fetch).await,
            },
            None => false,
        };

        let _ = fetch.sender.send(RelayMessage::Finished {
            stage,
            has_answered,
        });
    };

    tokio::join!(
        run_stage(Stage::FileEvents),
        run_stage(Stage::CurrentUser),
        run_stage(Stage::Profiles)
    );
}

async fn stream_file_events(relay: &Relay, fetch: &RelayFetch) -> bool {
    let filter = Filter::new()
        .kind(Kind::FileMetadata)
        .custom_tag(SingleLetterTag::LOWERCASE_X, &fetch.hash);

    stream(relay, vec![filter], fetch, Update::FileEvent).await
}

async fn stream_current_user(relay: &Relay, fetch: &RelayFetch) -> bool {
    let Some(public_key) = fetch.user_to_fetch else {
        return true;
    };

    let filters = [Kind::Metadata, Kind::ContactList]
        .map(|kind| Filter::new().kind(kind).author(public_key).limit(1));

    stream(relay, filters.to_vec(), fetch, |event| {
        if event.kind == Kind::Metadata {
            Update::Profile(event)
        } else {
            Update::FollowList(event)
        }
    })
    .await
}

// profiles are requested in batches as verifiers are found, one request at a
// time, so a relay isn't flooded with a subscription per verifier. profiles
// never fail a fetch, so this always counts as answered.
async fn stream_profiles(relay: &Relay, fetch: &RelayFetch) -> bool {
    let mut authors = fetch.authors.clone();
    let mut requested_public_keys = HashSet::new();

    loop {
        let (new_public_keys, is_final) = {
            let authors = authors.borrow_and_update();
            let new_public_keys: Vec<PublicKey> = authors
                .public_keys
                .difference(&requested_public_keys)
                .copied()
                .collect();

            (new_public_keys, authors.is_final)
        };

        if !new_public_keys.is_empty() {
            requested_public_keys.extend(&new_public_keys);
            let filter = Filter::new().kind(Kind::Metadata).authors(new_public_keys);
            stream(relay, vec![filter], fetch, Update::Profile).await;
            continue;
        }

        if is_final || authors.changed().await.is_err() {
            return true;
        }
    }
}

// returns whether the relay answered. nostr-sdk ends the stream the same way on
// eose and on a timeout, so running out the clock counts as no answer, like the
// web page treats a relay that never sends eose. an error means the relay
// closed the subscription.
async fn stream(
    relay: &Relay,
    filters: Vec<Filter>,
    fetch: &RelayFetch,
    to_update: fn(Event) -> Update,
) -> bool {
    let started_at = Instant::now();
    let Ok(mut events) = relay.stream_events(filters).timeout(FETCH_TIMEOUT).await else {
        return false;
    };

    while let Some(result) = events.next().await {
        let Ok(event) = result else {
            return false;
        };

        if fetch
            .sender
            .send(RelayMessage::Update(to_update(event)))
            .is_err()
        {
            return false;
        }
    }

    started_at.elapsed() < FETCH_TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::{
        HASH, build_event, build_file_event, build_follow_list, build_profile,
    };
    use crate::test_relays::{DEAD_RELAY_URL, build_relay_config, relay_url, run_relay};

    mod group_stages_by_url {
        use super::*;

        #[test]
        fn gives_file_relays_file_events_and_user_relays_profiles() {
            let relay_config = build_relay_config(&["wss://file.example"], &["wss://user.example"]);

            let stages_by_url = group_stages_by_url(&relay_config, false);

            assert_eq!(stages_by_url.len(), 2);
            assert!(stages_by_url[&relay_url("wss://file.example")] == [Stage::FileEvents]);
            assert!(stages_by_url[&relay_url("wss://user.example")] == [Stage::Profiles]);
        }

        #[test]
        fn has_user_relays_fetch_the_current_user_when_there_is_one() {
            let relay_config = build_relay_config(&["wss://file.example"], &["wss://user.example"]);

            let stages_by_url = group_stages_by_url(&relay_config, true);

            assert!(
                stages_by_url[&relay_url("wss://user.example")]
                    == [Stage::Profiles, Stage::CurrentUser]
            );
        }

        #[test]
        fn gives_a_repeated_url_each_stage_once() {
            let relay_config = build_relay_config(
                &["wss://file.example", "wss://file.example"],
                &["wss://user.example", "wss://user.example"],
            );

            let stages_by_url = group_stages_by_url(&relay_config, true);

            assert!(stages_by_url[&relay_url("wss://file.example")] == [Stage::FileEvents]);
            assert!(
                stages_by_url[&relay_url("wss://user.example")]
                    == [Stage::Profiles, Stage::CurrentUser]
            );
        }

        #[test]
        fn gives_a_relay_in_both_lists_every_stage() {
            let relay_config = build_relay_config(&["wss://both.example"], &["wss://both.example"]);

            let stages_by_url = group_stages_by_url(&relay_config, true);

            assert_eq!(stages_by_url.len(), 1);
            assert!(
                stages_by_url[&relay_url("wss://both.example")]
                    == [Stage::FileEvents, Stage::Profiles, Stage::CurrentUser]
            );
        }
    }

    mod progress {
        use super::*;

        fn build_progress(has_user: bool) -> Progress {
            build_progress_requiring(has_user, UserRequirement::Required)
        }

        fn build_progress_requiring(has_user: bool, user_requirement: UserRequirement) -> Progress {
            let relay_config = build_relay_config(
                &["wss://file-a.example", "wss://file-b.example"],
                &["wss://user-a.example", "wss://user-b.example"],
            );

            Progress::new(
                &group_stages_by_url(&relay_config, has_user),
                user_requirement,
            )
        }

        #[test]
        fn doesnt_fail_on_user_relays_when_the_user_isnt_required() {
            let mut progress = build_progress_requiring(true, UserRequirement::NotRequired);

            assert!(progress.finish(Stage::CurrentUser, false).is_ok());
            assert!(progress.finish(Stage::CurrentUser, false).is_ok());
        }

        #[test]
        fn waits_for_every_file_relay_before_failing() {
            let mut progress = build_progress(false);

            assert!(progress.finish(Stage::FileEvents, false).is_ok());
            assert!(matches!(
                progress.finish(Stage::FileEvents, false),
                Err(HasheusError::FileRelaysUnreachable)
            ));
        }

        #[test]
        fn doesnt_fail_once_a_file_relay_answers() {
            let mut progress = build_progress(false);

            assert!(progress.finish(Stage::FileEvents, true).is_ok());
            assert!(progress.finish(Stage::FileEvents, false).is_ok());
        }

        #[test]
        fn fails_when_no_user_relay_returns_the_current_user() {
            let mut progress = build_progress(true);

            assert!(progress.finish(Stage::CurrentUser, false).is_ok());
            assert!(matches!(
                progress.finish(Stage::CurrentUser, false),
                Err(HasheusError::UserRelaysUnreachable)
            ));
        }

        #[test]
        fn never_fails_on_profiles() {
            let mut progress = build_progress(false);

            assert!(progress.finish(Stage::Profiles, false).is_ok());
            assert!(progress.finish(Stage::Profiles, false).is_ok());
        }

        #[test]
        fn loads_once_file_events_and_the_current_user_are_in() {
            let mut progress = build_progress(true);

            for stage in [Stage::FileEvents, Stage::FileEvents, Stage::CurrentUser] {
                progress.finish(stage, true).unwrap();
                assert!(!progress.is_loaded());
            }

            progress.finish(Stage::CurrentUser, true).unwrap();

            assert!(progress.is_loaded());
        }

        #[test]
        fn doesnt_wait_for_profiles_to_load() {
            let mut progress = build_progress(false);

            progress.finish(Stage::FileEvents, true).unwrap();
            progress.finish(Stage::FileEvents, true).unwrap();

            assert!(progress.is_loaded());
            assert!(!progress.is_stage_finished(Stage::Profiles));
        }
    }

    mod fetch {
        use super::*;

        // what an update was, so a run can be compared as a whole.
        #[derive(Debug, PartialEq)]
        enum Received {
            FileEvent(PublicKey),
            Profile(PublicKey),
            FollowList(PublicKey),
            Loaded,
        }

        async fn fetch_all(
            user_to_fetch: Option<PublicKey>,
            relay_config: RelayConfig,
        ) -> Result<Vec<Received>, HasheusError> {
            let mut updates = fetch(
                HASH,
                user_to_fetch,
                &relay_config,
                UserRequirement::Required,
            );
            let mut received = Vec::new();

            while let Some(update) = updates.recv().await {
                received.push(match update? {
                    Update::FileEvent(event) => Received::FileEvent(event.pubkey),
                    Update::Profile(event) => Received::Profile(event.pubkey),
                    Update::FollowList(event) => Received::FollowList(event.pubkey),
                    Update::Loaded => Received::Loaded,
                });
            }

            Ok(received)
        }

        fn position(received: &[Received], expected: &Received) -> usize {
            received
                .iter()
                .position(|update| update == expected)
                .unwrap_or_else(|| panic!("{expected:?} wasn't received in {received:?}"))
        }

        #[tokio::test]
        async fn streams_verifications_profiles_and_the_current_user() {
            let (alice, bob, viewer) = (Keys::generate(), Keys::generate(), Keys::generate());
            let (_relay, url) = run_relay(&[
                build_file_event(&alice, 1, None),
                build_file_event(&bob, 2, None),
                build_profile(&alice, 1, "alice"),
                build_profile(&bob, 1, "bob"),
                build_profile(&viewer, 1, "viewer"),
                build_follow_list(&viewer, 1, &[alice.public_key()]),
            ])
            .await;

            let received = fetch_all(
                Some(viewer.public_key()),
                build_relay_config(&[&url], &[&url]),
            )
            .await
            .unwrap();

            let loaded = position(&received, &Received::Loaded);
            for expected in [
                Received::FileEvent(alice.public_key()),
                Received::FileEvent(bob.public_key()),
                Received::Profile(viewer.public_key()),
                Received::FollowList(viewer.public_key()),
            ] {
                assert!(position(&received, &expected) < loaded);
            }
            position(&received, &Received::Profile(alice.public_key()));
            position(&received, &Received::Profile(bob.public_key()));
        }

        #[tokio::test]
        async fn ignores_verifications_of_other_files() {
            let alice = Keys::generate();
            let other_hash = Tag::parse(["x", &"0".repeat(64)]).unwrap();
            let other_file = build_event(&alice, Kind::FileMetadata, 1, "", vec![other_hash]);
            let (_relay, url) = run_relay(&[other_file]).await;

            let received = fetch_all(None, build_relay_config(&[&url], &[&url]))
                .await
                .unwrap();

            assert_eq!(received, [Received::Loaded]);
        }

        #[tokio::test]
        async fn finishes_with_a_repeated_relay_url() {
            let alice = Keys::generate();
            let (_relay, url) = run_relay(&[build_file_event(&alice, 1, None)]).await;

            let received = fetch_all(None, build_relay_config(&[&url, &url], &[&url, &url]))
                .await
                .unwrap();

            assert_eq!(
                received,
                [Received::FileEvent(alice.public_key()), Received::Loaded]
            );
        }

        #[tokio::test]
        async fn fails_when_no_file_relay_connects() {
            let (_relay, url) = run_relay(&[]).await;

            let result = fetch_all(None, build_relay_config(&[DEAD_RELAY_URL], &[&url])).await;

            assert_eq!(
                result.unwrap_err().to_string(),
                "unable to connect to hasheus server"
            );
        }

        #[tokio::test]
        async fn doesnt_need_user_relays_without_a_user() {
            let alice = Keys::generate();
            let (_relay, url) = run_relay(&[build_file_event(&alice, 1, None)]).await;

            let received = fetch_all(None, build_relay_config(&[&url], &[DEAD_RELAY_URL]))
                .await
                .unwrap();

            assert_eq!(
                received,
                [Received::FileEvent(alice.public_key()), Received::Loaded]
            );
        }

        #[tokio::test]
        async fn fails_when_no_user_relay_connects_with_a_user() {
            let (_relay, url) = run_relay(&[]).await;

            let result = fetch_all(
                Some(Keys::generate().public_key()),
                build_relay_config(&[&url], &[DEAD_RELAY_URL]),
            )
            .await;

            assert_eq!(
                result.unwrap_err().to_string(),
                "unable to connect to any user relay"
            );
        }

        #[tokio::test]
        async fn fails_when_the_user_has_no_profile() {
            let viewer = Keys::generate();
            let (_relay, url) = run_relay(&[build_follow_list(&viewer, 1, &[])]).await;

            let result = fetch_all(
                Some(viewer.public_key()),
                build_relay_config(&[&url], &[&url]),
            )
            .await;

            assert_eq!(
                result.unwrap_err().to_string(),
                "unable to find your profile"
            );
        }
    }
}
