use crate::helmet_renderer;
use clap::{Arg, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use std::io::{self, IsTerminal};
use std::path::PathBuf;

// clap doesn't wrap help without its wrap_help feature, so it's wrapped here.
const VERIFY_AFTER_HELP: &str = "\
the user needs a nostr profile, which catches a mistyped key, and verifiers they
don't follow are unknown. the exit code is 0 unless an error occurs, whether or
not the file has been verified.

with --json, the result is printed once every relay has answered, and its
status is verified_by_you, trusted, unknown, or unverified. an error is
printed to stdout too, as {\"error\":{\"code\":…,\"message\":…}}, with exit code 1.";

const PUBLISH_AFTER_HELP: &str = "\
it shows what will be published and asks first, since it can't be undone, then
shows the file the way verify does. passing a key with --user leaves it in your
shell history, so HASHEUS_SECRET_KEY or the prompt is safer.

with --json, it needs --yes and a key from --user or HASHEUS_SECRET_KEY, and
prints the same result as verify.";

#[derive(Parser)]
#[command(version, about, long_about = None, override_usage = "hasheus [options] <command>")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// print the result as json instead of text
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// verify a file with signatures from people you trust
    #[command(
        override_usage = "hasheus verify [options] <file>",
        after_help = VERIFY_AFTER_HELP
    )]
    Verify(VerifyArgs),

    /// publish your own verification of a file
    #[command(
        override_usage = "hasheus publish [options] --url <url> <file>",
        after_help = PUBLISH_AFTER_HELP
    )]
    Publish(PublishArgs),
}

#[derive(Args)]
pub struct VerifyArgs {
    /// the file to verify, or its sha-256 hash
    #[arg(value_name = "file")]
    pub target: PathBuf,

    /// the user whose web of trust to use for verification (npub or hex public key) [default: HASHEUS_PUBLIC_KEY, then the public key of HASHEUS_SECRET_KEY]
    #[arg(short, long, value_name = "npub")]
    pub user: Option<String>,
}

#[derive(Args)]
pub struct PublishArgs {
    /// the file to publish a verification of
    #[arg(value_name = "file")]
    pub file: PathBuf,

    /// where the file can be downloaded (http or https)
    #[arg(long, value_name = "url")]
    pub url: String,

    /// what the file is [default: its file name]
    #[arg(long, value_name = "text")]
    pub description: Option<String>,

    /// the file's mime type, like application/zip [default: a guess from its extension]
    #[arg(long, value_name = "type")]
    pub mime_type: Option<String>,

    /// the secret key to sign with (nsec or ncryptsec) [default: HASHEUS_SECRET_KEY, then a prompt]
    #[arg(short, long, value_name = "nsec")]
    pub user: Option<String>,

    /// publish without asking first
    #[arg(short, long)]
    pub yes: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    // a prompt needs a terminal to answer it, and json output never asks.
    pub fn can_prompt(self) -> bool {
        self == Self::Text && io::stdin().is_terminal()
    }
}

impl Cli {
    pub fn output_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            OutputFormat::Text
        }
    }
}

pub fn parse() -> Cli {
    let matches = build_command().get_matches();

    Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit())
}

// clap's own help text is capitalized, so it's replaced to match the rest of the
// copy. the helmet is styled at runtime, which the derive attributes can't take.
fn build_command() -> clap::Command {
    let mut command = Cli::command().before_help(helmet_renderer::render());

    // clap only adds its help flags and help subcommand once the command is built.
    command.build();

    lowercase_help(command)
}

fn lowercase_help(command: clap::Command) -> clap::Command {
    let usage_style = *command.get_styles().get_usage();
    let help_template = format!(
        "{{before-help}}{{about-with-newline}}\n{usage_style}usage:{usage_style:#} {{usage}}\n\n{{all-args}}{{after-help}}"
    );

    command
        .help_template(help_template)
        .subcommand_help_heading("commands")
        .subcommand_value_name("command")
        .mut_args(lowercase_arg)
        .mut_subcommands(|subcommand| {
            let subcommand = if subcommand.get_name() == "help" {
                subcommand.about("print this message or the help of the given subcommand(s)")
            } else {
                subcommand
            };

            lowercase_help(subcommand)
        })
}

fn lowercase_arg(arg: Arg) -> Arg {
    let heading = if arg.is_positional() {
        "arguments"
    } else {
        "options"
    };
    let arg = arg.help_heading(heading);

    match arg.get_id().as_str() {
        "help" => arg.help("print help"),
        "version" => arg.help("print version"),
        // the help subcommand's argument.
        "subcommand" => arg.value_name("command"),
        _ => arg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_from(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(["hasheus"].iter().chain(args))
    }

    #[test]
    fn parses_verify() {
        let cli = parse_from(&["verify", "file.iso", "--user", "npub1"]).unwrap();

        let Command::Verify(args) = cli.command else {
            panic!("expected verify");
        };
        assert_eq!(args.target, PathBuf::from("file.iso"));
        assert_eq!(args.user.as_deref(), Some("npub1"));
    }

    #[test]
    fn parses_publish() {
        let cli = parse_from(&[
            "publish",
            "file.iso",
            "--url",
            "https://example.com/file.iso",
            "--description",
            "a file",
            "--mime-type",
            "application/x-iso9660-image",
            "--yes",
        ])
        .unwrap();

        let Command::Publish(args) = cli.command else {
            panic!("expected publish");
        };
        assert_eq!(args.url, "https://example.com/file.iso");
        assert_eq!(args.description.as_deref(), Some("a file"));
        assert_eq!(
            args.mime_type.as_deref(),
            Some("application/x-iso9660-image")
        );
        assert!(args.yes);
    }

    #[test]
    fn requires_a_url_to_publish() {
        assert!(parse_from(&["publish", "file.iso"]).is_err());
    }

    #[test]
    fn accepts_json_after_either_subcommand() {
        let cli = parse_from(&["verify", "file.iso", "--json"]).unwrap();

        assert!(cli.output_format() == OutputFormat::Json);
    }

    #[test]
    fn accepts_json_with_publish() {
        let cli = parse_from(&[
            "publish",
            "file.iso",
            "--json",
            "--url",
            "https://example.com/file.iso",
        ])
        .unwrap();

        assert!(cli.output_format() == OutputFormat::Json);
    }

    #[test]
    fn lowercases_the_help() {
        let mut command = build_command();
        let mut helps = vec![command.render_help().to_string()];

        for name in ["verify", "publish", "help"] {
            let subcommand = command.find_subcommand_mut(name).unwrap();

            helps.push(subcommand.render_help().to_string());
        }

        for help in helps {
            let help = ["HASHEUS_PUBLIC_KEY", "HASHEUS_SECRET_KEY", "-V"]
                .iter()
                .fold(help, |help, allowed| help.replace(allowed, ""));

            assert!(
                !help.chars().any(|char| char.is_ascii_uppercase()),
                "{help}"
            );
        }
    }

    #[test]
    fn requires_a_subcommand() {
        assert!(parse_from(&["file.iso"]).is_err());
    }
}
