use crate::hasheus_error::HasheusError;
use crate::secret_key_reader;
use nostr_sdk::prelude::*;
use std::env;

const USER_OPTION: &str = "--user";
const PUBLIC_KEY_ENV_VAR: &str = "HASHEUS_PUBLIC_KEY";

// the user comes from --user, then HASHEUS_PUBLIC_KEY, then the public key of
// HASHEUS_SECRET_KEY, so verifying doesn't have to unlock a secret key.
pub fn read(user: Option<&str>, can_prompt: bool) -> Result<Option<PublicKey>, HasheusError> {
    read_with(user, env::var(PUBLIC_KEY_ENV_VAR).ok(), || {
        secret_key_reader::read_from_env(can_prompt)
    })
}

// an invalid value fails rather than falling back, since silently switching to
// another web of trust would be worse.
fn read_with(
    user: Option<&str>,
    env_value: Option<String>,
    read_secret_key: impl FnOnce() -> Result<Option<Keys>, HasheusError>,
) -> Result<Option<PublicKey>, HasheusError> {
    if let Some(user) = user {
        return parse(user, USER_OPTION).map(Some);
    }

    // a blank value counts as unset, like HASHEUS_SECRET_KEY.
    if let Some(env_value) = env_value.filter(|value| !value.trim().is_empty()) {
        return parse(&env_value, PUBLIC_KEY_ENV_VAR).map(Some);
    }

    Ok(read_secret_key()?.map(|keys| keys.public_key()))
}

// verifying only needs a public key, so a secret one is turned away rather
// than left in shell history, checked loosely to catch a pasted one.
fn parse(user: &str, input_name: &'static str) -> Result<PublicKey, HasheusError> {
    let user = user.trim();
    let normalized = user.to_lowercase();
    let invalid_user = HasheusError::InvalidUser { input_name };

    if normalized.starts_with("nsec") || normalized.starts_with("ncryptsec") {
        return Err(HasheusError::SecretKeyNotNeeded { input_name });
    }

    match Nip19::from_bech32(user) {
        Ok(Nip19::Pubkey(public_key)) => Ok(public_key),
        Ok(_) => Err(invalid_user),
        Err(_) => PublicKey::from_hex(user).map_err(|_| invalid_user),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npub(keys: &Keys) -> String {
        keys.public_key().to_bech32().unwrap()
    }

    fn nsec(keys: &Keys) -> String {
        keys.secret_key().to_bech32().unwrap()
    }

    fn no_secret_key() -> Result<Option<Keys>, HasheusError> {
        unreachable!()
    }

    #[test]
    fn prefers_the_user_option() {
        let (user, env) = (Keys::generate(), Keys::generate());

        let public_key = read_with(Some(&npub(&user)), Some(npub(&env)), no_secret_key).unwrap();

        assert_eq!(public_key, Some(user.public_key()));
    }

    #[test]
    fn falls_back_to_the_env_var() {
        let env = Keys::generate();

        let public_key =
            read_with(None, Some(format!(" {}\n", npub(&env))), no_secret_key).unwrap();

        assert_eq!(public_key, Some(env.public_key()));
    }

    #[test]
    fn falls_back_to_the_secret_key_when_the_env_var_is_unset_or_blank() {
        let secret = Keys::generate();

        for env_value in [None, Some(String::new()), Some(" ".to_string())] {
            let public_key = read_with(None, env_value, || Ok(Some(secret.clone()))).unwrap();

            assert_eq!(public_key, Some(secret.public_key()));
        }
    }

    #[test]
    fn has_none_without_any_key() {
        assert_eq!(read_with(None, None, || Ok(None)).unwrap(), None);
    }

    #[test]
    fn fails_on_an_invalid_env_var_rather_than_falling_back() {
        let error = read_with(None, Some("alice".to_string()), no_secret_key).unwrap_err();

        assert_eq!(
            error.to_string(),
            "HASHEUS_PUBLIC_KEY isn't a valid npub or hex public key"
        );
    }

    #[test]
    fn passes_on_a_secret_key_error() {
        let error = read_with(None, None, || Err(HasheusError::InvalidSecretKey)).unwrap_err();

        assert!(matches!(error, HasheusError::InvalidSecretKey));
    }

    #[test]
    fn parses_a_hex_public_key() {
        let keys = Keys::generate();

        assert_eq!(
            parse(&keys.public_key().to_hex(), USER_OPTION).unwrap(),
            keys.public_key()
        );
    }

    #[test]
    fn turns_away_a_secret_key() {
        let keys = Keys::generate();
        let nsec = nsec(&keys);
        // a low scrypt cost keeps the test fast.
        let ncryptsec =
            EncryptedSecretKey::new(keys.secret_key(), "password", 1, KeySecurity::Unknown)
                .unwrap()
                .to_bech32()
                .unwrap();
        let pasted_nsecs = [format!("{nsec}x"), nsec.to_uppercase(), format!(" {nsec} ")];

        for secret in [nsec.clone(), ncryptsec].into_iter().chain(pasted_nsecs) {
            let error = parse(&secret, USER_OPTION).unwrap_err();

            assert!(matches!(error, HasheusError::SecretKeyNotNeeded { .. }));
            assert!(!error.to_string().contains(secret.trim()));
        }
    }

    #[test]
    fn turns_away_a_secret_key_in_the_env_var() {
        let nsec = nsec(&Keys::generate());

        let error = read_with(None, Some(nsec.clone()), no_secret_key).unwrap_err();

        assert_eq!(
            error.to_string(),
            "verifying only needs your public key: use your npub for HASHEUS_PUBLIC_KEY, not your secret key"
        );
        assert!(!error.to_string().contains(&nsec));
    }

    #[test]
    fn never_echoes_an_invalid_user() {
        let mangled_nsec = nsec(&Keys::generate()).replacen("nsec", "n5ec", 1);

        let error = parse(&mangled_nsec, USER_OPTION).unwrap_err();

        assert!(matches!(error, HasheusError::InvalidUser { .. }));
        assert!(!error.to_string().contains(&mangled_nsec[4..]));
    }

    #[test]
    fn fails_on_anything_else() {
        let error = parse("alice", USER_OPTION).unwrap_err();

        assert_eq!(
            error.to_string(),
            "--user isn't a valid npub or hex public key"
        );
    }
}
