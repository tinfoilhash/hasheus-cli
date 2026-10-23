use crate::cli::OutputFormat;
use crate::file_hasher::{self, FileHash};
use crate::hasheus_error::HasheusError;
use crate::spinner;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::time::Instant;

const CLEAR_LINE: &str = "\r\x1b[2K";

// like the web page's spinner while it hashes, plus how far along it is. the
// line is short so it never wraps, since a carriage return only erases one row.
pub fn hash(target: &Path, output_format: OutputFormat) -> Result<FileHash, HasheusError> {
    let is_live = output_format == OutputFormat::Text && io::stdout().is_terminal();
    // a pipe's size is only what's buffered in it (on macos), not what's left.
    let size = fs::metadata(target)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map_or(0, |metadata| metadata.len());
    let started_at = Instant::now();
    let mut drawn_frame_index = 0;

    let file_hash = file_hasher::hash(target, |hashed_byte_count| {
        // nothing shows until the first frame is due, so a small file that's
        // hashed right away never flashes a line.
        let frame_index = frame_index_at(started_at);

        if is_live && frame_index != drawn_frame_index {
            drawn_frame_index = frame_index;
            let line = render(frame_index, hashed_byte_count, size);
            write_to_stdout(&format!("{CLEAR_LINE}{line}"));
        }
    });

    if drawn_frame_index > 0 {
        write_to_stdout(CLEAR_LINE);
    }

    file_hash
}

fn frame_index_at(started_at: Instant) -> usize {
    let elapsed = started_at.elapsed().as_millis();

    (elapsed / spinner::FRAME_DURATION.as_millis()) as usize
}

// a pipe has no size to measure against, and a file that grows while it's
// hashed could go past its starting size.
fn render(frame_index: usize, hashed_byte_count: u64, size: u64) -> String {
    let line = format!("{} hashing…", spinner::frame(frame_index));

    if size == 0 {
        return line;
    }

    let percent = (hashed_byte_count * 100 / size).min(100);

    format!("{line} {percent}%")
}

// a closed stdout (like a pipe to `head`) can't be drawn to, so failed writes
// are ignored rather than aborting the hashing.
fn write_to_stdout(text: &str) {
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_shows_the_spinner_and_percent() {
        assert_eq!(render(2, 42, 100), "⠹ hashing… 42%");
    }

    #[test]
    fn render_rounds_down_until_its_done() {
        assert_eq!(render(0, 999, 1000), "⠋ hashing… 99%");
    }

    #[test]
    fn render_never_goes_past_100_percent() {
        assert_eq!(render(0, 150, 100), "⠋ hashing… 100%");
    }

    #[test]
    fn render_leaves_out_the_percent_without_a_size() {
        assert_eq!(render(0, 1024, 0), "⠋ hashing…");
    }
}
