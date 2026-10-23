use std::io::{self, IsTerminal, Write};
use terminal_size::{Height, terminal_size};

const DISABLE_LINE_WRAP: &str = "\x1b[?7l";
const ENABLE_LINE_WRAP: &str = "\x1b[?7h";
const CLEAR_TO_END_OF_SCREEN: &str = "\x1b[J";
const DEFAULT_TERMINAL_HEIGHT: usize = 24;

// while the output changes, line wrapping is off so each line takes exactly one
// row, and the previous draw can be erased by moving the cursor up its rows.
pub struct ScreenRedrawer {
    is_live: bool,
    drawn_row_count: usize,
    is_line_wrap_disabled: bool,
}

impl ScreenRedrawer {
    pub fn new(should_redraw: bool) -> Self {
        Self {
            is_live: should_redraw && io::stdout().is_terminal(),
            drawn_row_count: 0,
            is_line_wrap_disabled: false,
        }
    }

    // piped or redirected output only gets the final draw, so it stays clean
    // for scripts and logs.
    pub fn is_live(&self) -> bool {
        self.is_live
    }

    pub fn draw(&mut self, text: &str) {
        if !self.is_live {
            return;
        }

        let lines = fit_to_height(text, terminal_height());
        let mut frame = erase_rows(self.drawn_row_count);

        if !self.is_line_wrap_disabled {
            frame.push_str(DISABLE_LINE_WRAP);
            self.is_line_wrap_disabled = true;
        }

        for line in &lines {
            frame.push_str(line);
            frame.push('\n');
        }

        self.drawn_row_count = lines.len();
        write_to_stdout(&frame);
    }

    // the final draw is printed in full, with line wrapping back on, so long
    // values are never cut off.
    pub fn finish(&mut self, text: &str) {
        let mut frame = self.take_erase_and_restore();
        frame.push_str(text);
        frame.push('\n');

        write_to_stdout(&frame);
    }

    pub fn clear(&mut self) {
        let frame = self.take_erase_and_restore();

        write_to_stdout(&frame);
    }

    fn take_erase_and_restore(&mut self) -> String {
        let mut frame = erase_rows(self.drawn_row_count);
        self.drawn_row_count = 0;

        if self.is_line_wrap_disabled {
            frame.push_str(ENABLE_LINE_WRAP);
            self.is_line_wrap_disabled = false;
        }

        frame
    }
}

// a panic or early return would otherwise leave the terminal without line
// wrapping.
impl Drop for ScreenRedrawer {
    fn drop(&mut self) {
        if self.is_line_wrap_disabled {
            write_to_stdout(ENABLE_LINE_WRAP);
        }
    }
}

fn terminal_height() -> usize {
    terminal_size().map_or(DEFAULT_TERMINAL_HEIGHT, |(_, Height(height))| {
        usize::from(height)
    })
}

// the cursor can't move above the top of the screen, so a draw taller than the
// terminal is cut off at the bottom until the final, full draw.
fn fit_to_height(text: &str, terminal_height: usize) -> Vec<&str> {
    let max_row_count = terminal_height.saturating_sub(1).max(1);

    text.lines().take(max_row_count).collect()
}

fn erase_rows(row_count: usize) -> String {
    if row_count == 0 {
        return String::new();
    }

    format!("\x1b[{row_count}F{CLEAR_TO_END_OF_SCREEN}")
}

// a closed stdout (like a pipe to `head`) can't be redrawn, so failed writes are
// ignored rather than aborting the verification.
fn write_to_stdout(text: &str) {
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_to_height_keeps_text_that_fits() {
        assert_eq!(fit_to_height("a\nb\nc", 10), ["a", "b", "c"]);
    }

    #[test]
    fn fit_to_height_leaves_a_row_for_the_cursor() {
        assert_eq!(fit_to_height("a\nb\nc", 3), ["a", "b"]);
    }

    #[test]
    fn fit_to_height_cuts_off_the_bottom() {
        assert_eq!(fit_to_height("a\nb\nc\nd\ne", 4), ["a", "b", "c"]);
    }

    #[test]
    fn fit_to_height_shows_at_least_one_line() {
        assert_eq!(fit_to_height("a\nb", 0), ["a"]);
    }

    #[test]
    fn erase_rows_moves_up_and_clears() {
        assert_eq!(erase_rows(3), "\x1b[3F\x1b[J");
    }

    #[test]
    fn erase_rows_does_nothing_without_rows() {
        assert_eq!(erase_rows(0), "");
    }
}
