use crate::cli::OutputFormat;
use crate::current_user_cache::{self, CachedUser};
use crate::file_hasher::FileHash;
use crate::hasheus_error::HasheusError;
use crate::json_serializer;
use crate::relay_config::RelayConfig;
use crate::screen_redrawer::ScreenRedrawer;
use crate::spinner;
use crate::verification_fetcher::{self, Update, UserRequirement};
use crate::verification_renderer;
use crate::verifications::Verifications;
use nostr_sdk::prelude::*;
use std::time::Duration;
use tokio::{signal, time};

// updates can arrive in bursts, so redraws are batched to avoid flicker.
const REDRAW_INTERVAL: Duration = Duration::from_millis(100);

// someone who just published sees their verification even if no relay returns
// it, and doesn't need a profile (see UserRequirement).
pub async fn report(
    file_hash: &FileHash,
    user_public_key: Option<PublicKey>,
    relay_config: &RelayConfig,
    output_format: OutputFormat,
    published_event: Option<Event>,
) -> Result<(), HasheusError> {
    let user_requirement = if published_event.is_some() {
        UserRequirement::NotRequired
    } else {
        UserRequirement::Required
    };
    let mut verifications = Verifications::new(user_public_key);

    if let Some(event) = published_event {
        verifications.apply_update(Update::FileEvent(event));
    }

    let user_to_fetch = restore_cached_user(&mut verifications, user_public_key);
    let mut updates = verification_fetcher::fetch(
        &file_hash.hash,
        user_to_fetch,
        relay_config,
        user_requirement,
    );
    // json is only printed once, at the end, so programs get a single document.
    let mut screen = ScreenRedrawer::new(output_format == OutputFormat::Text);
    let mut redraw_interval = time::interval(REDRAW_INTERVAL);
    let mut is_redraw_needed = true;
    let mut spinner_interval = time::interval_at(
        time::Instant::now() + spinner::FRAME_DURATION,
        spinner::FRAME_DURATION,
    );
    let mut spinner_frame = 0;
    let ctrl_c = signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Some(Ok(update)) => is_redraw_needed |= verifications.apply_update(update),
                Some(Err(error)) => {
                    screen.clear();
                    return Err(error);
                }
                None => break,
            },
            _ = redraw_interval.tick(), if is_redraw_needed && screen.is_live() => {
                screen.draw(&verification_renderer::render(file_hash, &verifications, spinner_frame));
                is_redraw_needed = false;
            }
            // drawn right away rather than on the next redraw, so the spinner
            // keeps an even pace.
            _ = spinner_interval.tick(), if verifications.is_pending && screen.is_live() => {
                spinner_frame += 1;
                screen.draw(&verification_renderer::render(file_hash, &verifications, spinner_frame));
                is_redraw_needed = false;
            }
            _ = &mut ctrl_c => {
                screen.clear();
                return Err(HasheusError::Interrupted);
            }
        }
    }

    let output = match output_format {
        OutputFormat::Text => {
            verification_renderer::render(file_hash, &verifications, spinner_frame)
        }
        OutputFormat::Json => {
            json_serializer::serialize_verifications(&file_hash.hash, &verifications)
        }
    };
    screen.finish(&output);

    if user_to_fetch.is_some()
        && let Some(current_user) = &verifications.current_user
    {
        current_user_cache::write(current_user);
    }

    Ok(())
}

// like the web page, a user cached within the hour isn't fetched again. returns
// the user to fetch, if any.
fn restore_cached_user(
    verifications: &mut Verifications,
    user_public_key: Option<PublicKey>,
) -> Option<PublicKey> {
    let public_key = user_public_key?;
    let Some(CachedUser {
        profile_event,
        follow_list_event,
        ..
    }) = current_user_cache::read(&public_key)
    else {
        return Some(public_key);
    };

    let updates = profile_event
        .map(Update::Profile)
        .into_iter()
        .chain(follow_list_event.map(Update::FollowList));

    for update in updates {
        verifications.apply_update(update);
    }

    None
}
