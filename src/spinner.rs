use std::time::Duration;

// stands in for the web page's rotating dash, which can't keep one length in a
// terminal cell that's taller than it is wide.
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
// the usual pace for a braille spinner, which goes around in under a second.
pub const FRAME_DURATION: Duration = Duration::from_millis(80);

pub fn frame(index: usize) -> &'static str {
    FRAMES[index % FRAMES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_around_to_the_first_frame() {
        assert_eq!(frame(0), "⠋");
        assert_eq!(frame(FRAMES.len() + 2), "⠹");
    }
}
