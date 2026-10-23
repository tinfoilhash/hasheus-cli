// anyone can publish text on nostr, so escape sequences, line breaks, and bidi
// overrides are removed to keep it from rewriting or faking parts of the output.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter_map(|character| match character {
            character if character.is_control() && character.is_whitespace() => Some(' '),
            character if character.is_control() || is_bidi_control(character) => None,
            character => Some(character),
        })
        .collect()
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_escape_sequences() {
        assert_eq!(sanitize("\x1b[2Ksafe\x07"), "[2Ksafe");
    }

    #[test]
    fn replaces_line_breaks_with_spaces() {
        assert_eq!(sanitize("one\ntwo\r\tthree"), "one two  three");
    }

    #[test]
    fn removes_bidi_overrides() {
        assert_eq!(sanitize("file\u{202E}osi.exe"), "fileosi.exe");
    }
}
