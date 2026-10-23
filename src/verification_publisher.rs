use crate::cli::{OutputFormat, PublishArgs};
use crate::current_user_cache;
use crate::file_event_publisher::{self, FileEventPublisher, FileVerification};
use crate::file_hash_reporter;
use crate::hasheus_error::HasheusError;
use crate::mime_type_guesser;
use crate::name_formatter;
use crate::profile_fetcher;
use crate::relay_config::RelayConfig;
use crate::secret_key_reader;
use crate::text_sanitizer::sanitize;
use crate::verification_reporter;
use nostr_sdk::prelude::*;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use yansi::Paint;

// the web page's form limits.
const MAX_URL_LENGTH: usize = 1000;
const MAX_DESCRIPTION_LENGTH: usize = 1000;
// rfc 6838's limit for a type or subtype.
const MAX_MIME_TYPE_NAME_LENGTH: usize = 127;
pub async fn publish(
    args: PublishArgs,
    relay_config: &RelayConfig,
    output_format: OutputFormat,
) -> Result<(), HasheusError> {
    let url = parse_url(&args.url)?;
    let description = parse_description(args.description.as_deref(), &args.file)?;
    let mime_type = parse_mime_type(args.mime_type.as_deref(), &args.file)?;
    let size = read_size(&args.file)?;
    let can_prompt = output_format.can_prompt();
    // asked before hashing, which can take a while for a large file.
    let keys = secret_key_reader::read(args.user.as_deref(), can_prompt)?;
    let file_hash = file_hash_reporter::hash(&args.file, output_format)?;
    let hash = &file_hash.hash;
    let public_key = keys.public_key();
    // the name is only looked up when there's a preview to show it in.
    let is_previewing = !args.yes && can_prompt;
    let (publisher, name) = tokio::join!(FileEventPublisher::connect(relay_config), async {
        if is_previewing {
            fetch_profile_name(public_key, relay_config).await
        } else {
            None
        }
    });
    let publisher = publisher?;

    // like the web page, which only offers publishing until you've verified it.
    if publisher.has_verified(hash, public_key).await {
        return Err(HasheusError::AlreadyVerified);
    }

    let verification = FileVerification {
        hash,
        mime_type: &mime_type,
        size,
        url: &url,
        description: &description,
    };
    confirm(
        &verification,
        &public_key,
        name.as_deref(),
        args.yes,
        can_prompt,
    )?;
    let event = file_event_publisher::build_event(&verification, &keys);
    publisher.publish(&event).await?;

    verification_reporter::report(
        &file_hash,
        Some(public_key),
        relay_config,
        output_format,
        Some(event),
    )
    .await
}

// a fresh cached profile saves a lookup. a fetched one isn't cached, since the
// cache also holds the follow list, which isn't fetched here.
async fn fetch_profile_name(public_key: PublicKey, relay_config: &RelayConfig) -> Option<String> {
    let cached_profile =
        current_user_cache::read(&public_key).and_then(|cached| cached.profile_event);
    let profile = match cached_profile {
        Some(profile) => profile,
        None => profile_fetcher::fetch(public_key, &relay_config.user_relay_urls).await?,
    };

    name_formatter::format_profile_name(&Metadata::try_from(&profile).ok()?)
}

// published as parsed, which percent-encodes anything hidden (like a bidi
// override), so the preview shows exactly what's signed.
fn parse_url(url: &str) -> Result<String, HasheusError> {
    let url = Url::parse(url.trim()).map_err(|_| HasheusError::InvalidURL)?;
    let is_web_url = matches!(url.scheme(), "https" | "http");

    if !is_web_url || url.as_str().chars().count() > MAX_URL_LENGTH {
        return Err(HasheusError::InvalidURL);
    }

    Ok(url.into())
}

// like the web page's form, the description defaults to the file's name.
fn parse_description(description: Option<&str>, path: &Path) -> Result<String, HasheusError> {
    let description = match description {
        Some(description) => description.trim().to_string(),
        None => path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
    };
    let length = description.chars().count();

    if length == 0 || length > MAX_DESCRIPTION_LENGTH {
        return Err(HasheusError::InvalidDescription);
    }

    Ok(description)
}

