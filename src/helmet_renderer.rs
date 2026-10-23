use yansi::Paint;

const NAME: &str = "hasheus";

// the same helmet as the web page's header, a hash with the name across its
// brow.
const HELMET: [&str; 15] = [
    "        a        ",
    "       018       ",
    "       fe5       ",
    "    72eb8b00b    ",
    "  c72e95fd9c15c  ",
    " 003501bf305b2fd ",
    " 9e32hasheus23ff ",
    "ca018fe572eb8b00b",
    "c72     e     95f",
    "d9c15   c   00350",
    "1bf305b   2fd9e32",
    "23ffca0   18fe572",
    " eb8b00   bc72e9 ",
    "  5fd9c   15c00  ",
    "    350   1bf    ",
];

pub fn render() -> String {
    HELMET.map(render_row).join("\n")
}

// characters are spaced out like the web page's letter spacing, since a
// terminal cell is much taller than it is wide. the hash is dimmed so the name
// stands out across its brow.
fn render_row(row: &str) -> String {
    let name_range = row.find(NAME).map(|start| start..start + NAME.len());

    row.trim_end()
        .char_indices()
        .map(|(index, character)| {
            let is_name = name_range
                .as_ref()
                .is_some_and(|range| range.contains(&index));

            if is_name {
                character.bold().to_string()
            } else if character == ' ' {
                character.to_string()
            } else {
                character.dim().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_spaces_out_the_helmets_characters() {
        yansi::disable();

        let rendered = render();
        let rows: Vec<&str> = rendered.lines().collect();

        assert_eq!(rows.len(), HELMET.len());
        assert_eq!(rows[0], "                a");
        assert_eq!(rows[6], "  9 e 3 2 h a s h e u s 2 3 f f");
        assert_eq!(rows[14], "        3 5 0       1 b f");
    }
}
