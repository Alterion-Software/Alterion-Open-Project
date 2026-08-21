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

    /// Characters the interface types that the bundled font cannot draw, and
    /// which therefore depend on the machine having a font that can.
    ///
    /// Depending on that is a decision rather than an accident: the collection
    /// keeps the system fonts precisely so these get drawn by something. What
    /// it costs is that they are drawn by a **different** something on each
    /// platform, at a different weight and baseline, so the list should stay
    /// short and every entry should be one nobody minds varying.
    ///
    /// The dropdown caret is deliberately not here. It sits beside text at a
    /// size where a font's own baseline shows, so it is drawn in
    /// `crate::icons` and looks the same everywhere.
    const DRAWN_BY_THE_SYSTEM: &[u32] = &[
        0x2691, // flag, on a critical-path indicator
        0x2714, // heavy tick, on a completed one
        0x270E, // pencil, on a manually scheduled one
        0x25C9, // fisheye, on a deliverable
        0x2935, // turning arrow, the ribbon group launcher
        0x2715, // multiplication x, close buttons
    ];

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
    fn the_font_still_cannot_draw_the_ones_we_said_it_could_not() {
        // A guard that has never failed proves nothing. If this starts failing
        // it means the bundled font has grown a glyph, which is good news, and
        // the entry should come off the list.
        for codepoint in DRAWN_BY_THE_SYSTEM {
            assert!(
                !draws(*codepoint),
                "U+{codepoint:04X} is in the bundled font now; take it off the list"
            );
        }
    }

    #[test]
    fn nothing_new_quietly_starts_depending_on_the_machine() {
        // Every codepoint typed into the interface is either one the bundled
        // font draws, or one on the list above with a reason beside it. A new
        // one appearing unlisted is somebody adding a character whose shape
        // will differ on every platform, which deserves a moment's thought
        // rather than turning up in a screenshot.
        //
        // This started life as a stricter test, when the font collection held
        // nothing but the bundled font and a glyph it lacked was not drawn at
        // all: six dropdown carets and five close crosses were simply absent
        // and nothing said so. The collection keeps the system fonts now, so
        // the failure is no longer silence, it is inconsistency.
        let mut unlisted = Vec::new();
        for source in SOURCES {
            let mut rest = *source;
            while let Some(at) = rest.find("\\u{") {
                rest = &rest[at + 3..];
                let Some(close) = rest.find('}') else { break };
                if let Ok(cp) = u32::from_str_radix(&rest[..close], 16)
                    && cp > 0x7F
                    && !draws(cp)
                    && !DRAWN_BY_THE_SYSTEM.contains(&cp)
                {
                    let shown = char::from_u32(cp).unwrap_or('?');
                    unlisted.push(format!("U+{cp:04X} {shown}"));
                }
                rest = &rest[close..];
            }
        }
        unlisted.sort();
        unlisted.dedup();
        assert!(
            unlisted.is_empty(),
            "typed in the interface, absent from the bundled font, and not on \
             DRAWN_BY_THE_SYSTEM: {}. Either draw it in `crate::icons`, or add \
             it to that list with a note saying why varying by platform is \
             acceptable for it.",
            unlisted.join(", ")
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

/// The fonts to draw with: the one carried here, and whatever the machine has.
///
/// `blitz_dom::build_single_font_ctx` would register this font and nothing
/// else, and that is a trap worth naming. A font is not a character set. Inter
/// covers the letters a plan is written in and does not cover, for instance,
/// the small triangle a dropdown draws for its caret, and a glyph with no font
/// to draw it and nothing to fall back to is simply not drawn: no error, no
/// substitute box, just a caret that is not there.
///
/// So the collection keeps the system fonts and this one is added to it. What
/// is carried still decides the look, because the stylesheet names it first;
/// the rest is there to answer for anything it cannot draw.
#[cfg(feature = "native")]
pub fn context() -> dioxus_native::FontContext {
    use parley::fontique::Blob;
    use std::sync::Arc;

    let mut ctx = dioxus_native::FontContext::default();
    ctx.collection
        .register_fonts(Blob::new(Arc::new(UI) as _), None);
    ctx
}

/// The font families installed on this machine, for the font list to offer.
///
/// Asked rather than assumed. A hard coded list is a guess about somebody
/// else's computer: it offered Calibri and Segoe UI, which no Linux machine
/// has, and Inter, which almost none has either, so most of what it offered
/// could not be used and most of what could be was not offered.
///
/// Sorted, deduplicated, and with the font this program carries put first,
/// because that is the one the interface is drawn in and the one a document
/// looks the same in everywhere.
pub fn installed() -> Vec<String> {
    let mut collection = parley::fontique::Collection::new(parley::fontique::CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    let mut names: Vec<String> = collection
        .family_names()
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.to_string())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();

    // The carried font leads, then everything else in order.
    if let Some(at) = names.iter().position(|name| name == UI_FAMILY) {
        let ours = names.remove(at);
        names.insert(0, ours);
    } else {
        names.insert(0, UI_FAMILY.to_string());
    }
    names
}

/// The installed families, worked out once.
///
/// Reading the system's font configuration means touching the filesystem, and
/// a ribbon rebuilds constantly, so the answer is kept. Fonts installed while
/// the program is running will not appear until it is restarted, which is the
/// same bargain every other application makes.
pub fn families() -> &'static [String] {
    static FAMILIES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FAMILIES.get_or_init(installed)
}

#[cfg(test)]
mod family_tests {
    use super::*;

    #[test]
    fn the_machines_fonts_are_offered_and_the_carried_one_leads() {
        let found = families();
        assert!(
            !found.is_empty(),
            "no font families at all, which no machine that can draw text is true of"
        );
        assert_eq!(
            found[0], UI_FAMILY,
            "the font the program carries has to lead: it is what the interface \
             is drawn in and the only one a document looks the same in everywhere"
        );
        // The list that used to be hard coded named fonts most machines do not
        // have. Whatever is offered now has to be something this one does.
        assert!(found.len() > 1, "only the carried font was found");
    }

    #[test]
    fn it_is_worked_out_once() {
        assert!(std::ptr::eq(families(), families()));
    }
}