// without one, it's guessed from the file's extension. parameters, like github's
// "; charset=utf-8", are dropped, since the guess and the web page never have them.
fn parse_mime_type(mime_type: Option<&str>, path: &Path) -> Result<String, HasheusError> {
    let Some(mime_type) = mime_type else {
        return Ok(mime_type_guesser::guess(path).to_string());
    };
    let mime_type = mime_type
        .split_once(';')
        .map_or(mime_type, |(essence, _)| essence)
        .trim()
        .to_lowercase();
    let Some((type_name, subtype)) = mime_type.split_once('/') else {
        return Err(HasheusError::InvalidMIMEType);
    };

    if !is_mime_type_name(type_name) || !is_mime_type_name(subtype) {
        return Err(HasheusError::InvalidMIMEType);
    }

    Ok(mime_type)
}

// rfc 6838's restricted-name.
fn is_mime_type_name(name: &str) -> bool {
    let mut chars = name.chars();

    name.len() <= MAX_MIME_TYPE_NAME_LENGTH
        && chars
            .next()
            .is_some_and(|char| char.is_ascii_alphanumeric())
        && chars.all(|char| char.is_ascii_alphanumeric() || "!#$&-^_.+".contains(char))
}

fn read_size(path: &Path) -> Result<u64, HasheusError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|cause| HasheusError::ReadFile {
            path: path.to_path_buf(),
            cause,
        })
}

// publishing is public and can't be undone, so it's previewed and needs a yes.
// the preview goes to stderr, keeping stdout for the verify result.
fn confirm(
    verification: &FileVerification,
    public_key: &PublicKey,
    name: Option<&str>,
    is_confirmed: bool,
    can_prompt: bool,
) -> Result<(), HasheusError> {
    if is_confirmed {
        return Ok(());
    }

    if !can_prompt {
        return Err(HasheusError::ConfirmationRequired);
    }

    let mut stderr = io::stderr().lock();
    let _ = write!(
        stderr,
        "{}\n\npublish this verification? [y/N] ",
        render_preview(verification, public_key, name)
    );
    let _ = stderr.flush();

    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|_| HasheusError::NotPublished)?;

    if !is_yes(&answer) {
        return Err(HasheusError::NotPublished);
    }

    Ok(())
}

