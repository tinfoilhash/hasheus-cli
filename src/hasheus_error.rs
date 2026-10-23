use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum HasheusError {
    ReadFile { path: PathBuf, cause: io::Error },
    InvalidUser { input_name: &'static str },
    InvalidRelayURL { url: String },
    FileRelaysUnreachable,
    UserRelaysUnreachable,
    UserProfileNotFound,
    SecretKeyNotNeeded { input_name: &'static str },
    SecretKeyRequired,
    InvalidSecretKey,
    PasswordRequired,
    InvalidPassword,
    InvalidURL,
    InvalidDescription,
    InvalidMIMEType,
    AlreadyVerified,
    PublishRejected { message: String },
    ConfirmationRequired,
    NotPublished,
    Interrupted,
}

impl HasheusError {
    // stable identifiers for the json output, so scripts can branch on them
    // rather than on the message.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ReadFile { .. } => "read_file",
            Self::InvalidUser { .. } => "invalid_user",
            Self::InvalidRelayURL { .. } => "invalid_relay_url",
            Self::FileRelaysUnreachable => "file_relays_unreachable",
            Self::UserRelaysUnreachable => "user_relays_unreachable",
            Self::UserProfileNotFound => "user_profile_not_found",
            Self::SecretKeyNotNeeded { .. } => "secret_key_not_needed",
            Self::SecretKeyRequired => "secret_key_required",
            Self::InvalidSecretKey => "invalid_secret_key",
            Self::PasswordRequired => "password_required",
            Self::InvalidPassword => "invalid_password",
            Self::InvalidURL => "invalid_url",
            Self::InvalidDescription => "invalid_description",
            Self::InvalidMIMEType => "invalid_mime_type",
            Self::AlreadyVerified => "already_verified",
            Self::PublishRejected { .. } => "publish_rejected",
            Self::ConfirmationRequired => "confirmation_required",
            Self::NotPublished => "not_published",
            // never printed: like the text output, ctrl-c exits without any.
            Self::Interrupted => "interrupted",
        }
    }
}

impl fmt::Display for HasheusError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ReadFile { path, cause } => {
                write!(formatter, "failed to read {}: {cause}", path.display())
            }
            // the value itself is never included, since it could be a mangled
            // secret key.
            Self::InvalidUser { input_name } => {
                write!(
                    formatter,
                    "{input_name} isn't a valid npub or hex public key"
                )
            }
            Self::InvalidRelayURL { url } => write!(formatter, "{url} isn't a valid relay url"),
            Self::FileRelaysUnreachable => write!(formatter, "unable to connect to hasheus server"),
            Self::UserRelaysUnreachable => write!(formatter, "unable to connect to any user relay"),
            Self::UserProfileNotFound => write!(formatter, "unable to find your profile"),
            Self::SecretKeyNotNeeded { input_name } => write!(
                formatter,
                "verifying only needs your public key: use your npub for {input_name}, not your secret key"
            ),
            Self::SecretKeyRequired => write!(
                formatter,
                "publishing needs your secret key: pass --user <nsec>, set HASHEUS_SECRET_KEY, or run it in a terminal to be asked"
            ),
            // the secret itself is never included, so it can't end up in logs.
            Self::InvalidSecretKey => {
                write!(formatter, "the secret key isn't a valid nsec or ncryptsec")
            }
            Self::PasswordRequired => write!(
                formatter,
                "an ncryptsec needs its password, which can only be asked for in a terminal"
            ),
            Self::InvalidPassword => write!(formatter, "the password doesn't unlock the ncryptsec"),
            Self::InvalidURL => write!(
                formatter,
                "the url must be an http or https url of at most 1,000 characters"
            ),
            Self::InvalidDescription => {
                write!(formatter, "the description must be 1 to 1,000 characters")
            }
            Self::InvalidMIMEType => write!(
                formatter,
                "the mime type must be a type and subtype, like application/zip"
            ),
            Self::AlreadyVerified => write!(formatter, "you've already verified this file"),
            Self::PublishRejected { message } => {
                write!(
                    formatter,
                    "the hasheus server rejected the verification: {message}"
                )
            }
            Self::ConfirmationRequired => write!(
                formatter,
                "publishing needs confirmation: pass --yes to publish without being asked"
            ),
            Self::NotPublished => write!(formatter, "nothing was published"),
            Self::Interrupted => write!(formatter, "interrupted"),
        }
    }
}

impl Error for HasheusError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadFile { cause, .. } => Some(cause),
            _ => None,
        }
    }
}
