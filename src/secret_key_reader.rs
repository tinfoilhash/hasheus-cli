use crate::hasheus_error::HasheusError;
use nostr_sdk::prelude::*;
use std::env;
use std::io;

// prefixed like the other env vars, so it's only set on purpose for hasheus,
// since verify also uses it to know who you are.
const SECRET_KEY_ENV_VAR: &str = "HASHEUS_SECRET_KEY";

// the key comes from --user, then HASHEUS_SECRET_KEY, then a hidden prompt, which
// is only shown when there's a terminal to answer it.
pub fn read(user: Option<&str>, can_prompt: bool) -> Result<Keys, HasheusError> {
    read_with(user, read_env_var(), can_prompt, &mut prompt_hidden)
}

// without --user or HASHEUS_PUBLIC_KEY, verify uses the key publishing would sign
// with, to check your own web of trust. there's no prompt for the key itself here.
pub fn read_from_env(can_prompt: bool) -> Result<Option<Keys>, HasheusError> {
    read_from_env_with(read_env_var(), can_prompt, &mut prompt_hidden)
}

fn read_env_var() -> Option<String> {
    env::var(SECRET_KEY_ENV_VAR).ok()
}

fn prompt_hidden(prompt: &str) -> io::Result<String> {
    rpassword::prompt_password(prompt)
}

fn read_from_env_with(
    env_value: Option<String>,
    can_prompt: bool,
    prompt: &mut dyn FnMut(&str) -> io::Result<String>,
) -> Result<Option<Keys>, HasheusError> {
    // a blank value counts as unset, rather than falling through to a prompt.
    if env_value
        .as_deref()
        .is_none_or(|secret| secret.trim().is_empty())
    {
        return Ok(None);
    }

    read_with(None, env_value, can_prompt, prompt).map(Some)
}

fn read_with(
    user: Option<&str>,
    env_value: Option<String>,
    can_prompt: bool,
    prompt: &mut dyn FnMut(&str) -> io::Result<String>,
) -> Result<Keys, HasheusError> {
    let configured = user
        .map(str::to_string)
        .or(env_value)
        .filter(|secret| !secret.trim().is_empty());

    let secret = match configured {
        Some(secret) => secret,
        None if can_prompt => prompt("secret key (nsec or ncryptsec): ")
            .map_err(|_| HasheusError::SecretKeyRequired)?,
        None => return Err(HasheusError::SecretKeyRequired),
    };

    parse(secret.trim(), can_prompt, prompt)
}