// sanitized for display only, since a file name can carry escape codes or bidi
// overrides. the npub stays next to the name, since anyone can copy a name.
fn render_preview(
    verification: &FileVerification,
    public_key: &PublicKey,
    name: Option<&str>,
) -> String {
    let npub = name_formatter::truncate_npub(public_key);
    let verifier = match name {
        Some(name) => format!("{name} ({npub})"),
        None => npub,
    };

    [
        ("file hash", verification.hash),
        ("description", verification.description),
        ("url", verification.url),
        ("mime type", verification.mime_type),
        ("verified by", &verifier),
    ]
    .map(|(key, value)| format!("{key}\n{}", sanitize(value).bold()))
    .join("\n\n")
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_hasher;

    #[test]
    fn accepts_a_web_url() {
        assert_eq!(
            parse_url(" https://example.com/file.iso ").unwrap(),
            "https://example.com/file.iso"
        );
        assert!(parse_url("http://example.com/file.iso").is_ok());
    }

    #[test]
    fn percent_encodes_hidden_characters_in_the_url() {
        assert_eq!(
            parse_url("https://example.com/\u{202E}osi.exe").unwrap(),
            "https://example.com/%E2%80%AEosi.exe"
        );
    }

    #[test]
    fn rejects_a_url_that_isnt_http() {
        for url in [
            "javascript:alert(1)",
            "ftp://example.com/file.iso",
            "example.com/file.iso",
        ] {
            assert!(
                matches!(parse_url(url), Err(HasheusError::InvalidURL)),
                "{url}"
            );
        }
    }

    #[test]
    fn rejects_a_url_over_1000_characters() {
        let url = format!("https://example.com/{}", "a".repeat(1000));

        assert!(matches!(parse_url(&url), Err(HasheusError::InvalidURL)));
    }

    #[test]
    fn defaults_the_description_to_the_file_name() {
        let description = parse_description(None, Path::new("/downloads/ubuntu.iso")).unwrap();

        assert_eq!(description, "ubuntu.iso");
    }

    #[test]
    fn trims_the_description() {
        let description =
            parse_description(Some("  ubuntu 24.04  "), Path::new("ubuntu.iso")).unwrap();

        assert_eq!(description, "ubuntu 24.04");
    }

    #[test]
    fn rejects_a_blank_or_long_description() {
        let long = "a".repeat(1001);

        for description in ["  ", long.as_str()] {
            let result = parse_description(Some(description), Path::new("ubuntu.iso"));

            assert!(matches!(result, Err(HasheusError::InvalidDescription)));
        }
    }

    #[test]
    fn guesses_the_mime_type_without_one() {
        let mime_type = parse_mime_type(None, Path::new("ubuntu.iso")).unwrap();

        assert_eq!(mime_type, "application/x-iso9660-image");
    }

    #[test]
    fn trims_and_lowercases_the_mime_type() {
        let mime_type =
            parse_mime_type(Some(" Application/X-Apple-DiskImage "), Path::new("a.iso")).unwrap();

        assert_eq!(mime_type, "application/x-apple-diskimage");
    }

    #[test]
    fn accepts_a_mime_type_with_symbols() {
        let mime_type =
            parse_mime_type(Some("application/vnd.apple.installer+xml"), Path::new("a")).unwrap();

        assert_eq!(mime_type, "application/vnd.apple.installer+xml");
    }

    #[test]
    fn drops_the_mime_type_parameters() {
        let mime_type =
            parse_mime_type(Some("Text/Plain ; charset=utf-8"), Path::new("a.txt")).unwrap();

        assert_eq!(mime_type, "text/plain");
    }

    #[test]
    fn accepts_a_mime_type_subtype_of_127_characters() {
        let mime_type = format!("application/{}", "a".repeat(127));

        assert!(parse_mime_type(Some(&mime_type), Path::new("a")).is_ok());
    }

    #[test]
    fn rejects_an_invalid_mime_type() {
        let long = format!("application/{}", "a".repeat(128));

        for mime_type in [
            "",
            "application",
            "application/",
            "/zip",
            "application/-zip",
            "text/plain/extra",
            "; charset=utf-8",
            long.as_str(),
        ] {
            let result = parse_mime_type(Some(mime_type), Path::new("a.zip"));

            assert!(
                matches!(result, Err(HasheusError::InvalidMIMEType)),
                "{mime_type}"
            );
        }
    }

    #[test]
    fn previews_what_will_be_published() {
        yansi::disable();
        let keys = Keys::generate();
        let npub = name_formatter::truncate_npub(&keys.public_key());
        let verification = FileVerification {
            hash: "ffff",
            mime_type: mime_type_guesser::DEFAULT_MIME_TYPE,
            size: 1,
            url: "https://example.com/file.iso",
            description: "a file",
        };

        let preview = render_preview(&verification, &keys.public_key(), None);

        assert_eq!(
            preview,
            format!(
                "file hash\nffff\n\ndescription\na file\n\nurl\nhttps://example.com/file.iso\n\nmime type\napplication/octet-stream\n\nverified by\n{npub}"
            )
        );
    }

    #[test]
    fn previews_the_name_with_the_npub() {
        yansi::disable();
        let keys = Keys::generate();
        let npub = name_formatter::truncate_npub(&keys.public_key());
        let verification = FileVerification {
            hash: "ffff",
            mime_type: mime_type_guesser::DEFAULT_MIME_TYPE,
            size: 1,
            url: "https://example.com/file.iso",
            description: "a file",
        };

        let preview = render_preview(&verification, &keys.public_key(), Some("alice"));

        assert!(preview.ends_with(&format!("verified by\nalice ({npub})")));
    }

    #[test]
    fn sanitizes_the_preview() {
        yansi::disable();
        let verification = FileVerification {
            hash: "ffff",
            mime_type: mime_type_guesser::DEFAULT_MIME_TYPE,
            size: 1,
            url: "https://example.com/\u{202E}osi.exe",
            description: "file\x1b[2K\x1b[1Aname",
        };

        let preview = render_preview(&verification, &Keys::generate().public_key(), None);

        assert!(preview.contains("description\nfile[2K[1Aname"));
        assert!(preview.contains("url\nhttps://example.com/osi.exe"));
    }

    #[test]
    fn only_takes_yes_for_an_answer() {
        for answer in ["y\n", "Y", " yes \n"] {
            assert!(is_yes(answer), "{answer:?}");
        }

        for answer in ["", "\n", "n", "no", "yep"] {
            assert!(!is_yes(answer), "{answer:?}");
        }
    }

    #[test]
    fn needs_yes_when_it_cant_ask() {
        let verification = FileVerification {
            hash: "ffff",
            mime_type: mime_type_guesser::DEFAULT_MIME_TYPE,
            size: 1,
            url: "https://example.com/file.iso",
            description: "a file",
        };

        let result = confirm(
            &verification,
            &Keys::generate().public_key(),
            None,
            false,
            false,
        );

        assert!(matches!(result, Err(HasheusError::ConfirmationRequired)));
        assert!(
            confirm(
                &verification,
                &Keys::generate().public_key(),
                None,
                true,
                false
            )
            .is_ok()
        );
    }

    mod publish {
        use super::*;
        use crate::file_event_publisher::FileEventPublisher;
        use crate::test_relays::{DEAD_RELAY_URL, build_relay_config, run_relay};
        use std::path::PathBuf;

        // a file unique to each test, removed when it's done.
        struct TestFile(PathBuf);

        impl TestFile {
            fn new(keys: &Keys) -> Self {
                let path = std::env::temp_dir().join(format!(
                    "hasheus_cli_publish_test_{}.iso",
                    keys.public_key()
                ));
                fs::write(&path, keys.public_key().to_hex()).unwrap();

                Self(path)
            }
        }

        impl Drop for TestFile {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }

        fn build_args(file: &TestFile, keys: &Keys, yes: bool) -> PublishArgs {
            PublishArgs {
                file: file.0.clone(),
                url: "https://example.com/file.iso".to_string(),
                description: None,
                mime_type: None,
                user: Some(keys.secret_key().to_bech32().unwrap()),
                yes,
            }
        }

        #[tokio::test]
        async fn fails_when_the_user_has_already_verified_the_file() {
            let (_relay, url) = run_relay(&[]).await;
            let relay_config = build_relay_config(&[&url], &[&url]);
            let keys = Keys::generate();
            let file = TestFile::new(&keys);
            let hash = file_hasher::hash(&file.0, |_| {}).unwrap().hash;
            let verification = FileVerification {
                hash: &hash,
                mime_type: mime_type_guesser::DEFAULT_MIME_TYPE,
                size: 1,
                url: "https://example.com/file.iso",
                description: "an earlier verification",
            };
            let publisher = FileEventPublisher::connect(&relay_config).await.unwrap();
            publisher
                .publish(&file_event_publisher::build_event(&verification, &keys))
                .await
                .unwrap();

            let result = publish(
                build_args(&file, &keys, true),
                &relay_config,
                OutputFormat::Json,
            )
            .await;

            assert!(matches!(result, Err(HasheusError::AlreadyVerified)));
        }

        #[tokio::test]
        async fn needs_yes_for_json_and_publishes_nothing_without_it() {
            let (_relay, url) = run_relay(&[]).await;
            let relay_config = build_relay_config(&[&url], &[&url]);
            let keys = Keys::generate();
            let file = TestFile::new(&keys);

            let result = publish(
                build_args(&file, &keys, false),
                &relay_config,
                OutputFormat::Json,
            )
            .await;

            assert!(matches!(result, Err(HasheusError::ConfirmationRequired)));
            let hash = file_hasher::hash(&file.0, |_| {}).unwrap().hash;
            let publisher = FileEventPublisher::connect(&relay_config).await.unwrap();
            assert!(!publisher.has_verified(&hash, keys.public_key()).await);
        }

        #[tokio::test]
        async fn checks_the_url_before_anything_else() {
            let keys = Keys::generate();
            let file = TestFile::new(&keys);
            let mut args = build_args(&file, &keys, true);
            args.url = "ftp://example.com/file.iso".to_string();
            // no relay is running, so getting past the url would fail differently.
            let relay_config = build_relay_config(&[DEAD_RELAY_URL], &[]);

            let result = publish(args, &relay_config, OutputFormat::Json).await;

            assert!(matches!(result, Err(HasheusError::InvalidURL)));
        }
    }
}
