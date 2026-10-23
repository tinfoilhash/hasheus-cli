use crate::cli::{OutputFormat, VerifyArgs};
use crate::file_hash_reporter;
use crate::hasheus_error::HasheusError;
use crate::public_key_reader;
use crate::relay_config::RelayConfig;
use crate::verification_reporter;

pub async fn verify(
    args: VerifyArgs,
    relay_config: &RelayConfig,
    output_format: OutputFormat,
) -> Result<(), HasheusError> {
    let user_public_key =
        public_key_reader::read(args.user.as_deref(), output_format.can_prompt())?;
    let file_hash = file_hash_reporter::hash(&args.target, output_format)?;

    verification_reporter::report(
        &file_hash,
        user_public_key,
        relay_config,
        output_format,
        None,
    )
    .await
}
