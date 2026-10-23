use crate::current_user::CurrentUser;
use crate::verification_fetcher::Update;
use nostr_sdk::prelude::*;
use std::collections::HashMap;

pub struct Verifications {
    pub file_events: Vec<Event>,
    profiles: HashMap<PublicKey, Metadata>,
    pub current_user: Option<CurrentUser>,
    // like the web page, the verdict and empty values wait for the file events
    // and your profile and follow list, but not other profiles, which only
    // change names.
    pub is_pending: bool,
    profile_created_ats: HashMap<PublicKey, Timestamp>,
}

impl Verifications {
    pub fn new(user_public_key: Option<PublicKey>) -> Self {
        Self {
            file_events: Vec::new(),
            profiles: HashMap::new(),
            current_user: user_public_key.map(CurrentUser::new),
            is_pending: true,
            profile_created_ats: HashMap::new(),
        }
    }

    pub fn profile(&self, public_key: &PublicKey) -> Option<&Metadata> {
        self.profiles.get(public_key)
    }

    // returns whether anything changed, so an update that changes nothing
    // doesn't cause a redraw.
    pub fn apply_update(&mut self, update: Update) -> bool {
        match update {
            Update::FileEvent(event) => self.add_file_event(event),
            Update::Profile(event) => self.add_profile(event),
            Update::FollowList(event) => self.add_follow_list(event),
            Update::Loaded => std::mem::replace(&mut self.is_pending, false),
        }
    }

    // relays share events, so the same one can arrive more than once. events
    // are kept sorted oldest first.
    fn add_file_event(&mut self, event: Event) -> bool {
        let index = self.file_events.partition_point(|existing| {
            (existing.created_at, existing.id) < (event.created_at, event.id)
        });

        if self
            .file_events
            .get(index)
            .is_some_and(|existing| existing.id == event.id)
        {
            return false;
        }

        self.file_events.insert(index, event);

        true
    }

    // relays can have different versions of a profile, so only the newest is
    // kept. anyone can publish a profile, so ones that can't be parsed are
    // skipped.
    fn add_profile(&mut self, event: Event) -> bool {
        let is_newest = self
            .profile_created_ats
            .get(&event.pubkey)
            .is_none_or(|created_at| *created_at < event.created_at);

        if !is_newest {
            return false;
        }

        let Ok(metadata) = Metadata::try_from(&event) else {
            return false;
        };

        self.profile_created_ats
            .insert(event.pubkey, event.created_at);
        self.profiles.insert(event.pubkey, metadata);

        if let Some(current_user) = self.current_user.as_mut() {
            current_user.set_profile(event);
        }

        true
    }

    fn add_follow_list(&mut self, event: Event) -> bool {
        self.current_user
            .as_mut()
            .is_some_and(|current_user| current_user.set_follow_list(event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::{
        build_event, build_file_event, build_follow_list, build_profile,
    };

    #[test]
    fn keeps_file_events_sorted_oldest_first() {
        let keys = Keys::generate();
        let mut verifications = Verifications::new(None);

        for created_at in [3, 1, 2] {
            verifications
                .apply_update(Update::FileEvent(build_file_event(&keys, created_at, None)));
        }

        let created_ats: Vec<u64> = verifications
            .file_events
            .iter()
            .map(|event| event.created_at.as_secs())
            .collect();
        assert_eq!(created_ats, [1, 2, 3]);
    }

    #[test]
    fn ignores_a_file_event_it_already_has() {
        let event = build_file_event(&Keys::generate(), 1, None);
        let mut verifications = Verifications::new(None);

        assert!(verifications.apply_update(Update::FileEvent(event.clone())));
        assert!(!verifications.apply_update(Update::FileEvent(event)));
        assert_eq!(verifications.file_events.len(), 1);
    }

    #[test]
    fn keeps_the_newest_profile() {
        let keys = Keys::generate();
        let mut verifications = Verifications::new(None);

        verifications.apply_update(Update::Profile(build_profile(&keys, 2, "newer")));
        let has_changed =
            verifications.apply_update(Update::Profile(build_profile(&keys, 1, "older")));

        assert!(!has_changed);
        assert_eq!(
            verifications.profiles[&keys.public_key()].name.as_deref(),
            Some("newer")
        );
    }

    #[test]
    fn skips_a_profile_that_cant_be_parsed() {
        let keys = Keys::generate();
        let mut verifications = Verifications::new(None);

        verifications.apply_update(Update::Profile(build_profile(&keys, 1, "valid")));
        let invalid = build_event(&keys, Kind::Metadata, 2, "not json", Vec::new());

        assert!(!verifications.apply_update(Update::Profile(invalid)));
        assert_eq!(
            verifications.profiles[&keys.public_key()].name.as_deref(),
            Some("valid")
        );
    }

    #[test]
    fn applies_the_follow_list_to_the_current_user() {
        let keys = Keys::generate();
        let followed = Keys::generate().public_key();
        let mut verifications = Verifications::new(Some(keys.public_key()));

        let has_changed = verifications.apply_update(Update::FollowList(build_follow_list(
            &keys,
            1,
            &[followed],
        )));

        assert!(has_changed);
        assert!(verifications.current_user.unwrap().has_trusted(&followed));
    }

    #[test]
    fn keeps_the_current_users_profile_event() {
        let keys = Keys::generate();
        let profile = build_profile(&keys, 1, "alice");
        let mut verifications = Verifications::new(Some(keys.public_key()));

        verifications.apply_update(Update::Profile(profile.clone()));

        let current_user = verifications.current_user.unwrap();
        assert_eq!(current_user.profile_event(), Some(&profile));
    }

    #[test]
    fn stops_pending_once_loaded() {
        let mut verifications = Verifications::new(None);

        assert!(verifications.apply_update(Update::Loaded));
        assert!(!verifications.is_pending);
        assert!(!verifications.apply_update(Update::Loaded));
    }
}
