use nostr_sdk::prelude::*;
use std::collections::HashSet;

pub struct CurrentUser {
    pub public_key: PublicKey,
    profile_event: Option<Event>,
    follow_list_event: Option<Event>,
    follows: HashSet<PublicKey>,
    stored_follow_list_event: Option<Event>,
}

impl CurrentUser {
    pub fn new(public_key: PublicKey) -> Self {
        Self {
            public_key,
            profile_event: None,
            follow_list_event: None,
            follows: HashSet::new(),
            stored_follow_list_event: None,
        }
    }

    pub fn profile_event(&self) -> Option<&Event> {
        self.profile_event.as_ref()
    }

    pub fn follow_list_event(&self) -> Option<&Event> {
        self.follow_list_event.as_ref()
    }

    // returns whether the profile was kept, since only the newest one is.
    pub fn set_profile(&mut self, event: Event) -> bool {
        if !self.is_newest(self.profile_event.as_ref(), &event, Kind::Metadata) {
            return false;
        }

        self.profile_event = Some(event);

        true
    }

    // returns whether the follow list was kept, since only the newest one is.
    pub fn set_follow_list(&mut self, event: Event) -> bool {
        if !self.is_newest(self.follow_list_event.as_ref(), &event, Kind::ContactList) {
            return false;
        }

        // like the web page, a stored list newer than the one a relay returns is
        // kept, since the relay's may still include a key that's been unfollowed.
        let event = match self.stored_follow_list_event.take() {
            Some(stored) if stored.created_at > event.created_at => stored,
            _ => event,
        };

        self.follows = event.tags.public_keys().collect();
        self.follow_list_event = Some(event);

        true
    }

    // a follow list from an outdated cache. like the web page, it's only used
    // once a relay returns a follow list, so one no relay returns is dropped.
    pub fn set_stored_follow_list(&mut self, event: Event) {
        if self.is_newest(None, &event, Kind::ContactList) {
            self.stored_follow_list_event = Some(event);
        }
    }

    pub fn has_trusted(&self, public_key: &PublicKey) -> bool {
        self.public_key == *public_key || self.follows.contains(public_key)
    }

    // relays can have different versions of a replaceable event, so only the
    // newest one of the right kind from this user is kept.
    fn is_newest(&self, existing: Option<&Event>, event: &Event, kind: Kind) -> bool {
        event.pubkey == self.public_key
            && event.kind == kind
            && existing.is_none_or(|existing| existing.created_at < event.created_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::{build_event, build_follow_list, build_profile};

    #[test]
    fn trusts_themself_and_their_follows() {
        let keys = Keys::generate();
        let followed = Keys::generate().public_key();
        let stranger = Keys::generate().public_key();
        let mut current_user = CurrentUser::new(keys.public_key());

        current_user.set_follow_list(build_follow_list(&keys, 1, &[followed]));

        assert!(current_user.has_trusted(&keys.public_key()));
        assert!(current_user.has_trusted(&followed));
        assert!(!current_user.has_trusted(&stranger));
    }

    #[test]
    fn keeps_the_newest_follow_list() {
        let keys = Keys::generate();
        let newer_follow = Keys::generate().public_key();
        let older_follow = Keys::generate().public_key();
        let mut current_user = CurrentUser::new(keys.public_key());

        assert!(current_user.set_follow_list(build_follow_list(&keys, 2, &[newer_follow])));
        assert!(!current_user.set_follow_list(build_follow_list(&keys, 1, &[older_follow])));

        assert!(current_user.has_trusted(&newer_follow));
        assert!(!current_user.has_trusted(&older_follow));
    }

    #[test]
    fn keeps_a_stored_follow_list_newer_than_a_relays() {
        let keys = Keys::generate();
        let newer_follow = Keys::generate().public_key();
        let older_follow = Keys::generate().public_key();
        let mut current_user = CurrentUser::new(keys.public_key());

        current_user.set_stored_follow_list(build_follow_list(&keys, 2, &[newer_follow]));
        assert!(current_user.set_follow_list(build_follow_list(&keys, 1, &[older_follow])));

        assert!(current_user.has_trusted(&newer_follow));
        assert!(!current_user.has_trusted(&older_follow));
    }

    #[test]
    fn keeps_a_relays_follow_list_newer_than_the_stored_one() {
        let keys = Keys::generate();
        let newer_follow = Keys::generate().public_key();
        let older_follow = Keys::generate().public_key();
        let mut current_user = CurrentUser::new(keys.public_key());

        current_user.set_stored_follow_list(build_follow_list(&keys, 1, &[older_follow]));
        assert!(current_user.set_follow_list(build_follow_list(&keys, 2, &[newer_follow])));

        assert!(current_user.has_trusted(&newer_follow));
        assert!(!current_user.has_trusted(&older_follow));
    }

    #[test]
    fn doesnt_trust_a_stored_follow_list_until_a_relay_returns_one() {
        let keys = Keys::generate();
        let followed = Keys::generate().public_key();
        let mut current_user = CurrentUser::new(keys.public_key());

        current_user.set_stored_follow_list(build_follow_list(&keys, 1, &[followed]));

        assert!(!current_user.has_trusted(&followed));
        assert_eq!(current_user.follow_list_event(), None);
    }

    #[test]
    fn ignores_someone_elses_follow_list() {
        let followed = Keys::generate().public_key();
        let mut current_user = CurrentUser::new(Keys::generate().public_key());

        let follow_list = build_follow_list(&Keys::generate(), 1, &[followed]);

        assert!(!current_user.set_follow_list(follow_list));
        assert!(!current_user.has_trusted(&followed));
    }

    #[test]
    fn keeps_the_newest_profile() {
        let keys = Keys::generate();
        let newer = build_profile(&keys, 2, "newer");
        let older = build_profile(&keys, 1, "older");
        let mut current_user = CurrentUser::new(keys.public_key());

        assert!(current_user.set_profile(newer.clone()));
        assert!(!current_user.set_profile(older));
        assert_eq!(current_user.profile_event(), Some(&newer));
    }

    #[test]
    fn ignores_a_profile_of_the_wrong_kind() {
        let keys = Keys::generate();
        let mut current_user = CurrentUser::new(keys.public_key());

        let note = build_event(&keys, Kind::TextNote, 1, "", Vec::new());

        assert!(!current_user.set_profile(note));
        assert_eq!(current_user.profile_event(), None);
    }
}
