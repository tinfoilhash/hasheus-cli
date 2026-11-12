use crate::current_user::CurrentUser;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// matches the web page, which refreshes your profile and follow list once
// they're an hour old.
const REFRESH_AFTER_SECONDS: u64 = 60 * 60;

#[derive(Serialize, Deserialize)]
pub struct CachedUser {
    requested_at: u64,
    pub profile_event: Option<Event>,
    pub follow_list_event: Option<Event>,
    // whether it was requested within the hour. a stale one is still read, so
    // its follow list can keep an older one a relay returns from replacing it.
    #[serde(skip)]
    pub is_fresh: bool,
}

// the cache only saves fetching, so one that can't be read or written is
// treated as empty rather than failing the verification.
pub fn read(public_key: &PublicKey) -> Option<CachedUser> {
    read_from(&cache_dir()?, public_key, Timestamp::now())
}

pub fn write(current_user: &CurrentUser) {
    if let Some(directory) = cache_dir() {
        write_to(&directory, current_user, Timestamp::now());
    }
}

fn read_from(directory: &Path, public_key: &PublicKey, now: Timestamp) -> Option<CachedUser> {
    let json = fs::read_to_string(cache_path(directory, public_key)).ok()?;
    let mut cached_user: CachedUser = serde_json::from_str(&json).ok()?;
    cached_user.is_fresh =
        now.as_secs().saturating_sub(cached_user.requested_at) < REFRESH_AFTER_SECONDS;
    let is_valid = is_valid_event(
        cached_user.profile_event.as_ref(),
        public_key,
        Kind::Metadata,
    ) && is_valid_event(
        cached_user.follow_list_event.as_ref(),
        public_key,
        Kind::ContactList,
    );

    // a profile is required to connect, so a cache without one is refetched.
    let has_profile = cached_user.profile_event.is_some();

    (is_valid && has_profile).then_some(cached_user)
}

// the file could have been edited, so its events are checked like a relay's.
fn is_valid_event(event: Option<&Event>, public_key: &PublicKey, kind: Kind) -> bool {
    event.is_none_or(|event| {
        event.pubkey == *public_key && event.kind == kind && event.verify().is_ok()
    })
}

fn write_to(directory: &Path, current_user: &CurrentUser, now: Timestamp) {
    let cached_user = CachedUser {
        requested_at: now.as_secs(),
        profile_event: current_user.profile_event().cloned(),
        follow_list_event: current_user.follow_list_event().cloned(),
        is_fresh: true,
    };
    let Ok(json) = serde_json::to_string(&cached_user) else {
        return;
    };

    let _ = fs::create_dir_all(directory)
        .and_then(|()| fs::write(cache_path(directory, &current_user.public_key), json));
}

fn cache_path(directory: &Path, public_key: &PublicKey) -> PathBuf {
    directory.join(format!("{}.json", public_key.to_hex()))
}

// follows each platform's convention for cache files, with XDG_CACHE_HOME
// taking precedence wherever it's set.
fn cache_dir() -> Option<PathBuf> {
    let base = match env::var_os("XDG_CACHE_HOME").filter(|directory| !directory.is_empty()) {
        Some(directory) => PathBuf::from(directory),
        None if cfg!(target_os = "macos") => env::home_dir()?.join("Library/Caches"),
        None if cfg!(windows) => PathBuf::from(env::var_os("LOCALAPPDATA")?),
        None => env::home_dir()?.join(".cache"),
    };

    Some(base.join("hasheus").join("users"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_event_builders::{build_follow_list, build_profile};

    const NOW: u64 = 1_800_000_000;

    struct TestCache {
        directory: PathBuf,
        keys: Keys,
    }

    impl TestCache {
        fn new() -> Self {
            let keys = Keys::generate();
            let directory =
                env::temp_dir().join(format!("hasheus_cli_cache_test_{}", keys.public_key()));

            Self { directory, keys }
        }

        fn build_current_user(&self) -> CurrentUser {
            let mut current_user = CurrentUser::new(self.keys.public_key());
            current_user.set_profile(build_profile(&self.keys, 1, "alice"));
            current_user.set_follow_list(build_follow_list(&self.keys, 1, &[]));

            current_user
        }

        // writes the file directly, to test events a current user wouldn't keep.
        fn write_cached_user(&self, cached_user: &CachedUser) {
            fs::create_dir_all(&self.directory).unwrap();
            fs::write(
                cache_path(&self.directory, &self.keys.public_key()),
                serde_json::to_string(cached_user).unwrap(),
            )
            .unwrap();
        }

        fn read(&self, now: u64) -> Option<CachedUser> {
            read_from(
                &self.directory,
                &self.keys.public_key(),
                Timestamp::from_secs(now),
            )
        }
    }

    impl Drop for TestCache {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn reads_back_what_it_wrote() {
        let cache = TestCache::new();
        let current_user = cache.build_current_user();

        write_to(&cache.directory, &current_user, Timestamp::from_secs(NOW));
        let cached_user = cache.read(NOW + 60).unwrap();

        assert!(cached_user.is_fresh);
        assert_eq!(
            cached_user.profile_event.as_ref(),
            current_user.profile_event()
        );
        assert_eq!(
            cached_user.follow_list_event.as_ref(),
            current_user.follow_list_event()
        );
    }

    #[test]
    fn reads_a_cache_an_hour_old_as_stale() {
        let cache = TestCache::new();

        write_to(
            &cache.directory,
            &cache.build_current_user(),
            Timestamp::from_secs(NOW),
        );

        assert!(!cache.read(NOW + REFRESH_AFTER_SECONDS).unwrap().is_fresh);
    }

    #[test]
    fn ignores_a_missing_cache() {
        assert!(TestCache::new().read(NOW).is_none());
    }

    #[test]
    fn ignores_a_cache_that_cant_be_parsed() {
        let cache = TestCache::new();
        fs::create_dir_all(&cache.directory).unwrap();
        fs::write(
            cache_path(&cache.directory, &cache.keys.public_key()),
            "not json",
        )
        .unwrap();

        assert!(cache.read(NOW).is_none());
    }

    #[test]
    fn ignores_a_cache_without_a_profile() {
        let cache = TestCache::new();
        let mut current_user = CurrentUser::new(cache.keys.public_key());
        current_user.set_follow_list(build_follow_list(&cache.keys, 1, &[]));

        write_to(&cache.directory, &current_user, Timestamp::from_secs(NOW));

        assert!(cache.read(NOW).is_none());
    }

    #[test]
    fn ignores_events_from_someone_else() {
        let cache = TestCache::new();

        cache.write_cached_user(&CachedUser {
            requested_at: NOW,
            profile_event: Some(build_profile(&cache.keys, 1, "alice")),
            follow_list_event: Some(build_follow_list(&Keys::generate(), 1, &[])),
            is_fresh: true,
        });

        assert!(cache.read(NOW).is_none());
    }

    #[test]
    fn ignores_an_edited_event() {
        let cache = TestCache::new();
        write_to(
            &cache.directory,
            &cache.build_current_user(),
            Timestamp::from_secs(NOW),
        );
        let path = cache_path(&cache.directory, &cache.keys.public_key());
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("alice", "mallory");
        fs::write(&path, edited).unwrap();

        assert!(cache.read(NOW).is_none());
    }
}
