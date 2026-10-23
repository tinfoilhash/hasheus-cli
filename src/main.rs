mod cli;
mod current_user;
mod current_user_cache;
mod file_event;
mod file_event_publisher;
mod file_hash_reporter;
mod file_hasher;
mod file_verifier;
mod hasheus_error;
mod helmet_renderer;
mod json_serializer;
mod mime_type_guesser;
mod name_formatter;
mod profile_fetcher;
mod public_key_reader;
mod relay_config;
mod relay_connector;
mod screen_redrawer;
mod secret_key_reader;
mod spinner;
#[cfg(test)]
mod test_event_builders;
#[cfg(test)]
mod test_relays;
mod text_sanitizer;
mod verification_fetcher;
mod verification_publisher;
mod verification_renderer;
mod verification_reporter;
mod verification_sorter;
mod verifications;

use crate::cli::{Cli, Command, OutputFormat};
use crate::hasheus_error::HasheusError;
use crate::relay_config::RelayConfig;
use std::io::{self, Write};
use std::process::ExitCode;
use yansi::{Condition, Paint};

// the shell convention for a process stopped by ctrl-c (128 + SIGINT).
const INTERRUPTED_EXIT_CODE: u8 = 128 + 2;

#[tokio::main]
async fn main() -> ExitCode {
    yansi::whenever(Condition::TTY_AND_COLOR);

    let cli = cli::parse();
    let output_format = cli.output_format();

    match run(cli, output_format).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(HasheusError::Interrupted) => ExitCode::from(INTERRUPTED_EXIT_CODE),
        // declining to publish is a choice, not a failure worth a red banner.
        Err(HasheusError::NotPublished) if output_format == OutputFormat::Text => {
            eprintln!("{}", HasheusError::NotPublished);

            ExitCode::FAILURE
        }
        Err(error) => {
            print_error(&error, output_format);

            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli, output_format: OutputFormat) -> Result<(), HasheusError> {
    let relay_config = RelayConfig::from_env()?;

    match cli.command {
        Command::Verify(args) => file_verifier::verify(args, &relay_config, output_format).await,
        Command::Publish(args) => {
            verification_publisher::publish(args, &relay_config, output_format).await
        }
    }
}

// json errors go to stdout like json results, so programs only read one stream.
// a reader that's gone away (like a pipe to `timeout 2 jq`) can't be written to,
// so a failed write is ignored rather than panicking.
fn print_error(error: &HasheusError, output_format: OutputFormat) {
    match output_format {
        OutputFormat::Text => {
            eprintln!("{}", " an error occurred! ".bold().on_red());
            eprintln!();
            eprintln!("{error}");
        }
        OutputFormat::Json => {
            let json = json_serializer::serialize_error(error);
            let _ = writeln!(io::stdout().lock(), "{json}");
        }
    }
}
