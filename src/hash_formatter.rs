use yansi::Paint;

const GROUP_LENGTH: usize = 8;

// like the web page, it's grouped by eight characters to compare by eye. a gap
// would be copied with the hash, so the groups alternate bold and regular
// instead. dim is too hard to read.
pub fn format(hash: &str) -> String {
    groups(hash)
        .enumerate()
        .map(|(index, group)| {
            if index % 2 == 0 {
                group.bold().to_string()
            } else {
                group.to_string()
            }
        })
        .collect()
}

// a hash is hex, so every group ends on a character boundary.
fn groups(hash: &str) -> impl Iterator<Item = &str> {
    (0..hash.len())
        .step_by(GROUP_LENGTH)
        .map(move |start| &hash[start..hash.len().min(start + GROUP_LENGTH)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_a_hash_by_eight_characters() {
        let hash = "0123456789abcdef".repeat(4);

        assert_eq!(groups(&hash).count(), 8);
        assert!(groups(&hash).all(|group| group == "01234567" || group == "89abcdef"));
    }

    #[test]
    fn keeps_a_short_last_group() {
        assert_eq!(groups("0123456789").collect::<Vec<_>>(), ["01234567", "89"]);
    }

    #[test]
    fn format_keeps_the_hash_whole_without_color() {
        yansi::disable();
        let hash = "0123456789abcdef".repeat(4);

        assert_eq!(format(&hash), hash);
    }
}
