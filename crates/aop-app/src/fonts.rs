//! The font the application draws with, carried rather than hoped for.
//!
//! The stylesheet has always named Inter first. Naming a font is a request,
//! and whether it is honoured depends on the machine: Inter is not installed
//! on most of them, Windows stops at Segoe UI, macOS at neither, and this
//! Linux box has no Inter at all. Three platforms, three faces.
//!
//! That is not only a matter of looks. Text metrics decide how wide a column
//! has to be and where a line breaks, so the same plan lays out differently
//! for each person looking at it, and a printed page does not match the screen
//! it was checked on.
//!
//! It also fails worse than it sounds. A request for a family nobody has is
//! answered by whatever the matcher considers closest, and on this machine
//! `Inter` matched `CustomTkinter_shapes_font`, a shapes font sitting in the
//! user font directory. The chart drew its dates in Greek letters: `Aug`
//! became three Greek glyphs, and the days of the week came out as sigmas and
//! omegas. Latin codepoints, Greek shapes, no error anywhere.
//!
//! So the font travels with the program. One variable file covers every weight
//! the interface uses, which is why there is one of these and not four.
//!
//! Inter is licensed under the SIL Open Font License 1.1. The licence sits
//! beside the font in `assets/fonts/` and has to keep travelling with it.

/// Inter, variable, covering every weight the interface asks for.
pub const UI: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");

/// The family name to ask for, which has to be the name inside the file above
/// rather than a name we would like it to have.
pub const UI_FAMILY: &str = "InterVariable";

/// Which codepoints the bundled font can actually draw.
///
/// Reading the font's own character map, because the alternative is trusting
/// that it has whatever the interface happens to ask for, and that is exactly
/// what went wrong: six dropdown carets and five close crosses were typed as
/// codepoints the bundled font does not contain, there is no system fallback
/// by design, and so they were not drawn at all. Nothing reported it. A
/// missing glyph is the quietest possible failure.
#[cfg(test)]
fn coverage() -> Vec<(u32, u32)> {
    fn u16_at(d: &[u8], at: usize) -> u32 {
        u16::from_be_bytes([d[at], d[at + 1]]) as u32
    }
    fn u32_at(d: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
    }

    let d = UI;
    let tables = u16_at(d, 4) as usize;
    let mut cmap = None;
    for i in 0..tables {
        let at = 12 + i * 16;
        if &d[at..at + 4] == b"cmap" {
            cmap = Some(u32_at(d, at + 8) as usize);
        }
    }
    let cmap = cmap.expect("the bundled font has a character map");

    // Prefer a full Unicode subtable over a basic-plane one, so a codepoint
    // above the first plane is not reported missing merely because the older
    // table cannot describe it.
    let mut chosen = None;
    for i in 0..u16_at(d, cmap + 2) as usize {
        let at = cmap + 4 + i * 8;
        let (platform, encoding) = (u16_at(d, at), u16_at(d, at + 2));
        let sub = cmap + u32_at(d, at + 4) as usize;
        let full = matches!((platform, encoding), (3, 10) | (0, 4));
        if full || chosen.is_none() {
            chosen = Some(sub);
        }
        if full {
            break;
        }
    }
    let sub = chosen.expect("a usable subtable");

    let mut ranges = Vec::new();
    match u16_at(d, sub) {
        12 => {
            for i in 0..u32_at(d, sub + 12) as usize {
                let at = sub + 16 + i * 12;
                ranges.push((u32_at(d, at), u32_at(d, at + 4)));
            }
        }
        4 => {
            let segments = u16_at(d, sub + 6) as usize / 2;
            for i in 0..segments {
                let end = u16_at(d, sub + 14 + i * 2);
                let start = u16_at(d, sub + 16 + segments * 2 + i * 2);
                ranges.push((start, end));
            }
        }
        other => panic!("character map format {other} is not one this reads"),
    }
    ranges
}

/// Whether the bundled font can draw this codepoint.
///
/// Only the tests ask. Nothing at run time should need to: what the font can
/// draw is decided when this program is written, not while it is running, and
/// the test below is what decides it.
#[cfg(test)]
pub fn draws(codepoint: u32) -> bool {
    coverage().iter().any(|(a, b)| *a <= codepoint && codepoint <= *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_font_is_a_font() {
        assert!(UI.len() > 100_000, "that is not a font file");
        assert_eq!(&UI[..4], b"\x00\x01\x00\x00", "not TrueType");
    }

    #[test]
    fn it_draws_the_letters_a_plan_is_written_in() {
        for c in ['A', 'z', '0', '9', ' ', '-', '/', '\u{e9}'] {
            assert!(draws(c as u32), "{c:?} is missing from the bundled font");
        }
    }

    #[test]
    fn a_glyph_the_font_does_not_have_is_reported_missing() {
        // A guard that has never failed proves nothing, so this pins the half
        // that matters. These are the six that were being typed into the
        // interface and drawn as nothing at all until they became icons, and
        // the font still does not have them. If this test ever starts failing
        // it means `draws` has stopped being able to say no, and the guard
        // below has quietly become decoration.
        for (codepoint, what) in [
            (0x25BE, "caret"),
            (0x2714, "tick"),
            (0x2691, "flag"),
            (0x270E, "pencil"),
            (0x25C9, "fisheye"),
            (0x2935, "turning arrow"),
        ] {
            assert!(!draws(codepoint), "the font does have {what} after all");
        }
    }

    #[test]
    fn every_character_the_interface_types_can_actually_be_drawn() {
        // The guard. Any codepoint written as `\u{...}` anywhere in this
        // program has to exist in the font that ships with it, because there
        // is no fallback: a glyph the font does not have is not drawn, and
        // nothing anywhere says so.
        //
        // If this fails, do not reach for system fallback. Whatever it names
        // is almost certainly an affordance rather than a letter, and belongs
        // in `crate::icons` where it will be the same shape on every machine.
        let mut missing = Vec::new();
        for source in SOURCES {
            let mut rest = *source;
            while let Some(at) = rest.find("\\u{") {
                rest = &rest[at + 3..];
                let Some(close) = rest.find('}') else { break };
                if let Ok(cp) = u32::from_str_radix(&rest[..close], 16)
                    && cp > 0x7F
                    && !draws(cp)
                {
                    let shown = char::from_u32(cp).unwrap_or('?');
                    missing.push(format!("U+{cp:04X} {shown}"));
                }
                rest = &rest[close..];
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "typed in the interface but absent from the bundled font, so drawn \
             as nothing at all: {}",
            missing.join(", ")
        );
    }

    /// Every file that puts characters on the screen.
    const SOURCES: &[&str] = &[
        include_str!("controls.rs"),
        include_str!("contextmenu.rs"),
        include_str!("ribbon.rs"),
        include_str!("grid.rs"),
        include_str!("gantt.rs"),
        include_str!("views.rs"),
        include_str!("dialogs.rs"),
        include_str!("backstage.rs"),
        include_str!("versions.rs"),
        include_str!("popups.rs"),
        include_str!("welcome.rs"),
        include_str!("state.rs"),
    ];
}