// a hex key isn't accepted, since it could just as well be a public key and
// would silently sign as someone else.
fn parse(
    secret: &str,
    can_prompt: bool,
    prompt: &mut dyn FnMut(&str) -> io::Result<String>,
) -> Result<Keys, HasheusError> {
    if let Ok(encrypted_secret_key) = EncryptedSecretKey::from_bech32(secret) {
        if !can_prompt {
            return Err(HasheusError::PasswordRequired);
        }

        let password = prompt("password: ").map_err(|_| HasheusError::PasswordRequired)?;
        let secret_key = encrypted_secret_key
            .decrypt(&password)
            .map_err(|_| HasheusError::InvalidPassword)?;

        return Ok(Keys::new(secret_key));
    }

    SecretKey::from_bech32(secret)
        .map(Keys::new)
        .map_err(|_| HasheusError::InvalidSecretKey)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "correct horse";

    fn nsec(keys: &Keys) -> String {
        keys.secret_key().to_bech32().unwrap()
    }

    // a low scrypt cost keeps the tests fast.
    fn ncryptsec(keys: &Keys) -> String {
        EncryptedSecretKey::new(keys.secret_key(), PASSWORD, 1, KeySecurity::Unknown)
            .unwrap()
            .to_bech32()
            .unwrap()
    }

    // answers each prompt in order, and records what was asked.
    fn answer(answers: &[&str], asked: &mut Vec<String>) -> impl FnMut(&str) -> io::Result<String> {
        let mut answers: Vec<String> = answers
            .iter()
            .rev()
            .map(|answer| answer.to_string())
            .collect();

        move |prompt| {
            asked.push(prompt.to_string());
            answers
                .pop()
                .ok_or_else(|| io::Error::other("no answer left"))
        }
    }

    #[test]
    fn prefers_the_user_option() {
        let (user, env) = (Keys::generate(), Keys::generate());

        let keys = read_with(
            Some(&nsec(&user)),
            Some(nsec(&env)),
            false,
            &mut |_| unreachable!(),
        )
        .unwrap();

        assert_eq!(keys.public_key(), user.public_key());
    }

    #[test]
    fn falls_back_to_the_env_var() {
        let env = Keys::generate();

        let keys = read_with(None, Some(nsec(&env)), false, &mut |_| unreachable!()).unwrap();

        assert_eq!(keys.public_key(), env.public_key());
    }

    #[test]
    fn prompts_without_a_configured_key() {
        let prompted = Keys::generate();
        let mut asked = Vec::new();

        let keys = read_with(
            None,
            None,
            true,
            &mut answer(&[&nsec(&prompted)], &mut asked),
        )
        .unwrap();

        assert_eq!(keys.public_key(), prompted.public_key());
        assert_eq!(asked, ["secret key (nsec or ncryptsec): "]);
    }

    #[test]
    fn needs_a_key_when_it_cant_prompt() {
        let error =
            read_with(None, Some(" ".to_string()), false, &mut |_| unreachable!()).unwrap_err();

        assert!(matches!(error, HasheusError::SecretKeyRequired));
    }

    #[test]
    fn decrypts_an_ncryptsec_with_the_prompted_password() {
        let keys = Keys::generate();
        let mut asked = Vec::new();

        let decrypted = read_with(
            Some(&ncryptsec(&keys)),
            None,
            true,
            &mut answer(&[PASSWORD], &mut asked),
        )
        .unwrap();

        assert_eq!(decrypted.public_key(), keys.public_key());
        assert_eq!(asked, ["password: "]);
    }

    #[test]
    fn rejects_a_wrong_password() {
        let mut asked = Vec::new();

        let error = read_with(
            Some(&ncryptsec(&Keys::generate())),
            None,
            true,
            &mut answer(&["wrong"], &mut asked),
        )
        .unwrap_err();

        assert!(matches!(error, HasheusError::InvalidPassword));
    }

    #[test]
    fn needs_a_terminal_for_an_ncryptsec_password() {
        let error = read_with(
            None,
            Some(ncryptsec(&Keys::generate())),
            false,
            &mut |_| unreachable!(),
        )
        .unwrap_err();

        assert!(matches!(error, HasheusError::PasswordRequired));
    }

    #[test]
    fn reads_nothing_from_an_unset_or_blank_env_var() {
        for env_value in [None, Some(" ".to_string())] {
            let keys = read_from_env_with(env_value, true, &mut |_| unreachable!()).unwrap();

            assert!(keys.is_none());
        }
    }

    #[test]
    fn reads_the_env_var() {
        let env = Keys::generate();

        let keys = read_from_env_with(Some(nsec(&env)), false, &mut |_| unreachable!())
            .unwrap()
            .unwrap();

        assert_eq!(keys.public_key(), env.public_key());
    }

    #[test]
    fn asks_for_the_env_vars_ncryptsec_password() {
        let env = Keys::generate();
        let mut asked = Vec::new();

        let keys = read_from_env_with(
            Some(ncryptsec(&env)),
            true,
            &mut answer(&[PASSWORD], &mut asked),
        )
        .unwrap()
        .unwrap();

        assert_eq!(keys.public_key(), env.public_key());
        assert_eq!(asked, ["password: "]);
    }

    #[test]
    fn rejects_an_invalid_env_var() {
        let error = read_from_env_with(
            Some("nsec1nope".to_string()),
            false,
            &mut |_| unreachable!(),
        )
        .unwrap_err();

        assert!(matches!(error, HasheusError::InvalidSecretKey));
    }

    #[test]
    fn rejects_a_public_key() {
        let npub = Keys::generate().public_key().to_bech32().unwrap();

        let error = read_with(Some(&npub), None, false, &mut |_| unreachable!()).unwrap_err();

        assert!(matches!(error, HasheusError::InvalidSecretKey));
    }

    #[test]
    fn rejects_a_hex_key() {
        let hex = Keys::generate().secret_key().to_secret_hex();

        let error = read_with(Some(&hex), None, false, &mut |_| unreachable!()).unwrap_err();

        assert!(matches!(error, HasheusError::InvalidSecretKey));
    }
}
