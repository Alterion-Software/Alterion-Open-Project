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
