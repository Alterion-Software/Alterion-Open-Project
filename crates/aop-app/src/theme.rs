//! The stylesheet, kept inline so the binary runs from `cargo run` with no
//! asset pipeline.
//!
//! The layout follows Microsoft Project (green-bar chrome, ribbon groups, a
//! grid beside a timescale) but the palette is Alterion's: near-black surfaces,
//! a muted teal accent and pale teal text, taken from the Alterion website.
//!
//! The palette itself is Rust data rather than text in the sheet. It used to be
//! written straight into a `:root` block, which reads perfectly well right up
//! until something outside CSS needs to know what a colour is. An SVG
//! presentation attribute is an attribute, not a declaration: `fill="var(--line)"`
//! is only ever resolved by a browser choosing to be generous about it, and a
//! renderer that parses SVG as SVG finds no valid paint there and falls back to
//! the SVG default, which is black. That is a chart of black blocks.
//!
//! So the table below is the one place a colour is written down. The `:root`
//! blocks are generated from it, which is why they no longer appear here as
//! text, and `Palette::paint` hands the same value to anything that has to say
//! it literally. One source, two consumers, no second list to keep in step.

use std::sync::LazyLock;

use dioxus::prelude::*;

use crate::state::AppState;

/// What a custom property holds.
///
/// Worth telling apart, because the whole reason the palette is data is that
/// something outside CSS wants to read it, and what that something wants is
/// almost always a paint. Three of these are not paints: two font stacks and a
/// drop shadow. Naming that here lets `Palette::colour` answer "there is no
/// colour under that name" instead of handing an SVG `fill` a list of font
/// families, which would leave the shape black and the mistake invisible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A paint. Valid as an SVG `fill` or `stroke` exactly as it stands.
    Colour,
    /// Anything else the sheet needs, which only CSS can make sense of.
    NotAColour,
}

/// One custom property, in both palettes.
pub struct Token {
    /// The name as CSS spells it, leading dashes included, so that the table
    /// reads the same way the sheet used to and nothing has to remember
    /// whether the dashes are part of the name or part of the syntax.
    pub name: &'static str,
    /// What the dark palette says. The sheet's own `:root` is the dark one, so
    /// every token has this and it is what an unanswered token falls back to.
    pub dark: &'static str,
    /// What the light palette says, where it says anything at all. `None`
    /// means the light overlay leaves the token alone, which it does for the
    /// two font stacks, and for `--on-bar`, whose bars are a mid tone in both
    /// palettes and so want the same ink on them either way.
    pub light: Option<&'static str>,
    pub kind: TokenKind,
}

impl Token {
    /// A paint the two palettes disagree about, which is nearly all of them.
    const fn colour(name: &'static str, dark: &'static str, light: &'static str) -> Self {
        Self { name, dark, light: Some(light), kind: TokenKind::Colour }
    }

    /// A paint that is the same whichever palette is up.
    const fn colour_either_way(name: &'static str, value: &'static str) -> Self {
        Self { name, dark: value, light: None, kind: TokenKind::Colour }
    }

    /// Something only CSS can use, that the two palettes disagree about.
    const fn other(name: &'static str, dark: &'static str, light: &'static str) -> Self {
        Self { name, dark, light: Some(light), kind: TokenKind::NotAColour }
    }

    /// Something only CSS can use that is the same either way.
    const fn other_either_way(name: &'static str, value: &'static str) -> Self {
        Self { name, dark: value, light: None, kind: TokenKind::NotAColour }
    }
}

/// The palette, in the order the `:root` block declares it.
///
/// Order is load bearing rather than incidental: the generated blocks are
/// written out in this order, and the test that holds them against the sheet
/// they replaced compares the sequence, not just the set.
///
/// `static` rather than `const` on purpose. A `const` array is copied at every
/// mention, so a reference into one borrows a temporary and cannot be handed
/// back; the resolver returns `&'static str` out of these entries.
static PALETTE: [Token; 43] = [
    // ---- Alterion palette --------------------------------------------
    Token::colour("--bg", "#0d0f10", "#eaefef"),
    Token::colour("--surface", "#131718", "#ffffff"),
    Token::colour("--surface-2", "#0d1a1a", "#f2f7f7"),
    Token::colour("--surface-3", "#171d1e", "#f7fafa"),
    Token::colour("--surface-4", "#1b2223", "#ffffff"),
    // The accent darkens rather than staying put when the palette flips: a mid
    // teal that reads well on near-black has too little contrast against white
    // to carry text or a focus ring.
    Token::colour("--accent", "#81b5b5", "#2f5f5e"),
    Token::colour("--accent-bright", "#a5d3d3", "#1e4746"),
    // Ink for text sitting on the accent or the contextual colour. Those are
    // the pale elements on this palette, so it is the dark one. It flips with
    // the palette; anything hardcoding a colour here is a bug waiting for a
    // theme.
    Token::colour("--on-accent", "#0b1414", "#f4f8f8"),
    // Ink for text sitting on a chart bar. The bars are a mid tone in both
    // palettes, so unlike --on-accent this does not flip.
    Token::colour_either_way("--on-bar", "#f2f7f7"),
    Token::colour("--accent-dim", "rgba(129, 181, 181, 0.14)", "rgba(47, 95, 94, 0.10)"),
    Token::colour("--accent-line", "rgba(129, 181, 181, 0.42)", "rgba(47, 95, 94, 0.38)"),
    Token::colour("--contextual", "#8aa2c4", "#45699b"),
    Token::colour("--line", "#27302f", "#d2dedd"),
    Token::colour("--line-soft", "#1d2425", "#e6eded"),
    Token::colour("--ink", "#d8e7e8", "#10201f"),
    Token::colour("--ink-soft", "#8fafaf", "#4a6362"),
    Token::colour("--ink-faint", "#5f7676", "#7b9291"),
    Token::colour("--hover", "rgba(216, 231, 232, 0.065)", "rgba(16, 32, 31, 0.055)"),
    Token::colour("--pressed", "rgba(216, 231, 232, 0.12)", "rgba(16, 32, 31, 0.10)"),
    Token::colour("--selection", "rgba(129, 181, 181, 0.17)", "rgba(47, 95, 94, 0.13)"),
    Token::colour("--selection-line", "rgba(129, 181, 181, 0.55)", "rgba(47, 95, 94, 0.5)"),
    // The one token in the palette that names another rather than a value, and
    // the reason the resolver follows a name instead of reading a field.
    Token::colour("--focus", "var(--accent)", "var(--accent)"),
    Token::colour("--grid-line", "#222a2b", "#e3ebeb"),
    Token::colour("--grid-header", "#171d1e", "#f2f7f7"),
    Token::colour("--nonworking", "rgba(216, 231, 232, 0.032)", "rgba(16, 32, 31, 0.038)"),
    // ---- chart --------------------------------------------------------
    Token::colour("--bar", "#3f7d7d", "#4b8b8b"),
    Token::colour("--bar-edge", "#6aadad", "#2f5f5e"),
    Token::colour("--bar-progress", "#a5d3d3", "#1e4746"),
    Token::colour("--bar-critical", "#9d474d", "#b3565c"),
    Token::colour("--bar-critical-edge", "#d9636a", "#8b393f"),
    Token::colour("--bar-progress-critical", "#e79aa0", "#7a2f34"),
    Token::colour("--bar-summary", "#cfe3e3", "#20403f"),
    Token::colour("--bar-inactive", "#414c4c", "#b9c6c6"),
    Token::colour("--baseline", "#6b7f7f", "#8aa3a2"),
    Token::colour("--slack", "#4d6060", "#a9bdbc"),
    Token::colour("--today", "#d9636a", "#b3565c"),
    Token::colour("--link-arrow", "#7e9a9a", "#6d8786"),
    Token::colour("--danger", "#d9636a", "#ac5157"),
    Token::colour("--danger-bg", "rgba(217, 99, 106, 0.12)", "rgba(172, 81, 87, 0.10)"),
    Token::colour("--warn", "#d9b06a", "#9d6f16"),
    Token::other("--shadow", "0 12px 34px rgba(0, 0, 0, 0.55)", "0 12px 30px rgba(16, 32, 31, 0.18)"),
    // Families are listed most-wanted first, but every name here must be one
    // that actually exists somewhere, because a matcher that falls back by
    // substring can land on an unrelated font whose name merely contains the
    // word. "Inter" matching a symbol font called CustomTkinter_shapes_font is
    // exactly that, and it renders text as arbitrary shapes.
    Token::other_either_way(
        "--font",
        "\"Inter\", \"InterVariable\", \"Segoe UI\", \"Noto Sans\", \"DejaVu Sans\", \"Liberation Sans\", sans-serif",
    ),
    Token::other_either_way(
        "--mono",
        "ui-monospace, \"Cascadia Mono\", \"JetBrains Mono\", Consolas, monospace",
    ),
];

/// One of the two palettes, as something that can be asked for a value.
///
/// Separate from `ThemeChoice` because the two are different questions. A
/// choice includes "whatever the desktop says", which is not a palette until
/// something has answered it; this is the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    Dark,
    Light,
}

/// How many names deep a token is followed before it is called a cycle.
///
/// The palette uses exactly one hop today, `--focus: var(--accent)`. A second
/// is allowed for rather than assumed, so that adding one does not quietly put
/// the literal text `var(--accent)` into an SVG attribute, which is the same
/// black shape this whole change exists to get rid of. Anything longer than
/// this is a loop, and a loop has no value to hand back.
const MAX_HOPS: usize = 4;

fn token(name: &str) -> Option<&'static Token> {
    PALETTE.iter().find(|token| token.name == name)
}

impl Palette {
    /// What this palette says about a token, before any name is followed.
    ///
    /// A token the light palette does not restate keeps the dark value, which
    /// is exactly what the overlay does in CSS: it wins by coming after the
    /// sheet, so a property it never mentions still holds what the sheet gave
    /// it.
    fn stated(self, token: &Token) -> &'static str {
        match self {
            Palette::Dark => token.dark,
            Palette::Light => token.light.unwrap_or(token.dark),
        }
    }

    /// The value of a token in this palette, with any `var()` followed until
    /// it lands on something literal.
    ///
    /// `None` for a name the palette does not have, and for a chain that never
    /// ends.
    pub fn value(self, name: &str) -> Option<&'static str> {
        let mut at = token(name)?;
        for _ in 0..MAX_HOPS {
            let value = self.stated(at);
            let Some(inner) = value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) else {
                return Some(value);
            };
            at = token(inner.trim())?;
        }
        None
    }

    /// The literal colour a token stands for, or `None` if that name is not a
    /// colour in this palette.
    ///
    /// The honest form of the question, and the one worth calling when the
    /// name came from somewhere other than the source of this program.
    pub fn colour(self, name: &str) -> Option<&'static str> {
        match token(name) {
            Some(found) if found.kind == TokenKind::Colour => self.value(name),
            _ => None,
        }
    }

    /// The literal colour for a token, for an attribute that cannot take a
    /// `var()`.
    ///
    /// A name that is not a colour is a mistake in this program rather than
    /// something a plan can cause, and there is a test that walks the source of
    /// every caller and checks each name against the table. What it falls back
    /// to still matters, because a fallback is what ships if that test is ever
    /// weakened: `currentColor` inherits, so a mistake shows up as a shape in
    /// the wrong colour rather than as the black rectangle this change is
    /// about.
    pub fn paint(self, name: &str) -> &'static str {
        self.colour(name).unwrap_or("currentColor")
    }

    /// The font stack for text drawn inside an SVG.
    ///
    /// Text in a page inherits its font from whatever contains it. Text inside
    /// an SVG only inherits it if the renderer carries the inheritance across
    /// that boundary, and not every renderer does. Text that inherits nothing
    /// gets whatever the renderer reaches for first, which on one machine was
    /// a symbol face: `Aug` came out as `Auy` in Greek letters, and the days
    /// of the week as sigmas and omegas. Latin codepoints, Greek glyphs, no
    /// error anywhere.
    ///
    /// So the chart says which font it wants rather than assuming it will be
    /// told. `sans-serif` is the fallback because a generic family is the one
    /// thing every renderer resolves to something readable.
    pub fn font(self) -> &'static str {
        self.value("--font").unwrap_or("sans-serif")
    }

    /// The same, for a value that may or may not be naming a token.
    ///
    /// Colours that ride with the plan rather than the program come through
    /// here: an annotation's stroke is a string somebody typed or a token the
    /// defaults chose, and `aop_core` has no idea which palette is up. A
    /// literal colour is handed straight back, so this is safe to put in front
    /// of anything.
    pub fn literal(self, value: &str) -> &str {
        match value.strip_prefix("var(").and_then(|v| v.strip_suffix(')')) {
            Some(name) => self.paint(name.trim()),
            None => value,
        }
    }
}

/// The `:root` block for one palette, which is what the sheet used to carry as
/// text.
///
/// Light emits only the tokens it actually restates. The overlay wins by
/// coming after the sheet, so a token it leaves out keeps the value the sheet
/// already gave it, and restating those would be forty lines saying nothing.
fn root_block(palette: Palette) -> String {
    let mut css = String::from(":root {\n");
    for token in &PALETTE {
        let value = match palette {
            Palette::Dark => Some(token.dark),
            Palette::Light => token.light,
        };
        if let Some(value) = value {
            css.push_str("  ");
            css.push_str(token.name);
            css.push_str(": ");
            css.push_str(value);
            css.push_str(";\n");
        }
    }
    css.push_str("}\n");
    css
}

/// The palette the interface is painting with, for anything that has to write
/// a colour down rather than name it.
///
/// A lookup where the drawing happens rather than an argument threaded down
/// from the top: the components that draw SVG are spread across three files and
/// most of them already reach for the application state exactly this way, so
/// this adds no new route in and nothing to keep passing along. Reading the
/// signal also subscribes the caller, which is what makes a chart repaint when
/// the theme changes rather than keeping the colours it was first drawn with.
///
/// `consume_context` and not `use_context`, despite the name of this function.
/// `use_context` is a hook and takes a hook slot, so it may only be called
/// before any early return; several of the callers here start with one, and a
/// palette that has to be fetched at the top of a function that has not yet
/// decided whether it is drawing anything is a rule waiting to be broken by
/// somebody who did not know it existed. Consuming the context walks the scope
/// tree instead, which costs a few pointer hops and can be called anywhere. The
/// subscription is unaffected: it comes from reading the signal, not from how
/// the signal was found.
pub fn use_palette() -> Palette {
    consume_context::<Signal<AppState>>().read().theme.palette()
}

/// The whole stylesheet: the dark palette, then every rule written against it.
///
/// Built once and kept. It goes into a `<style>` element that never re-renders,
/// so there is no reason to assemble a hundred kilobytes of text twice.
pub static CSS: LazyLock<String> =
    LazyLock::new(|| format!("{}{RULES}", root_block(Palette::Dark)));

/// Everything in the sheet that is not the palette.
const RULES: &str = r##"
* { box-sizing: border-box; }

html, body, #main {
  height: 100%;
  margin: 0;
  padding: 0;
  overflow: hidden;
}

body {
  font-family: var(--font);
  font-size: 12px;
  color: var(--ink);
  background: var(--bg);
  -webkit-user-select: none;
  user-select: none;
  cursor: default;
  -webkit-font-smoothing: antialiased;
  text-rendering: optimizeLegibility;
}

.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
  /* No OS decorations, so the app draws its own edge. */
  border: 1px solid #202a2a;
}

button { font: inherit; color: inherit; }

/* ---------- scrollbars ---------- */

::-webkit-scrollbar { width: 13px; height: 13px; }
::-webkit-scrollbar-track { background: var(--bg); }
::-webkit-scrollbar-thumb { background: #2c3737; border: 3px solid var(--bg); border-radius: 8px; }
::-webkit-scrollbar-thumb:hover { background: #3d4c4c; }
::-webkit-scrollbar-corner { background: var(--bg); }

/* ---------- title bar ---------- */

.titlebar {
  display: flex;
  align-items: center;
  height: 30px;
  background: var(--surface-2);
  color: var(--ink);
  padding: 0 6px;
  flex: none;
  border-bottom: 1px solid var(--line-soft);
}

.qat { display: flex; align-items: center; gap: 1px; padding-left: 2px; }

.qat-btn {
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 3px;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
}

.qat-btn:hover:not(:disabled) { background: var(--hover); color: var(--accent-bright); }
.qat-btn:active:not(:disabled) { background: var(--pressed); }
.qat-btn:disabled { opacity: 0.32; }

.qat-sep { width: 1px; height: 15px; margin: 0 4px; background: var(--line); }

.drag-region {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-width: 0;
}

.wincontrols { display: flex; flex: none; margin-right: -6px; }

.wc {
  width: 44px;
  height: 30px;
  display: grid;
  place-items: center;
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
}

.wc:hover { background: var(--hover); color: var(--ink); }
.wc.close:hover { background: var(--danger); color: #fff; }

.title-text {
  text-align: center;
  font-size: 12px;
  color: var(--ink-soft);
  letter-spacing: 0.2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  padding: 0 12px;
}

.title-text b { color: var(--ink); font-weight: 500; }

/* ---------- contextual tools banner ---------- */

.tools-banner {
  display: flex;
  align-items: stretch;
  height: 16px;
  background: var(--surface-2);
  flex: none;
  overflow: hidden;
}

/* Hidden copies of the tabs, purely to reserve the same widths. */
.tools-banner .ghost {
  visibility: hidden;
  height: 16px;
  border-top: 0;
  pointer-events: none;
  flex: none;
}

.tools-label {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--contextual);
  color: var(--on-accent);
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.7px;
  text-transform: uppercase;
  padding: 0 14px;
  border-radius: 4px 4px 0 0;
  white-space: nowrap;
  flex: none;
}

/* ---------- ribbon tab strip ---------- */

.tabstrip {
  display: flex;
  align-items: stretch;
  height: 27px;
  background: var(--surface-2);
  flex: none;
}

.tab {
  display: flex;
  align-items: center;
  padding: 0 14px;
  color: var(--ink-soft);
  font-size: 12px;
  border: 0;
  border-top: 2px solid transparent;
  background: transparent;
  cursor: default;
  white-space: nowrap;
}

.tab:hover { color: var(--ink); background: var(--hover); }

.tab.active {
  background: var(--surface);
  color: var(--accent-bright);
  border-top-color: var(--accent);
}

.tab.file {
  background: var(--accent);
  color: var(--on-accent);
  font-weight: 600;
  padding: 0 17px;
  border-top-color: var(--accent);
}

.tab.file:hover { background: var(--accent-bright); color: var(--on-accent); }

.tab.contextual { background: transparent; color: var(--contextual); }
.tab.contextual:hover { background: var(--hover); }
.tab.contextual.active { background: var(--surface); border-top-color: var(--contextual); color: var(--contextual); }

.tabstrip .filler { flex: 1; }

/* ---------- ribbon ---------- */

.ribbon {
  display: flex;
  align-items: stretch;
  background: var(--surface);
  border-bottom: 1px solid var(--line);
  height: 94px;
  flex: none;
  overflow: hidden;
}

.ribbon.collapsed { height: 0; }

.ribbon-scroll {
  display: flex;
  align-items: stretch;
  overflow-x: auto;
  overflow-y: hidden;
  flex: 1;
}

.ribbon-scroll::-webkit-scrollbar { height: 5px; }

.rgroup {
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--line-soft);
  padding: 3px 6px 0;
  flex: none;
}

.rgroup-body {
  display: flex;
  /* Stretch, so a button with a long caption sets the height and the rest come
     up to meet it rather than one spilling past a fixed box. */
  align-items: stretch;
  gap: 2px;
  flex: 1;
  padding-bottom: 2px;
}

.rgroup-title {
  text-align: center;
  font-size: 9.5px;
  color: var(--ink-faint);
  letter-spacing: 0.3px;
  padding: 1px 0 3px;
  white-space: nowrap;
}

.rgroup-title .launcher { display: inline-block; margin-left: 4px; opacity: 0.6; }

.rcol { display: flex; flex-direction: column; gap: 1px; }
.rrow { display: flex; align-items: center; gap: 2px; }

.rbtn-lg {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  min-width: 48px;
  max-width: 76px;
  /* A minimum rather than a fixed height. A caption like "Change Working Time"
     wraps to three lines, and against a fixed height it simply spilled out of
     the button. The row stretches its buttons to match, so they stay level. */
  min-height: 66px;
  padding: 4px 5px 2px;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  cursor: default;
  text-align: center;
  line-height: 1.15;
  font-size: 11px;
}

.rbtn-lg .glyph { height: 32px; flex: none; display: grid; place-items: center; color: var(--accent); }
.rbtn-lg .caption { white-space: normal; overflow-wrap: anywhere; }

.rbtn-sm {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 21px;
  padding: 0 6px;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  cursor: default;
  white-space: nowrap;
  font-size: 11px;
  max-width: 178px;
}

.rbtn-sm .glyph { display: grid; place-items: center; width: 16px; flex: none; color: var(--accent); }
.rbtn-sm .caption { overflow: hidden; text-overflow: ellipsis; }

.rbtn-icon {
  width: 22px;
  height: 21px;
  display: grid;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
}

.rbtn-lg:hover:not(.disabled), .rbtn-sm:hover:not(.disabled), .rbtn-icon:hover:not(.disabled) {
  background: var(--hover);
  border-color: var(--line);
}

.rbtn-lg:active:not(.disabled), .rbtn-sm:active:not(.disabled), .rbtn-icon:active:not(.disabled) {
  background: var(--pressed);
}

.rbtn-lg.on, .rbtn-sm.on, .rbtn-icon.on {
  background: var(--accent-dim);
  border-color: var(--accent-line);
}

.disabled { opacity: 0.32; }

.caret { font-size: 8px; opacity: 0.65; line-height: 1; }

.rcheck {
  display: flex;
  /* Centred on the box rather than the text baseline: the label is 11px and
     the box 12px, so aligning by baseline leaves the tick sitting low. */
  align-items: center;
  gap: 7px;
  height: 20px;
  padding: 0 5px;
  font-size: 11px;
  line-height: 1;
  border-radius: 3px;
  cursor: default;
  white-space: nowrap;
  text-align: left;
}

.rcheck:hover { background: var(--hover); }

.rcheck .box {
  width: 12px;
  height: 12px;
  border: 1px solid var(--ink-faint);
  border-radius: 2px;
  background: transparent;
  display: grid;
  place-items: center;
  /* The tick is drawn from the font, so it needs its own line box or it sits
     a pixel low inside the square. */
  font-size: 9px;
  line-height: 1;
  flex: none;
}

.rcheck .box.on { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }

.font-row { display: flex; gap: 2px; align-items: center; }

.rselect {
  height: 20px;
  border: 1px solid var(--line);
  border-radius: 3px;
  background: var(--surface-3);
  font-size: 11px;
  padding: 0 5px;
  color: var(--ink);
  -webkit-user-select: text;
  user-select: text;
}

.rselect:focus { outline: none; border-color: var(--accent); }

/* ---------- dropdown ---------- */

.dd {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 7px;
  height: 20px;
  padding: 0 7px;
  border: 1px solid var(--line);
  border-radius: 3px;
  background: var(--surface-3);
  color: var(--ink);
  font-size: 11px;
  cursor: default;
  text-align: left;
  min-width: 0;
}

.dd.lg { height: 30px; font-size: 12px; padding: 0 10px; border-radius: 4px; }
.dd:hover:not(.disabled) { border-color: var(--accent-line); background: var(--surface-4); }
.dd.disabled { opacity: 0.38; }
.dd .dd-value { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dd .dd-caret { font-size: 8px; color: var(--ink-soft); flex: none; }

.dd-list {
  position: fixed;
  z-index: 90;
  background: var(--surface-4);
  border: 1px solid var(--line);
  border-radius: 5px;
  box-shadow: var(--shadow);
  padding: 4px;
  max-height: 320px;
  overflow-y: auto;
}

.dd-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 9px;
  border: 0;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  font-size: 12px;
  text-align: left;
  cursor: default;
  white-space: nowrap;
}

.dd-item:hover { background: var(--accent-dim); color: var(--accent-bright); }
.dd-item .tick svg { color: var(--accent); }

/* ---------- combo box ---------- */

.combo {
  display: flex;
  align-items: stretch;
  height: 20px;
  border: 1px solid var(--line);
  border-radius: 3px;
  background: var(--surface-3);
  overflow: hidden;
}

.combo:focus-within { border-color: var(--accent); }

.combo-input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--ink);
  font: inherit;
  font-size: 11px;
  padding: 0 6px;
  -webkit-user-select: text;
  user-select: text;
}

.combo-caret {
  width: 18px;
  border: 0;
  border-left: 1px solid var(--line);
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  display: grid;
  place-items: center;
}

.combo-caret:hover { background: var(--hover); color: var(--accent-bright); }
.dd-item.on { color: var(--accent-bright); }
.dd-item .tick { width: 12px; flex: none; color: var(--accent); }

.gallery { display: flex; gap: 5px; align-items: center; padding: 2px 0; }

.gallery-item {
  width: 60px;
  height: 46px;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: var(--surface-2);
  padding: 6px 5px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  justify-content: center;
  cursor: default;
  flex: none;
  overflow: hidden;
}

.gallery-item:hover { border-color: var(--accent-line); background: var(--surface-3); }

.gallery-item.on {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-dim);
}

.gallery-item .g-bar { height: 4px; border-radius: 2px; flex: none; }

/* ---------- backstage ---------- */

.backstage {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  background: var(--bg);
}

.bs-nav {
  width: 196px;
  background: var(--surface-2);
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  padding: 6px 0 10px;
  flex: none;
}

.bs-back {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 40px;
  padding: 0 14px;
  border: 0;
  background: transparent;
  color: var(--accent);
  cursor: default;
  font-size: 13px;
}

.bs-back:hover { background: var(--hover); }

.bs-item {
  display: flex;
  align-items: center;
  gap: 11px;
  text-align: left;
  padding: 8px 18px;
  border: 0;
  border-left: 2px solid transparent;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  font-size: 13px;
}

/* The glyph column is a fixed width so the labels line up whatever each icon
   happens to measure. */
.bs-item .glyph {
  display: grid;
  place-items: center;
  width: 16px;
  flex: none;
  color: var(--ink-faint);
}
.bs-item.active .glyph, .bs-item:hover .glyph { color: var(--accent); }

.bs-item:hover { background: var(--hover); color: var(--ink); }
.bs-item.active { background: var(--accent-dim); border-left-color: var(--accent); color: var(--accent-bright); }
.bs-sep { height: 1px; background: var(--line); margin: 7px 14px; }
.bs-spacer { flex: 1; min-height: 12px; }

.bs-body {
  flex: 1;
  min-width: 0;
  padding: 26px 40px 40px;
  overflow-y: auto;
}

.bs-title {
  font-size: 30px;
  font-weight: 300;
  letter-spacing: -0.6px;
  color: var(--ink);
  margin: 0 0 20px;
}

.bs-sub {
  font-size: 14px;
  font-weight: 600;
  margin: 24px 0 10px;
  color: var(--ink);
  letter-spacing: 0.1px;
}

/* ---------- splash ---------- */

.splash {
  position: fixed;
  inset: 0;
  z-index: 200;
  display: flex;
  align-items: stretch;
  background: var(--bg);
  animation: splash-in 260ms ease-out;
}

@keyframes splash-in { from { opacity: 0; } to { opacity: 1; } }

.splash-left {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 10px;
  padding: 0 56px;
  min-width: 0;
}

.splash-logo { color: var(--ink); }
.splash-logo svg { width: 100%; height: 100%; display: block; }

.splash-product {
  font-size: 15px;
  letter-spacing: 4.4px;
  text-transform: uppercase;
  color: var(--ink-faint);
  padding-left: 3px;
}

.splash-version { font-size: 11.5px; color: var(--ink-faint); padding-left: 3px; margin-top: 8px; }

.splash-bar {
  width: 240px;
  height: 3px;
  border-radius: 2px;
  background: rgba(216, 231, 232, 0.09);
  overflow: hidden;
  margin: 12px 0 4px 3px;
}

.splash-fill {
  height: 100%;
  width: 40%;
  border-radius: 2px;
  background: var(--accent);
  animation: splash-sweep 1.7s ease-in-out forwards;
}

@keyframes splash-sweep {
  from { width: 6%; }
  to { width: 100%; }
}

.splash-note { font-size: 11px; color: var(--ink-faint); padding-left: 3px; }

.splash-art {
  width: 42%;
  max-width: 420px;
  flex: none;
  display: grid;
  place-items: center;
  background: var(--surface-2);
  border-left: 1px solid var(--line);
}

/* ---------- info ---------- */

.info-head { margin-bottom: 20px; }
.info-head .recent-path { margin-top: 5px; }

.info-alert {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  padding: 11px 14px;
  margin-bottom: 18px;
  max-width: 740px;
  background: var(--danger-bg);
  border: 1px solid rgba(217, 99, 106, 0.4);
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.5;
}

.stat-row {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(112px, 1fr));
  gap: 10px;
  max-width: 740px;
  margin-bottom: 20px;
}

.stat-tile {
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 12px 14px;
}

.stat-value {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.3px;
  color: var(--ink);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.stat-label {
  font-size: 9.5px;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: var(--ink-faint);
  margin-top: 4px;
}

.info-chart {
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  max-width: 740px;
  margin-bottom: 20px;
  overflow: hidden;
}

.info-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: 14px;
  max-width: 740px;
}

.info-card {
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 14px 16px 6px;
}

.info-card h3 {
  font-size: 11px;
  letter-spacing: 0.9px;
  text-transform: uppercase;
  color: var(--accent);
  margin: 0 0 10px;
  font-weight: 600;
}

.info-line {
  display: flex;
  justify-content: space-between;
  gap: 14px;
  padding: 7px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}

.info-card .info-line:last-child { border-bottom: 0; }
.info-line .k { color: var(--ink-soft); }
.info-line .v { color: var(--ink); text-align: right; }

/* ---------- home ---------- */

.home-section {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  width: 100%;
  margin-top: 26px;
}

.home-section .bs-sub { margin-bottom: 10px; }

.bs-link {
  border: 0;
  background: transparent;
  color: var(--accent);
  font-size: 12px;
  cursor: default;
  padding: 2px 4px;
  border-radius: 4px;
}

.bs-link:hover { background: var(--accent-dim); color: var(--accent-bright); }

.home-empty {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 22px;
  border: 1px dashed var(--line);
  border-radius: 8px;
  color: var(--ink-soft);
  font-size: 12px;
  max-width: 520px;
}

.home-empty > :first-child { color: var(--accent); }

/* template gallery */
.tpl-grid {
  display: grid;
  /* auto-fill so the cards spread across whatever width there is, rather than
     huddling on the left of a wide window. */
  grid-template-columns: repeat(auto-fill, minmax(212px, 1fr));
  gap: 16px;
  width: 100%;
}

.tpl-card {
  border: 1px solid var(--line);
  border-radius: 5px;
  background: var(--surface);
  cursor: default;
  padding: 0;
  text-align: left;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  transition: border-color 0.15s, transform 0.15s;
}

.tpl-card:hover { border-color: var(--accent-line); transform: translateY(-2px); }

.tpl-thumb {
  height: 128px;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
  position: relative;
  overflow: hidden;
}

.tpl-thumb svg { display: block; width: 100%; height: 100%; }

.tpl-meta { padding: 9px 11px 11px; }
.tpl-name { font-size: 12.5px; font-weight: 600; color: var(--ink); }
.tpl-desc { font-size: 11px; color: var(--ink-soft); margin-top: 4px; line-height: 1.4; }
.tpl-count { font-size: 10.5px; color: var(--accent); margin-top: 6px; letter-spacing: 0.2px; }

/* recent list */
.recent-list { max-width: 680px; }

.recent-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 9px 10px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--ink);
  width: 100%;
  text-align: left;
  cursor: default;
}

.recent-row:hover { background: var(--hover); }
.recent-row .glyph { color: var(--accent); display: grid; place-items: center; }
.recent-name { font-size: 13px; }
.recent-path { font-size: 11px; color: var(--ink-faint); }

.bs-field { display: flex; align-items: center; gap: 10px; margin: 10px 0; max-width: 680px; }
.bs-field label { width: 120px; color: var(--ink-soft); flex: none; }

.bs-input, .dlg input, .dlg select, .dlg textarea {
  border: 1px solid var(--line);
  border-radius: 4px;
  padding: 6px 8px;
  /* Matches .dd.lg, so a dropdown beside a text box lines up. A textarea
     overrides this with its own height, since it is meant to be tall. */
  height: 30px;
  box-sizing: border-box;
  /* WebKit paints a native select with the platform look and ignores the
     background above unless the appearance is cleared first. */
  appearance: none;
  font: inherit;
  color: var(--ink);
  background: var(--surface-3);
  flex: 1;
  -webkit-user-select: text;
  user-select: text;
}

.dlg textarea { height: auto; }
/* A value that is shown rather than set here. Still legible: it is being read,
   which is the whole reason it is on the page. */
.bs-input:disabled { background: var(--surface-2); color: var(--ink-soft); }
.bs-input::placeholder, .dlg input::placeholder { color: var(--ink-faint); }
.bs-input:focus, .dlg input:focus, .dlg select:focus, .dlg textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-dim);
}

.btn {
  border: 1px solid var(--line);
  background: var(--surface-3);
  color: var(--ink);
  border-radius: 4px;
  padding: 6px 16px;
  cursor: default;
  font: inherit;
}

.btn:hover { background: var(--surface-4); border-color: var(--accent-line); }

.btn.primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; }
.btn.primary:hover { background: var(--accent-bright); }

.btn.danger { color: var(--danger); border-color: rgba(217, 99, 106, 0.4); }
.btn.danger:hover { background: var(--danger-bg); }

.info-grid {
  display: grid;
  grid-template-columns: 180px 1fr;
  gap: 9px 18px;
  max-width: 680px;
  font-size: 12px;
}

.info-grid .k { color: var(--ink-soft); }

.ok-banner {
  background: var(--accent-dim);
  border: 1px solid var(--accent-line);
  color: var(--accent-bright);
  border-radius: 4px;
  padding: 8px 12px;
  font-size: 12px;
  max-width: 680px;
  margin: 12px 0;
}

/* ---------- import ---------- */

.imp-step {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--ink);
  margin: 22px 0 6px;
}

/* Picking the heading row by eye rather than by counting: each row is shown
   as the sheet holds it, so the real headings are obvious among the titles. */
.imp-rows {
  border: 1px solid var(--line);
  border-radius: 4px;
  max-height: 190px;
  overflow-y: auto;
  max-width: 860px;
}

.imp-row {
  display: flex;
  gap: 12px;
  align-items: baseline;
  width: 100%;
  text-align: left;
  border: 0;
  border-bottom: 1px solid var(--line-soft);
  background: transparent;
  color: var(--ink-soft);
  font: inherit;
  font-size: 12px;
  padding: 5px 10px;
  cursor: default;
}

.imp-row:last-child { border-bottom: 0; }
.imp-row:hover { background: var(--hover); }
.imp-row.on { background: var(--selection); color: var(--ink); }
.imp-rownum { color: var(--ink-faint); flex: none; width: 58px; }
.imp-rowtext { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

/* One card per column of the sheet. Wide enough for a date and a dropdown,
   narrow enough that a dozen columns fit on a screen. */
.imp-cols {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
  gap: 10px;
  max-width: 1100px;
}

.imp-col {
  border: 1px solid var(--line);
  border-radius: 4px;
  padding: 9px 10px 8px;
  background: var(--surface-3);
  min-width: 0;
}

/* A mapped column is doing something, and that has to be visible at a glance
   across a dozen cards. */
.imp-col.on { border-color: var(--accent-line); background: var(--accent-dim); }

.imp-head {
  font-size: 12px;
  font-weight: 600;
  color: var(--ink);
  margin-bottom: 6px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.imp-samples { margin-top: 7px; }

.imp-sample {
  font-size: 11px;
  color: var(--ink-faint);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  line-height: 1.55;
}

.imp-list {
  border: 1px solid var(--line);
  border-radius: 4px;
  max-height: 240px;
  overflow-y: auto;
  max-width: 860px;
}

.imp-notice {
  display: grid;
  grid-template-columns: 150px 160px 1fr;
  gap: 10px;
  font-size: 11.5px;
  padding: 5px 10px;
  border-bottom: 1px solid var(--line-soft);
  align-items: baseline;
}

.imp-notice:last-child { border-bottom: 0; }
.imp-where { color: var(--ink-faint); }
.imp-value { color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.imp-why { color: var(--ink-soft); }

/* ---------- options ---------- */

.opt-layout { display: flex; gap: 26px; align-items: flex-start; max-width: 940px; }

.opt-nav {
  width: 200px;
  flex: none;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line);
  border-radius: 6px;
  overflow: hidden;
}

.opt-nav-item {
  text-align: left;
  padding: 9px 14px;
  border: 0;
  border-left: 2px solid transparent;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  font-size: 12.5px;
}

.opt-nav-item:hover { background: var(--hover); color: var(--ink); }
.opt-nav-item.active { background: var(--accent-dim); border-left-color: var(--accent); color: var(--accent-bright); }

.opt-body { flex: 1; min-width: 0; }

.opt-head {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--ink);
  margin: 0 0 12px;
  padding-bottom: 7px;
  border-bottom: 1px solid var(--line);
}

.opt-head + .opt-head, .opt-row + .opt-head, .rcheck + .opt-head, .opt-static + .opt-head { margin-top: 26px; }

.opt-row { display: flex; align-items: flex-start; gap: 16px; margin-bottom: 12px; }

.opt-label { width: 210px; flex: none; display: flex; flex-direction: column; gap: 2px; padding-top: 6px; }
.opt-label span { color: var(--ink); font-size: 12px; }
.opt-label .opt-hint { color: var(--ink-faint); font-size: 10.5px; line-height: 1.35; }

.opt-control { flex: 1; min-width: 0; display: flex; }
.opt-control > * { flex: 1; min-width: 0; }

.opt-static { border: 1px solid var(--line); border-radius: 5px; overflow: hidden; }

/* ---------- keyboard shortcuts ---------- */

.opt-note {
  border: 1px solid var(--accent-line);
  background: var(--accent-dim);
  border-radius: 6px;
  padding: 8px 12px;
  font-size: 11.5px;
  color: var(--ink);
  margin-bottom: 14px;
}

/* Each group is a heading and its list. The gap between groups has to be
   clearly larger than the gap between rows, or the headings read as belonging
   to the list above them rather than the one below. */
.key-group + .key-group { margin-top: 26px; }
.key-group .opt-head { margin: 0 0 9px; }

.key-list { border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }

.key-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 12px;
  border-bottom: 1px solid var(--line-soft);
}
.key-row:last-child { border-bottom: none; }
.key-row:hover { background: var(--hover); }
.key-row.recording { background: var(--accent-dim); }

.key-name { flex: 1; min-width: 0; font-size: 12px; color: var(--ink); }

/* Marks a binding that has been moved off its default, so Reset means something
   visible rather than being a button that might do nothing. */
.key-changed {
  font-size: 9.5px;
  letter-spacing: 0.5px;
  text-transform: uppercase;
  color: var(--accent);
  border: 1px solid var(--accent-line);
  border-radius: 999px;
  padding: 1px 7px;
  flex: none;
}

/* The binding is the control: click it, then press the keys. */
.key-bind {
  min-width: 190px;
  text-align: left;
  padding: 4px 9px;
  border: 1px solid var(--line);
  border-radius: 5px;
  background: var(--surface-3);
  cursor: default;
  font-size: 11.5px;
}
.key-bind:hover { border-color: var(--accent-line); }
.key-row.recording .key-bind { border-color: var(--accent); }

.key-combo { color: var(--ink); font-family: var(--mono); font-size: 11px; }
.key-none { color: var(--ink-faint); font-style: italic; }
.key-listening { color: var(--accent-bright); }

.key-clear {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  flex: none;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--ink-faint);
  cursor: default;
}
.key-clear:hover { background: var(--hover); color: var(--ink); }

.opt-static-row {
  display: flex;
  justify-content: space-between;
  gap: 14px;
  padding: 8px 12px;
  font-size: 12px;
  border-bottom: 1px solid var(--line-soft);
}

.opt-static-row:last-child { border-bottom: 0; }
.opt-static-row .v { color: var(--ink-soft); }

/* ---------- fix issue ---------- */

.error-banner .grow { flex: 1; }

.fix-problem {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  padding: 12px 14px;
  background: var(--danger-bg);
  border: 1px solid rgba(217, 99, 106, 0.4);
  border-radius: 6px;
  line-height: 1.55;
  font-size: 12.5px;
}

.fix-icon { color: var(--danger); flex: none; display: grid; place-items: center; }

.fix-head {
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-soft);
  margin: 20px 0 8px;
  letter-spacing: 0.2px;
}

.fix-action { font-size: 12.5px; line-height: 1.55; color: var(--ink); }

.fix-changes { border: 1px solid var(--line); border-radius: 5px; overflow: hidden; }

.fix-change {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}

.fix-change:last-child { border-bottom: 0; }
.fix-bullet { color: var(--danger); display: grid; place-items: center; flex: none; }

/* ---------- field picker ---------- */

.field-list {
  border: 1px solid var(--line);
  border-radius: 5px;
  max-height: 380px;
  overflow-y: auto;
}

.field-group {
  position: sticky;
  top: 0;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
  padding: 7px 12px;
  font-size: 10px;
  letter-spacing: 0.9px;
  text-transform: uppercase;
  color: var(--accent);
  font-weight: 600;
  z-index: 1;
}

.field-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--line-soft);
  cursor: default;
}

.field-row:hover:not(.shown) { background: var(--accent-dim); }
.field-row.shown { opacity: 0.5; }
.field-text { flex: 1; min-width: 0; }
.field-name { font-size: 12.5px; color: var(--ink); }
.field-desc { font-size: 11px; color: var(--ink-faint); margin-top: 2px; }

.field-badge {
  flex: none;
  font-size: 10px;
  color: var(--accent);
  border: 1px solid var(--accent-line);
  border-radius: 999px;
  padding: 2px 9px;
}

/* ---------- predecessor picker ---------- */

.pred-list {
  border: 1px solid var(--line);
  border-radius: 5px;
  max-height: 320px;
  overflow-y: auto;
  margin: 6px 0 4px;
}

.pred-row { border-bottom: 1px solid var(--line-soft); }
.pred-row:last-child { border-bottom: 0; }
.pred-row.on { background: var(--accent-dim); }

.pred-pick {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 28px;
  padding-right: 10px;
  cursor: default;
}

.pred-row:not(.on) .pred-pick:hover { background: var(--hover); }

.pred-id {
  min-width: 22px;
  text-align: right;
  color: var(--ink-faint);
  font-size: 11px;
  flex: none;
}

.pred-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.pred-row.summary .pred-name { font-weight: 600; }
.pred-row.on .pred-name { color: var(--accent-bright); }

.pred-detail {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 0 10px 8px 46px;
}

.pred-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 10px;
}

/* ---------- colour rows ---------- */

.colour-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 7px 0;
  border-bottom: 1px solid var(--line-soft);
}

.colour-swatch {
  width: 30px;
  height: 14px;
  border-radius: 3px;
  border: 1px solid var(--line);
  flex: none;
}

.colour-name { flex: 1; font-size: 12px; }
.colour-hex { font-family: var(--mono); font-size: 11px; color: var(--ink-faint); width: 74px; text-align: right; }

.colour-picker {
  width: 44px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: transparent;
  cursor: default;
  flex: none;
}

.colour-picker::-webkit-color-swatch-wrapper { padding: 2px; }
.colour-picker::-webkit-color-swatch { border: 0; border-radius: 2px; }

/* ---------- quick access editor ---------- */

.qat-list { border: 1px solid var(--line); border-radius: 5px; overflow: hidden; }

.qat-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 34px;
  padding: 0 6px 0 11px;
  border-bottom: 1px solid var(--line-soft);
}

.qat-item:last-child { border-bottom: 0; }
.qat-item:hover { background: var(--hover); }
.qat-item .qat-glyph { display: grid; place-items: center; width: 18px; flex: none; color: var(--accent); }
.qat-item .qat-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }

/* ---------- about ---------- */

.about-wrap {
  display: flex;
  justify-content: center;
  padding: 8px 0 32px;
}

.about-card {
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 14px;
  padding: 36px 40px 32px;
  width: 100%;
  max-width: 620px;
}

.about-brand {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  text-align: center;
  padding-bottom: 26px;
  border-bottom: 1px solid var(--line);
}

.about-logo { color: var(--ink); }
.about-logo svg { width: 100%; height: 100%; display: block; }

.about-name {
  font-size: 27px;
  font-weight: 600;
  letter-spacing: -0.4px;
  color: var(--ink);
}

.about-pills { display: flex; gap: 8px; flex-wrap: wrap; justify-content: center; }

.pill {
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 3px 12px;
  font-size: 11px;
  color: var(--ink-soft);
}

.pill.accent { border-color: var(--accent-line); color: var(--accent-bright); background: var(--accent-dim); }

.about-rows { margin-top: 22px; }

.about-row {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 9px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12.5px;
}

.about-row .k { color: var(--ink-soft); }
.about-row .v { color: var(--ink); text-align: right; }

.about-attr-btn {
  margin: 24px auto 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  background: transparent;
  border: 1px solid var(--accent-line);
  border-radius: 9px;
  color: var(--accent-bright);
  font-size: 12.5px;
  font-weight: 600;
  cursor: default;
}

.about-attr-btn:hover { background: var(--accent-dim); border-color: var(--accent); }

.attr-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  padding: 6px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}

.attr-name { flex: none; width: 130px; color: var(--ink); }
.attr-license { flex: none; width: 130px; color: var(--ink-soft); font-size: 11px; }
.attr-url { flex: 1; color: var(--ink-faint); font-size: 11px; overflow: hidden; text-overflow: ellipsis; }

/* print preview */
/* The document on the left, where it is going on the right. The preview takes
   the room because it is the thing being judged. */
.print-layout { display: flex; gap: 22px; align-items: stretch; min-height: 0; }
.print-preview { flex: 1 1 auto; min-width: 0; display: flex; }
.print-settings {
  width: 300px;
  flex: none;
  overflow-y: auto;
  max-height: calc(100vh - 210px);
  padding-right: 4px;
}

/* Shown when the engine has no PDF viewer of its own to hand. */
.print-fallback {
  display: grid;
  place-items: center;
  height: 100%;
  padding: 24px;
  text-align: center;
  color: var(--ink-soft);
  font-size: 12px;
}

/* ---------- print queues ---------- */

.queue-list { display: flex; flex-direction: column; gap: 4px; margin-bottom: 12px; }

.queue {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface-3);
  cursor: default;
  text-align: left;
}
.queue:hover { border-color: var(--accent-line); }
.queue.on { border-color: var(--accent); background: var(--accent-dim); }
.queue .glyph { display: grid; place-items: center; flex: none; color: var(--ink-faint); }
.queue.on .glyph { color: var(--accent); }

.queue-text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.queue-name { display: flex; align-items: center; gap: 7px; font-size: 12px; color: var(--ink); }
.queue-status { font-size: 10.5px; color: var(--ink-faint); }

.queue-default {
  font-size: 9px;
  letter-spacing: 0.5px;
  text-transform: uppercase;
  color: var(--accent);
  border: 1px solid var(--accent-line);
  border-radius: 999px;
  padding: 0 6px;
}

/* Buttons on the Print page carry a glyph, so they lay out as a row rather
   than as a bare label. */
.print-settings .btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.print-settings .btn .glyph { display: grid; place-items: center; }
.print-go { width: 100%; padding: 10px 14px; font-size: 13px; }

/* What the printed document will look like, stated rather than guessed at. */
.print-fact {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 0;
  border-bottom: 1px solid var(--line);
  font-size: 11.5px;
}
.print-fact:last-child { border-bottom: none; }
.pf-label { color: var(--ink-faint); }
.pf-value { color: var(--ink); text-align: right; }

.print-frame {
  flex: 1 1 auto;
  width: 100%;
  min-width: 0;
  height: calc(100vh - 210px);
  border: 1px solid var(--line);
  border-radius: 6px;
  background: #f2f5f5;
  box-shadow: var(--shadow);
}

/* ---------- timeline band ---------- */

.timeline {
  background: var(--surface);
  border-bottom: 1px solid var(--line);
  flex: none;
  position: relative;
  overflow: hidden;
  padding: 4px 10px;
}

/* The contextual banner with nothing to announce: still there, still the same
   height, just not saying anything. */
.tools-banner.empty { visibility: hidden; }

.timeline-caption {
  position: absolute;
  left: 9px;
  top: 3px;
  font-size: 9px;
  color: var(--ink-faint);
  letter-spacing: 0.7px;
  text-transform: uppercase;
}

/* A transparent sheet over everything, so a drag keeps receiving events
   however fast the pointer moves or wherever it wanders. */
.drag-shield {
  position: fixed;
  inset: 0;
  z-index: 150;
  background: transparent;
}

.drag-shield.col-resize { cursor: col-resize; }
.drag-shield.grabbing { cursor: grabbing; }

/* ---------- icon buttons ---------- */

.iconbtn {
  width: 22px;
  height: 22px;
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 4px;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  line-height: 1;
  font-size: 12px;
}

.iconbtn svg { display: block; }
.iconbtn:hover:not(:disabled) { background: var(--hover); border-color: var(--line); color: var(--accent-bright); }
.iconbtn:active:not(:disabled) { background: var(--pressed); }
.iconbtn:disabled { opacity: 0.3; }
.iconbtn.danger:hover:not(:disabled) { background: var(--danger-bg); border-color: var(--danger); color: var(--danger); }

/* a row of icon buttons that reads as one control */
.btn-group { display: inline-flex; align-items: center; gap: 2px; }

/* ---------- internal window panes ---------- */

.panes {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  padding: 6px 6px 0;
  gap: 0;
}

.pane-bar { display: flex; flex: none; gap: 6px; min-width: 0; }

.pane-tab {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 25px;
  padding: 0 10px;
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-bottom: 0;
  border-radius: 6px 6px 0 0;
  font-size: 11px;
  color: var(--ink-soft);
  flex: none;
  min-width: 0;
}

.pane-tab.grow { flex: 1; }
.pane-tab.active { color: var(--accent-bright); background: var(--surface-3); }
/* The name takes the slack, so the button lands on the tab's right edge
   whether or not there is a subtitle to sit beside it. */
.pane-tab .pane-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pane-tab .pane-sub {
  color: var(--ink-faint);
  font-size: 10px;
  flex: none;
  padding-right: 4px;
  white-space: nowrap;
}
.pane-tab .pane-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); flex: none; opacity: 0.75; }

/* The tab's own button sits flush with the right edge of the tab. */
.pane-tab .iconbtn {
  width: 20px;
  height: 19px;
  flex: none;
  margin-left: 4px;
  margin-right: -2px;
}
.pane-tab .iconbtn:not(:hover) { color: var(--ink-faint); }

.pane-frame {
  flex: 1 1 auto;
  min-height: 0;
  min-width: 0;
  display: flex;
  border: 1px solid var(--line);
  border-radius: 0 6px 0 0;
  background: var(--surface);
  overflow: hidden;
}

/* ---------- workspace ---------- */

.workspace { flex: 1; display: flex; min-height: 0; background: var(--bg); }

.viewbar {
  width: 22px;
  background: var(--surface-2);
  color: var(--ink-soft);
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  border-right: 1px solid var(--line);
  /* The label inside is turned on its side and told not to wrap, so it is far
     wider than this bar until something turns it. A renderer that does not
     support vertical writing leaves it lying flat, and a flat line of text
     that is told not to wrap runs straight out of the window and paints over
     whatever is beside it. Clipping does not make that readable, but it does
     keep a missing feature inside its own 22 pixels instead of across the
     screen. The label itself still needs rebuilding without vertical writing
     mode, which is a separate job. */
  overflow: hidden;
}

.viewbar span {
  writing-mode: vertical-rl;
  transform: rotate(180deg);
  font-size: 10.5px;
  letter-spacing: 0.8px;
  white-space: nowrap;
}

.split {
  flex: 1 1 auto;
  display: flex;
  align-items: stretch;
  min-width: 0;
  min-height: 0;
  /* Each pane scrolls itself, so this only frames them. */
  overflow: hidden;
  background: var(--surface);
}

.split.hide-table .pane-left { display: none; }
.split.hide-chart .chart-pane { display: none; }

.pane-left {
  display: flex;
  align-items: stretch;
  flex: none;
  min-width: 0;
  background: var(--surface);
}

.splitter {
  position: relative;
  width: 5px;
  background: var(--line);
  cursor: col-resize;
  flex: none;
  align-self: stretch;
  min-height: 100%;
}

/* A wider invisible grip, so the splitter is easy to catch. */
.splitter::after {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  left: -4px;
  right: -4px;
}

.splitter:hover { background: var(--accent); }

/* While resizing, nothing under the pointer may react. */
.split.resizing { cursor: col-resize; }
.split.resizing .grid,
.split.resizing .chart-svg { pointer-events: none; }
.split.resizing .splitter { background: var(--accent); }

/* While a row is being dragged the cursor says so throughout. */
.split.row-dragging { cursor: grabbing; }
.split.row-dragging .chart-svg { pointer-events: none; }

/* ---------- task grid ---------- */

/* Stands in for the rows outside the viewport, so the pane scrolls its full
   height without those rows existing. It must not take any styling of its own,
   or the striping and borders would show a seam where the drawn rows end. */
.row-spacer { border: none; background: none; }
.row-spacer td { padding: 0; border: none; }

/* Wide tables scroll sideways on their own, without moving the chart. */
.grid-pane {
  flex: none;
  background: var(--surface);
  overflow: auto;
  min-width: 0;
  /* So other people's pointers can be placed in the table's own coordinates.
     They are children of this box, which means the pane scrolls them along
     with the rows they are on and clips the ones that have scrolled off. */
  position: relative;
}

/* The table pane is sized to its columns exactly. A vertical scrollbar here
   would eat into that width and force a pointless horizontal scrollbar, so it
   is hidden: vertical scrolling is driven from the chart's bar and the wheel,
   and the two panes are kept in step anyway. */
.grid-pane::-webkit-scrollbar:vertical { width: 0; }

.grid { border-collapse: collapse; table-layout: fixed; font-size: 12px; width: 100%; }

.grid th {
  position: sticky;
  top: 0;
  z-index: 3;
  overflow: visible;
  background: var(--grid-header);
  border: 1px solid var(--grid-line);
  border-top: 0;
  height: 38px;
  font-weight: 500;
  font-size: 11px;
  color: var(--ink-soft);
  padding: 2px 6px;
  text-align: left;
  white-space: nowrap;
}

.grid th.num { text-align: center; }

.grid td {
  border: 1px solid var(--grid-line);
  height: 22px;
  padding: 0 6px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--ink);
}

/* The gridline choices, as classes on the table so one toggle repaints
   without every cell carrying its own style. */
.grid.no-rows td { border-top-color: transparent; border-bottom-color: transparent; }
.grid.no-columns td { border-left-color: transparent; border-right-color: transparent; }

.grid tr.row:hover td { background: var(--hover); }
.grid tr.row.selected td { background: var(--selection); }
.grid tr.row.summary td { font-weight: 600; color: var(--ink); }
.grid tr.row.inactive td { color: var(--ink-faint); text-decoration: line-through; }
/* Criticality reads as a quiet marker on the row number, not red text. */
.grid tr.row.critical td.rownum {
  box-shadow: inset 2px 0 0 var(--bar-critical-edge);
}

.grid tr.row.dragging td { opacity: 0.45; }
.grid tr.row.drop-above td { box-shadow: inset 0 2px 0 var(--accent); }
.grid tr.row.drop-below td { box-shadow: inset 0 -2px 0 var(--accent); }
.grid tr.row.drop-into td { box-shadow: inset 0 0 0 1px var(--accent); }

/* A grouping band: a heading over the rows beneath it, spanning every column.
   It is not a task, so none of the row states above can reach it, and it takes
   the header's surface to read as a divider rather than as an empty row. */
.grid tr.row.band td {
  background: var(--grid-header);
  border-left: 0;
  border-right: 0;
  color: var(--ink-soft);
  font-size: 11px;
  cursor: default;
}

.grid tr.row.band:hover td { background: var(--grid-header); }
.grid tr.row.band .band-label { font-weight: 600; color: var(--ink); }
.grid tr.row.band .band-totals { margin-left: 10px; }

.grid td.rownum {
  background: var(--grid-header);
  text-align: center;
  color: var(--ink-faint);
  font-size: 11px;
  position: sticky;
  left: 0;
  z-index: 2;
  cursor: grab;
  user-select: none;
}

.grid td.rownum:hover { color: var(--accent-bright); background: var(--surface-4); }
.grid td.rownum:active { cursor: grabbing; }

.grid tr.row.selected td.rownum { background: var(--accent-dim); color: var(--accent-bright); }

.grid td.c-num { text-align: right; }
.grid td.c-mid { text-align: center; }

/* Symbols in a cell are centred on the row, not sat on the text baseline,
   which is what made them look a pixel or two high. */
.grid td.c-mid > span,
.grid td.rownum {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  line-height: 1;
}

.grid .ind-critical { color: var(--warn); font-size: 12px; line-height: 1; }
.grid .mode-glyph { display: grid; place-items: center; line-height: 1; }

/* The grip that resizes a column, straddling its right-hand border. */
/* Sits centred on the 1px divider between this column and the next, so the
   line the planner is aiming at is the line they grab. The cell's padding box
   ends at right: 0, the border occupies the next pixel, so a 9px grab area
   pulled 5px past that edge is centred on it. */
.col-grip {
  position: absolute;
  top: 0;
  right: -5px;
  width: 9px;
  height: 100%;
  cursor: col-resize;
  z-index: 4;
}

.col-grip::after {
  content: "";
  position: absolute;
  top: 5px;
  bottom: 5px;
  left: 4px;
  width: 1px;
  background: transparent;
}

.col-grip:hover::after { background: var(--accent); }

/* The typed alternative under the predecessor picker's list. */
/* The pickers appear both in a floating popup and inside a dialog tab. Inside
   a dialog the popup's own header is redundant, and the list wants to use the
   height the tab already has. */
.dlg .picker .ctxheader { display: none; }
.dlg .picker .pred-list { max-height: 300px; }

/* Lifted over the picker's scrim, which covers the whole window while the list
   is open. The box and the list are one edit, so a click meant for the box has
   to reach it: without this, moving the caret, or pressing the caret button to
   bring the list back, lands on the scrim and abandons the edit instead. */
.picker-cell {
  position: relative;
  z-index: 82;
  display: flex;
  align-items: center;
  width: 100%;
  height: 100%;
}
.picker-cell .cell-input { flex: 1; min-width: 0; }
/* The cell looks like a plain text box otherwise, and nothing says a list is
   behind it. */
.picker-caret {
  flex: none;
  width: 16px;
  height: 100%;
  border: 0;
  padding: 0;
  cursor: pointer;
  font-size: 9px;
  color: var(--ink-soft);
  background: transparent;
}
.picker-caret:hover { color: var(--accent-bright); }

/* ---------- other people's pointers ---------- */

/* Never in the way. A pointer sits over cells that are clicked, typed in and
   dragged, so the whole layer is transparent to the mouse: nothing under one
   of these ever becomes unreachable because somebody else is looking at it. */
.cursors {
  position: absolute;
  inset: 0;
  pointer-events: none;
  /* Over the rows and the bars, under the sticky headers, which have to stay
     readable and are not part of the plan anybody is pointing at. */
  z-index: 2;
}

/* The tip is the point. Everything else hangs off it, so the arrow's own
   corner is what lands on the coordinate.

   The transition is what makes somebody else's pointer readable. Positions
   arrive at a modest rate, because a mouse produces events far faster than
   anything should put on a wire and every message costs the receiving copy a
   redraw. Told about eight positions a second and drawn at eight positions a
   second, a pointer is a slideshow; told about eight and glided between them,
   it is a pointer. The browser does the interpolating, so this costs nothing
   here.

   The duration is a little longer than the gap between updates: shorter and
   the pointer arrives early and waits, which stutters, and much longer and it
   visibly trails where the person actually is. Linear, not eased: easing
   between a stream of positions accelerates and decelerates between every
   pair, which reads as a stutter of its own. */
.cursor {
  position: absolute;
  transition: left 140ms linear, top 140ms linear;
}

.cursor-arrow { display: block; }

.cursor-label {
  position: absolute;
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 2px 7px 2px 2px;
  border-radius: 999px;
  /* White on the peer's own colour, which is chosen light enough to carry it
     in either theme. */
  color: #0d1717;
  font-size: 10.5px;
  line-height: 1.5;
  white-space: nowrap;
  box-shadow: var(--shadow);
}

.cursor-name { font-weight: 600; max-width: 132px; overflow: hidden; text-overflow: ellipsis; }

.cursor-face {
  flex: none;
  width: 15px;
  height: 15px;
  border-radius: 999px;
  object-fit: cover;
  background: var(--surface);
}

/* The letters stand in for a face. Centred by grid rather than by line height
   so a pair of capitals sits properly in a circle this small. */
.cursor-face.initials {
  display: grid;
  place-items: center;
  font-size: 8px;
  font-weight: 700;
  letter-spacing: 0.2px;
}

/* A cell somebody else has open, in their colour, so two people do not both
   start typing in one without knowing. */
.peer-cell {
  position: absolute;
  height: 22px;
  border: 1.5px solid;
  border-radius: 3px;
  pointer-events: none;
  box-sizing: border-box;
}

/* What they have typed and not committed. Above the cell rather than in it:
   inside, it would read as what the plan says, and the plan does not say it
   until they commit. */
.peer-draft {
  position: absolute;
  left: -1.5px;
  bottom: 100%;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  padding: 1px 5px;
  border-radius: 3px 3px 3px 0;
  color: #0d1717;
  font-size: 10.5px;
  line-height: 1.5;
}

/* ---------- critical path report ---------- */

.cp-legend {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--ink-soft);
  font-size: 11px;
  margin-bottom: 18px;
  line-height: 1.5;
}
.cp-swatch {
  flex: none;
  width: 20px;
  height: 10px;
  border-radius: 2px;
  background: var(--danger);
}

.pred-type { display: flex; align-items: center; gap: 8px; padding: 8px 10px 0; }
.pred-type label { color: var(--ink-soft); font-size: 11px; flex: none; }
.pred-type .bs-input { flex: 1; min-width: 0; }

/* Sticky already makes the header a containing block, which is what lets the
   resize grip position itself against the cell's own edge. */
.grid th { position: sticky; }
.grid th .th-inner {
  position: relative;
  display: flex;
  align-items: center;
  height: 100%;
}

.grid th.num .th-inner,
.grid th.c-mid .th-inner { justify-content: center; }

.grid th .th-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--ink-soft);
}

.cell-name { display: flex; align-items: center; gap: 3px; }

.twisty {
  width: 12px;
  flex: none;
  text-align: center;
  font-size: 8px;
  color: var(--ink-faint);
  cursor: default;
}

.cell-input {
  width: 100%;
  height: 20px;
  border: 1px solid var(--accent);
  outline: none;
  font: inherit;
  padding: 0 4px;
  background: var(--surface-4);
  color: var(--ink);
  -webkit-user-select: text;
  user-select: text;
}

.add-row td { color: var(--ink-faint); font-style: italic; }

/* Manual is the state worth noticing: it means the scheduler is not moving
   this row, which is why a date can look wrong. Auto is the norm, so it stays
   quiet. */
.mode-glyph { display: grid; place-items: center; color: var(--ink-faint); }
.mode-glyph.manual { color: var(--contextual); }
.mode-glyph.auto { color: var(--ink-faint); }

/* ---------- gantt chart ---------- */

.chart-pane {
  flex: 1 1 auto;
  position: relative;
  background: var(--surface);
  overflow: auto;
  min-width: 0;
}

/* The chart standing in for a report figure. A report is a picture of one
   chain rather than a pane you scroll, so it is sized by the chart inside it
   and the whole thing is there at once. */
/* The critical path's left window: the report itself, scrolling on its own
   beside the chart, the way the entry table sits beside the plan. */
.cp-report {
  flex: 1 1 auto;
  overflow: auto;
  padding: 14px;
}
.cp-report .rep-head { margin-top: 0; }

/* The print page leads with the command and the copy count, the way Project
   does, so the first thing in reach is the thing you came to do. */
.print-action {
  display: flex;
  align-items: center;
  gap: 14px;
  margin-bottom: 16px;
}
.print-action .print-go { flex: none; }
.print-copies { display: flex; align-items: center; gap: 8px; }
.print-copies label { color: var(--ink-soft); font-size: 11px; }
.print-copies input {
  width: 58px;
  height: 30px;
  box-sizing: border-box;
  border: 1px solid var(--line);
  border-radius: 4px;
  padding: 0 8px;
  color: var(--ink);
  background: var(--surface-3);
  font: inherit;
}
.print-go[disabled] { opacity: 0.45; cursor: default; }

/* Paging through the preview, the way Project's print view does. */
.print-range { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.print-range label { color: var(--ink-soft); font-size: 11px; flex: none; }
.print-range input {
  width: 66px;
  height: 30px;
  box-sizing: border-box;
  border: 1px solid var(--line);
  border-radius: 4px;
  padding: 0 8px;
  color: var(--ink);
  background: var(--surface-3);
  font: inherit;
}

/* Pointing at a row here outlines its bar in the chart, and pointing at a bar
   outlines the row. Two panes, one answer. */
.rep-table tr.hot td {
  background: var(--selection);
  box-shadow: inset 0 0 0 1px var(--accent-bright);
}
.rep-table tbody tr { cursor: default; }

/* A report's chart sits in the same split the plan does, so it needs no
   overrides of its own: the window gives it a height, and it scrolls sideways
   exactly as the main one does. */

/* Holds the chart's own width so the pane above can scroll to it. */
.chart-canvas { display: block; }
.chart-head { position: sticky; top: 0; z-index: 4; background: var(--grid-header); }
.chart-svg { display: block; }

.tl-major, .tl-minor { font-size: 10px; fill: var(--ink-soft); }
.tl-major { fill: var(--ink); }
.tl-minor.weekend { fill: var(--ink-faint); }

.bar-label { font-size: 10px; fill: var(--ink-soft); dominant-baseline: middle; }

/* Annotation shapes. The group is inert as a whole and each shape opts back
   in, so an unfilled outline is still clickable while the empty space between
   two shapes lets the pointer through to the bars underneath. */
.drawings { pointer-events: none; }
.draw-text { dominant-baseline: middle; user-select: none; }

/* Timeline band labels. One beside its bar reads as ordinary text; one within
   a bar sits on the bar's own colour, so it takes the dark ink instead. */
.band-label { font-size: 10px; fill: var(--ink-soft); }
.band-label.in { fill: var(--on-accent); font-weight: 600; }

/* ---------- reports ---------- */

.reports-pane {
  flex: 1 1 auto;
  overflow: auto;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  background: var(--surface);
}
.report-row { display: flex; gap: 14px; align-items: stretch; flex-wrap: wrap; }

.report-card {
  flex: 1 1 380px;
  min-width: 320px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-3);
  padding: 12px 14px 10px;
}
.report-head { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
.report-title { font-size: 13px; font-weight: 650; color: var(--ink); }
.report-note { font-size: 11px; color: var(--ink-faint); }
.report-chart { display: block; margin: 10px 0 6px; overflow: visible; }
.axis-label { font-size: 9.5px; fill: var(--ink-faint); }
.axis-title { font-size: 9.5px; fill: var(--ink-soft); letter-spacing: 0.4px; }

.report-legend { display: flex; gap: 14px; font-size: 10.5px; color: var(--ink-soft); }
.report-legend span { display: inline-flex; align-items: center; gap: 6px; }
.report-legend .sw { width: 14px; height: 3px; border-radius: 2px; display: inline-block; }
/* Dashed in the key because it is dashed on the chart: a solid swatch beside a
   dashed line is a legend that points at the wrong series. */
.report-legend .sw.ideal {
  border-radius: 0;
  background: repeating-linear-gradient(
    to right, var(--ink-faint) 0 5px, transparent 5px 9px);
}
.report-legend .sw.actual { background: var(--accent-bright); }
.report-legend .sw.scope { background: var(--ink-faint); }
.report-legend .sw.done { background: var(--bar-progress); }
.report-legend .sw.planned { background: var(--bar); }
.report-legend .sw.average {
  border-radius: 0;
  background: repeating-linear-gradient(
    to right, var(--accent-bright) 0 5px, transparent 5px 9px);
}

/* Velocity: planned behind, completed in front, so the gap between them is
   the thing you read rather than two bars to compare by eye. */
.velocity {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  height: 170px;
  margin: 10px 0 6px;
  padding-bottom: 16px;
  padding-left: 78px;
  position: relative;
}

/* What the bars count, said once up the side, the way the burn charts say it. */
.vel-title {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 16px;
  width: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  writing-mode: vertical-rl;
  transform: rotate(180deg);
  font-size: 9.5px;
  color: var(--ink-soft);
  letter-spacing: 0.4px;
  white-space: nowrap;
}

/* The scale the bars are read against. Bars with no axis show which iteration
   was busiest but not by how much. */
.vel-axis { position: absolute; left: 16px; top: 0; bottom: 16px; width: 56px; }
.vel-tick {
  position: absolute;
  right: 6px;
  transform: translateY(50%);
  font-size: 9.5px;
  color: var(--ink-faint);
  white-space: nowrap;
}
.vel-grid {
  position: absolute;
  left: 78px;
  right: 0;
  border-top: 1px solid var(--grid-line);
}

/* The average, which is the line a velocity chart is actually read against:
   without it a column of bars says which iteration was busiest and nothing
   about whether the team is holding its pace. */
.vel-average {
  position: absolute;
  left: 78px;
  right: 0;
  border-top: 1.5px dashed var(--accent-bright);
  pointer-events: none;
}
.vel-col { flex: 1 1 0; min-width: 10px; height: 100%; position: relative; }
.vel-stack { position: relative; height: 100%; }
.vel-planned, .vel-done {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  border-radius: 3px 3px 0 0;
}
.vel-planned { background: var(--bar); opacity: 0.35; }
.vel-done { background: var(--bar-progress); }
/* The iteration in progress is not a finished number, so it is drawn as one
   still being filled in rather than as a delivered total. */
.vel-done.running { background: var(--bar-progress); opacity: 0.55; }
.vel-label {
  position: absolute;
  bottom: -15px;
  left: 0;
  right: 0;
  text-align: center;
  font-size: 9.5px;
  color: var(--ink-faint);
}

/* The path reads as a chain: each step, then the link that carries it onward.
   A flat list would say which tasks are critical but not in what order, which
   is the part that actually matters. */
.crit-list { max-height: 170px; overflow-y: auto; margin-top: 8px; }
.crit-step { display: flex; flex-direction: column; }
.crit-joint {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 1px 0 1px 30px;
  font-size: 9.5px;
  color: var(--ink-faint);
}
.crit-arrow { color: var(--accent); }
.crit-dur { color: var(--ink-faint); font-size: 10.5px; white-space: nowrap; }
.crit-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding: 4px 0;
  font-size: 11.5px;
}
.crit-id { color: var(--ink-faint); min-width: 26px; }
.crit-name { flex: 1; min-width: 0; color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.crit-dates { color: var(--ink-soft); font-size: 10.5px; white-space: nowrap; }

/* A resource in the usage view heads the bookings under it, so it is ruled off
   from the group above rather than left to run into it. */
.sheet tr.usage-head td { border-top: 1px solid var(--line); }

/* ---------- report pages ---------- */

.rep-head { margin-bottom: 16px; }
.rep-title { font-size: 22px; font-weight: 650; color: var(--ink); margin: 0 0 6px; letter-spacing: -0.3px; }
.rep-sub { margin: 0; font-size: 12px; color: var(--ink-soft); line-height: 1.6; max-width: 760px; }

/* A chart says a shape but not a number, so the figures come first. */
.rep-figures { display: flex; gap: 10px; flex-wrap: wrap; margin-bottom: 16px; }
.rep-figure {
  flex: 1 1 150px;
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.rep-value { font-size: 19px; font-weight: 650; color: var(--ink); letter-spacing: -0.3px; }
.rep-label { font-size: 9.5px; letter-spacing: 0.9px; text-transform: uppercase; color: var(--ink-faint); }

.rep-chart-box {
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-3);
  padding: 14px;
  margin-bottom: 18px;
}
.velocity.tall { height: 260px; }

/* Said when there is nothing to draw. Axes with no lines and no explanation
   read as a broken chart rather than as an empty one. */
.rep-chart-note {
  margin: 6px 0 8px;
  font-size: 11px;
  color: var(--ink-soft);
  line-height: 1.5;
}

.rep-section {
  font-size: 13px;
  font-weight: 650;
  color: var(--ink);
  margin: 0 0 8px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--line);
}
.rep-table { width: 100%; border-collapse: collapse; font-size: 11.5px; }
.rep-table thead th {
  text-align: left;
  font-size: 9.5px;
  letter-spacing: 0.6px;
  text-transform: uppercase;
  color: var(--ink-faint);
  font-weight: 600;
  padding: 6px 8px;
  border-bottom: 1px solid var(--line);
}
.rep-table td { padding: 6px 8px; border-bottom: 1px solid var(--line-soft); color: var(--ink); }
.rep-table tbody tr:hover { background: var(--hover); }
.rep-table .n { text-align: right; }
.rep-table .muted { color: var(--ink-soft); }

/* ---------- colour commands ---------- */

/* A row command that also carries a swatch: glyph, label, then the colour. */
.swatch-btn { display: flex; align-items: center; gap: 6px; }
.swatch-btn .colour-bar { width: 14px; height: 10px; border-radius: 2px; margin-left: 2px; }

.colour-btn-wrap { position: relative; display: inline-flex; }

/* Glyph above, the colour it will apply below, the way Office does it. */
.colour-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1px;
  padding: 3px 4px 2px;
}
.colour-bar {
  display: block;
  width: 15px;
  height: 3px;
  border-radius: 1px;
  border: 1px solid var(--line);
}

.colour-pop {
  position: absolute;
  top: 100%;
  left: 0;
  z-index: 60;
  margin-top: 4px;
  padding: 8px;
  border: 1px solid var(--line);
  border-radius: 7px;
  background: var(--surface-4);
  box-shadow: var(--shadow);
}
.colour-grid {
  display: grid;
  grid-template-columns: repeat(4, 20px);
  gap: 5px;
  margin-bottom: 7px;
}
.colour-chip {
  width: 20px;
  height: 20px;
  border-radius: 4px;
  border: 1px solid var(--line);
  cursor: default;
  padding: 0;
}
.colour-chip:hover { outline: 2px solid var(--accent); outline-offset: 1px; }
.colour-clear {
  width: 100%;
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  font-size: 10.5px;
  cursor: default;
  padding: 3px;
  border-radius: 4px;
  white-space: nowrap;
}
.colour-clear:hover { background: var(--hover); color: var(--ink); }

/* ---------- external dependencies ---------- */

.ext-add { display: flex; gap: 8px; margin-bottom: 14px; }
.ext-add .bs-input { flex: 1; min-width: 0; }

.ext-list { display: flex; flex-direction: column; gap: 4px; }
.ext-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface-3);
}
.ext-main { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
.ext-ref { font-family: var(--mono); font-size: 11.5px; color: var(--accent); }
.ext-label { font-size: 12px; color: var(--ink); }
.ext-date { max-width: 132px; font-size: 11px; padding: 3px 8px; }
.ext-users { font-size: 10.5px; color: var(--ink-faint); }
.ext-acts { display: flex; align-items: center; gap: 6px; flex: none; }

/* ---------- custom fields ---------- */

.cf-pick { display: flex; gap: 14px; margin-bottom: 6px; }
.cf-pick .bs-field { flex: 1; }

.cf-indicator {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 8px;
  border: 1px solid var(--line);
  border-radius: 6px;
  margin-bottom: 5px;
  background: var(--surface-3);
}
.cf-glyph { font-size: 14px; color: var(--accent); width: 18px; text-align: center; }
.cf-rule { font-size: 11.5px; color: var(--ink); }
.cf-meaning { flex: 1; font-size: 10.5px; color: var(--ink-faint); }

/* The fields this plan already uses, as a way back to one. */
.cf-inuse { display: flex; flex-wrap: wrap; gap: 6px; }
.cf-chip {
  display: flex;
  align-items: baseline;
  gap: 7px;
  padding: 4px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: var(--surface-3);
  cursor: default;
}
.cf-chip:hover { border-color: var(--accent-line); }
.cf-chip-name { font-size: 11.5px; color: var(--ink); }
.cf-chip-slot { font-size: 9.5px; color: var(--ink-faint); font-family: var(--mono); }

/* ---------- change log ---------- */

.hist-tally { display: flex; align-items: baseline; gap: 14px; font-size: 12px; color: var(--ink); }
.hist-unsent { color: var(--accent-bright); }
.hist-when { color: var(--ink-soft); white-space: nowrap; }
/* The command is the stored form, so it is shown as written rather than
   reflowed into prose. */
.hist-cmd { font-family: var(--mono); font-size: 11px; color: var(--accent); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 250px; }
.hist-more { color: var(--ink-faint); font-family: var(--font); font-size: 10.5px; }

/* ---------- dictionaries ---------- */

.dict-list { margin-top: 8px; }
.dict-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 7px 2px;
  border-bottom: 1px solid var(--line-soft);
}
.dict-row:last-child { border-bottom: none; }
.dict-name { flex: 1; min-width: 0; font-size: 12px; color: var(--ink); }
.dict-code { font-family: var(--mono); font-size: 10.5px; color: var(--ink-faint); }
.dict-size { font-size: 10.5px; color: var(--ink-faint); min-width: 58px; text-align: right; }
.dict-state { font-size: 11px; color: var(--accent-bright); }

.dict-note {
  margin: 10px 0 4px;
  padding: 8px 12px;
  border-radius: 6px;
  font-size: 11.5px;
  border: 1px solid var(--line);
}
.dict-note.ok { background: var(--accent-dim); border-color: var(--accent-line); color: var(--ink); }
.dict-note.bad { background: var(--danger-bg); border-color: var(--danger); color: var(--ink); }

/* ---------- spelling panel ---------- */

/* Floats over the right of the workspace: the plan stays visible, because a
   correction only makes sense next to the row it belongs to. */
.spell-panel {
  position: fixed;
  top: 108px;
  right: 12px;
  bottom: 34px;
  width: 460px;
  max-width: calc(100vw - 24px);
  z-index: 55;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--surface-4);
  box-shadow: var(--shadow);
  overflow: hidden;
}
.spell-panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 10px 9px 14px;
  border-bottom: 1px solid var(--line);
  background: var(--surface-3);
}
.spell-panel-title { font-size: 12.5px; font-weight: 650; color: var(--ink); }
.spell-panel-body { flex: 1; overflow-y: auto; padding: 12px 14px 14px; }
.spell-panel-body .report-card { border: 0; background: transparent; padding: 0; margin-bottom: 14px; }

/* ---------- spelling ---------- */

.spell-hint {
  margin: 8px 0 0;
  padding: 10px 12px;
  border-radius: 6px;
  background: var(--surface-2);
  border: 1px solid var(--line);
  font-family: var(--mono);
  font-size: 11.5px;
  color: var(--ink);
  white-space: pre;
}

.spell-list { margin-top: 10px; max-height: calc(100vh - 300px); overflow-y: auto; }

.spell-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 8px 4px;
  border-bottom: 1px solid var(--line-soft);
}
.spell-row:last-child { border-bottom: none; }

.spell-main { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 10px; }
.spell-word {
  font-weight: 650;
  color: var(--warn);
  font-size: 12.5px;
  text-decoration: underline wavy var(--warn);
  text-underline-offset: 3px;
}
.spell-where { font-size: 10px; color: var(--ink-faint); white-space: nowrap; }
.spell-context {
  flex: 1;
  min-width: 0;
  font-size: 11px;
  color: var(--ink-soft);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.spell-acts { display: flex; gap: 6px; flex: none; align-items: center; }
.spell-fix, .spell-skip {
  border-radius: 4px;
  padding: 3px 10px;
  font-size: 11px;
  cursor: default;
  border: 1px solid var(--line);
  background: var(--surface-3);
  color: var(--ink);
}
.spell-fix {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--on-accent);
  font-weight: 600;
}
.spell-fix:hover { background: var(--accent-bright); }
.spell-skip:hover { background: var(--hover); }
.spell-none { font-size: 10.5px; color: var(--ink-faint); font-style: italic; }

.spell-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 14px;
  padding-top: 10px;
  border-top: 1px solid var(--line);
  font-size: 10.5px;
  color: var(--ink-faint);
}

/* ---------- sheets ---------- */

.sheet-pane { flex: 1; overflow: auto; background: var(--surface); }

.sheet { border-collapse: collapse; font-size: 12px; width: 100%; table-layout: fixed; }

.sheet th {
  position: sticky;
  top: 0;
  background: var(--grid-header);
  border: 1px solid var(--grid-line);
  height: 34px;
  font-weight: 500;
  font-size: 11px;
  color: var(--ink-soft);
  padding: 2px 7px;
  text-align: left;
  white-space: nowrap;
}

.sheet td {
  border: 1px solid var(--grid-line);
  height: 22px;
  padding: 0 7px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sheet tr:hover td { background: var(--hover); }
.sheet tr.selected td { background: var(--selection); }
.sheet tr.over td { color: var(--warn); font-weight: 600; }
.sheet tr.add-row td { color: var(--ink-faint); font-style: italic; }

/* ---------- network diagram ---------- */

.network-pane { flex: 1; overflow: auto; background: var(--bg); padding: 22px; }

.node {
  position: absolute;
  width: 168px;
  border: 1px solid var(--bar-edge);
  border-left: 3px solid var(--bar-edge);
  border-radius: 3px;
  background: var(--surface);
  font-size: 10px;
  padding: 4px 6px;
  line-height: 1.4;
}

.node.critical { border-color: var(--bar-critical-edge); border-left-color: var(--bar-critical-edge); }
.node .n-name { font-weight: 600; color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.node .n-row { display: flex; justify-content: space-between; color: var(--ink-soft); }

/* ---------- calendar view ---------- */

.calendar-pane { flex: 1; overflow: auto; background: var(--surface); padding: 12px; }

.cal-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  border-left: 1px solid var(--grid-line);
  border-top: 1px solid var(--grid-line);
}

.cal-dow {
  background: var(--grid-header);
  border-right: 1px solid var(--grid-line);
  border-bottom: 1px solid var(--grid-line);
  padding: 5px 6px;
  font-size: 11px;
  color: var(--ink-soft);
  text-align: center;
}

.cal-cell {
  min-height: 86px;
  border-right: 1px solid var(--grid-line);
  border-bottom: 1px solid var(--grid-line);
  padding: 3px;
  font-size: 10px;
  overflow: hidden;
}

.cal-cell.nonworking { background: var(--nonworking); }
.cal-cell .d { font-size: 11px; color: var(--ink-faint); text-align: right; }

.cal-chip {
  background: var(--bar);
  color: var(--ink);
  border-radius: 2px;
  padding: 1px 4px;
  margin-bottom: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cal-chip.critical { background: var(--bar-critical); }
.cal-chip.summary { background: #3a4747; }

/* ---------- status bar ---------- */

.statusbar {
  height: 24px;
  background: var(--surface-2);
  border-top: 1px solid var(--line);
  color: var(--ink-soft);
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 0 10px;
  font-size: 11px;
  flex: none;
}

.statusbar .grow { flex: 1; }
.statusbar .chip { white-space: nowrap; }
.statusbar .chip b { color: var(--ink); font-weight: 500; }
.statusbar .warn { color: var(--warn); font-weight: 600; }

.zoom-slider {
  display: flex;
  align-items: stretch;
  height: 19px;
  border: 1px solid var(--line);
  border-radius: 4px;
  overflow: hidden;
  background: rgba(216, 231, 232, 0.04);
}

.zoom-btn {
  width: 24px;
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  line-height: 1;
  padding: 0 0 1px;
}

.zoom-btn:hover { background: var(--hover); color: var(--accent-bright); }

.zoom-label {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 62px;
  padding: 0 6px;
  font-size: 11px;
  color: var(--ink);
  border-left: 1px solid var(--line);
  border-right: 1px solid var(--line);
}

/* ---------- context menu ---------- */

/* Office puts a floating mini toolbar above its context menu. */
/* The toolbar and the menu, anchored as one so the toolbar is above the menu
   whichever way the pair opened. */
.ctx-stack {
  position: fixed;
  z-index: 81;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  max-height: 80vh;
}
.ctx-minibar-wrap { flex: none; }

.ctx-minibar {
  display: flex;
  align-items: center;
  gap: 1px;
  padding: 3px;
  background: var(--surface-4);
  border: 1px solid var(--line);
  border-radius: 6px;
  box-shadow: var(--shadow);
}

.minibtn {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  height: 26px;
  min-width: 28px;
  padding: 0 5px;
  border: 1px solid transparent;
  border-radius: 4px;
  background: transparent;
  color: var(--ink);
  cursor: default;
}

.minibtn svg { color: var(--accent); }
.minibtn:hover:not(.disabled) { background: var(--accent-dim); border-color: var(--accent-line); }
.minibtn:active:not(.disabled) { background: var(--pressed); }
.minibtn.disabled { opacity: 0.32; }
.minibtn.disabled svg { color: var(--ink-faint); }

.minisep { width: 1px; height: 18px; margin: 0 3px; background: var(--line); }

.ctx-scrim { position: fixed; inset: 0; z-index: 80; }

/* A panel that places itself, for the pickers, which have no stack to be
   placed by. Without it a panel lays out in the flow beneath a window that is
   already the full height, so it is never seen, while the scrim beside it
   still covers everything and swallows the clicks meant for the cell. */
.ctx-anchored { position: fixed; z-index: 81; }

.ctxmenu {
  /* Sits inside the stack, so it is the menu that scrolls when the pair is
     taller than the room available, never the toolbar above it. */
  min-height: 0;
  overflow-y: auto;
  min-width: 226px;
  background: var(--surface-4);
  border: 1px solid var(--line);
  border-radius: 5px;
  box-shadow: var(--shadow);
  padding: 4px;
}

.ctxitem {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 5px 9px;
  border: 0;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  font-size: 12px;
  text-align: left;
  cursor: default;
}

.ctxitem:hover:not(.disabled) { background: var(--accent-dim); color: var(--accent-bright); }
.ctxitem.disabled { opacity: 0.34; }
.ctxitem .glyph { width: 16px; display: grid; place-items: center; color: var(--accent); flex: none; }
.ctxitem .label { flex: 1; }
.ctxitem .shortcut { color: var(--ink-faint); font-size: 10.5px; }
.ctxitem .tick { width: 12px; color: var(--accent); }

/* A flagged issue inside the context menu: what is wrong, then what can be
   done about it. Wider than a menu row because it carries a sentence, not a
   command name. */
.ctx-issue {
  max-width: 340px;
  padding: 8px 10px;
  margin: 2px 0;
  border-radius: 5px;
  background: var(--accent-dim);
  border: 1px solid var(--accent-line);
}
.ctx-issue-text {
  display: block;
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--ink);
}
.ctx-issue-acts { display: flex; gap: 6px; margin-top: 7px; }
.ctx-issue-fix, .ctx-issue-ignore {
  border-radius: 4px;
  padding: 3px 10px;
  font-size: 11px;
  cursor: default;
  border: 1px solid var(--line);
  background: var(--surface-3);
  color: var(--ink);
}
.ctx-issue-fix { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; }
.ctx-issue-fix:hover { background: var(--accent-bright); }
.ctx-issue-ignore:hover { background: var(--hover); }

/* A warning the planner has said they know about. Still there, so the row does
   not silently stop mentioning it, but no longer competing for attention. */
.ind-critical.ignored { color: var(--ink-faint); opacity: 0.55; }

/* An issue in the menu that has been dismissed reads the same way. */
.ctx-issue.ignored { background: transparent; border-color: var(--line); }
.ctx-issue.ignored .ctx-issue-text { color: var(--ink-faint); }

.ctxsep { height: 1px; background: var(--line); margin: 4px 6px; }

.ctxheader {
  padding: 5px 9px 6px;
  font-size: 10.5px;
  color: var(--ink-faint);
  letter-spacing: 0.4px;
  text-transform: uppercase;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ---------- dialogs ---------- */

.scrim {
  position: fixed;
  inset: 0;
  background: rgba(4, 8, 8, 0.62);
  z-index: 70;
  display: grid;
  place-items: center;
}

.dlg {
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 7px;
  box-shadow: var(--shadow);
  min-width: 470px;
  max-width: 860px;
  max-height: 86vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.dlg-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
  font-weight: 600;
  color: var(--ink);
}

.dlg-close {
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  width: 24px;
  height: 24px;
  border-radius: 3px;
}

.dlg-close:hover { background: var(--danger); color: #fff; }

.dlg-body { padding: 16px; overflow-y: auto; }

.dlg-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 11px 14px;
  border-top: 1px solid var(--line);
  background: var(--surface-2);
}

.dlg-tabs {
  display: flex;
  gap: 3px;
  padding: 9px 14px 0;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
}

.dlg-tab {
  border: 1px solid transparent;
  border-bottom: 0;
  background: transparent;
  color: var(--ink-soft);
  padding: 5px 14px;
  border-radius: 4px 4px 0 0;
  cursor: default;
  font-size: 11px;
}

.dlg-tab:hover { color: var(--ink); background: var(--hover); }
.dlg-tab.active { background: var(--surface); border-color: var(--line); color: var(--accent-bright); position: relative; top: 1px; }

.form-row { display: flex; align-items: center; gap: 10px; margin-bottom: 11px; }
.form-row label { width: 132px; flex: none; color: var(--ink-soft); }
.form-row .grow { flex: 1; }

.assign-table { width: 100%; border-collapse: collapse; font-size: 12px; }
.assign-table th, .assign-table td { border: 1px solid var(--grid-line); padding: 4px 7px; text-align: left; }
.assign-table th { background: var(--grid-header); color: var(--ink-soft); font-weight: 500; }
.assign-table tr.on td { background: var(--selection); }
.assign-table tr:hover td { background: var(--hover); }

.hint { color: var(--ink-soft); font-size: 11px; margin-top: 10px; line-height: 1.5; }

/* Trailing unit beside a rate box, so the number does not have to say it. */
.unit { color: var(--ink-soft); font-size: 11px; }
.dlg-sub { font-size: 12px; font-weight: 600; color: var(--ink); margin: 0 0 8px; }
.sep { height: 1px; background: var(--grid-line); margin: 14px 0; }
.dlg-list { max-height: 150px; overflow-y: auto; border: 1px solid var(--grid-line); border-radius: 3px; margin-top: 8px; }
.dlg-list-row { display: flex; align-items: center; gap: 10px; padding: 4px 9px; font-size: 12px; }
.dlg-list-row:nth-child(even) { background: var(--hover); }
.dlg-list-row .grow { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.error-banner {
  background: var(--danger-bg);
  border-bottom: 1px solid var(--danger);
  color: var(--danger);
  padding: 6px 12px;
  font-size: 11px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
}

.empty-state {
  display: grid;
  place-items: center;
  height: 100%;
  color: var(--ink-faint);
  font-size: 13px;
  text-align: center;
  line-height: 1.7;
  white-space: pre-line;
}

/* ---------- history and sync ---------- */

.sync-view {
  flex: 1 1 auto;
  overflow: auto;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  background: var(--surface);
}

/* The same shell the spelling panel uses, a little wider: these rows are
   sentences about what a server said, not single words. */
.sync-side { width: 560px; }
.sync-side-count { margin-left: auto; margin-right: 10px; color: var(--ink-faint); font-size: 11px; }

.sync-panel {
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface-3);
  padding: 12px 14px 14px;
}

/* A label and an answer, rather than a table: the answers are sentences of
   very different lengths and a column would set itself by the longest. */
.sync-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  padding: 5px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}
.sync-row:last-of-type { border-bottom: none; }
.sync-key { width: 168px; flex: none; color: var(--ink-soft); }
.sync-value { flex: 1; color: var(--ink); line-height: 1.55; }
.sync-value.mono { font-family: var(--mono); font-size: 11px; color: var(--accent); }
.sync-value.good { color: var(--accent-bright); }
.sync-value.warn { color: var(--warn); }
.sync-value.bad { color: var(--danger); }

.sync-actions { display: flex; align-items: center; gap: 10px; margin-top: 12px; flex-wrap: wrap; }
/* Why the button beside it is disabled. Beside rather than in a tooltip: a
   greyed control nobody can hover is a control nobody can ask about. */
.sync-why { color: var(--ink-soft); font-size: 11px; line-height: 1.5; }

.ver-row { cursor: default; }
.ver-row.selected td { background: var(--selection); }
.ver-why { color: var(--ink); }
.ver-note { color: var(--ink-faint); font-size: 11px; }

.ver-diff {
  margin-top: 12px;
  padding: 10px 12px;
  border: 1px solid var(--accent-line);
  border-radius: 6px;
  background: var(--accent-dim);
  font-size: 12px;
  line-height: 1.55;
}
.ver-diff-head {
  display: flex;
  align-items: center;
  gap: 7px;
  color: var(--accent-bright);
  margin-bottom: 6px;
}
.ver-diff button { margin-top: 10px; }

/* ---------- the sync dialogs ---------- */

/* Their work, filed under what it is about. Enough to recognise the plan in,
   which is what the question needs; the sentence above says how big it is. */
.diff-list {
  margin-top: 12px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface-3);
  padding: 4px 0;
}
.diff-group { padding: 6px 12px; border-bottom: 1px solid var(--line-soft); }
.diff-group:last-child { border-bottom: none; }
.diff-subject { font-size: 12px; color: var(--ink); margin-bottom: 3px; }
.diff-line { font-size: 11.5px; color: var(--ink-soft); padding-left: 12px; line-height: 1.6; }

.sync-drift {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  margin-top: 12px;
  padding: 9px 11px;
  border: 1px solid var(--danger);
  border-radius: 6px;
  background: var(--danger-bg);
  color: var(--ink);
  font-size: 11.5px;
  line-height: 1.6;
}
.sync-drift > span:first-child { color: var(--danger); flex: none; }

/* ---------- the health check ---------- */

.health-list { margin-top: 12px; display: flex; flex-direction: column; gap: 2px; }
.health-row {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  padding: 8px 2px;
  border-bottom: 1px solid var(--line-soft);
}
.health-row:last-child { border-bottom: none; }
.health-badge {
  flex: none;
  width: 84px;
  text-align: center;
  padding: 2px 0;
  border-radius: 999px;
  border: 1px solid var(--line);
  font-size: 10.5px;
  letter-spacing: 0.3px;
}
.health-badge.good { color: var(--accent-bright); border-color: var(--accent-line); background: var(--accent-dim); }
.health-badge.warn { color: var(--warn); border-color: var(--warn); }
.health-badge.bad { color: var(--danger); border-color: var(--danger); background: var(--danger-bg); }
/* Not checked is its own thing rather than a pale pass: a question that was
   not asked has not been answered. */
.health-badge.idle { color: var(--ink-faint); }
.health-text { flex: 1; min-width: 0; }
.health-asked { font-size: 12px; color: var(--ink); }
.health-detail { font-size: 11.5px; color: var(--ink-soft); line-height: 1.6; margin-top: 2px; }

/* ---------- gated ribbon commands ---------- */

/* Why the buttons beside it are grey. In the group rather than in a tooltip,
   because a disabled button is one nobody can hover to find out. */
.rwhy {
  max-width: 176px;
  align-self: center;
  padding: 0 6px;
  font-size: 10px;
  line-height: 1.4;
  color: var(--ink-faint);
}

/* ---------- collaborate options ---------- */

.opt-actions { display: flex; align-items: center; gap: 10px; margin: 10px 0 4px; flex-wrap: wrap; }
.opt-why { color: var(--ink-soft); font-size: 11px; line-height: 1.5; flex: 1; min-width: 200px; }

/* A line that qualifies the control above it. Quieter than .opt-note, which
   is a box and pulls the eye: these are things to notice while reading past,
   not things to stop at. */
.opt-aside {
  color: var(--ink-soft);
  font-size: 11px;
  line-height: 1.55;
  margin: -4px 0 12px 226px;
  max-width: 520px;
}

/* ---------- the account card ---------- */

/* Who is signed in, as one object rather than a stack of sentences. The
   avatar, the name and the address are what somebody checks; the buttons sit
   at the far end so the card reads left to right as "this is you, and here is
   what you can do about it". */
.acct-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 14px 16px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-2);
  margin-bottom: 12px;
}

.acct-avatar {
  flex: none;
  width: 44px;
  height: 44px;
  border-radius: 999px;
  display: grid;
  place-items: center;
  overflow: hidden;
  background: var(--accent-dim);
  border: 1px solid var(--accent-line);
  color: var(--accent-bright);
}

/* Signed out there is nobody to draw, so the circle is plainly empty rather
   than a face-shaped gap waiting to be filled. */
.acct-avatar.nobody { background: var(--surface-3); border-color: var(--line); color: var(--ink-faint); }

.acct-face { width: 100%; height: 100%; object-fit: cover; display: block; }

/* Sized and spaced like a monogram, not like text that failed to load. */
.acct-initials {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.6px;
  line-height: 1;
}

.acct-who { flex: 1; min-width: 0; }
.acct-name {
  font-size: 13.5px;
  font-weight: 600;
  color: var(--ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.acct-email {
  font-size: 11.5px;
  color: var(--ink-soft);
  margin-top: 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.acct-actions { flex: none; display: flex; align-items: center; gap: 8px; }

/* Facts that are worth keeping and not worth reading twice. */
.acct-details { margin: 0 0 14px; }
.acct-details > summary {
  cursor: default;
  font-size: 11.5px;
  color: var(--ink-soft);
  padding: 3px 0;
  list-style: none;
}
.acct-details > summary::-webkit-details-marker { display: none; }
.acct-details > summary::before { content: "\25b8 "; color: var(--ink-faint); }
.acct-details[open] > summary::before { content: "\25be "; }
.acct-details > summary:hover { color: var(--ink); }
.acct-details p {
  margin: 6px 0 0 14px;
  font-size: 11px;
  line-height: 1.55;
  color: var(--ink-faint);
  max-width: 560px;
}

/* ---------- the licence, the notes and the ask ---------- */

/* Over the splash as well, and with no click-through: on a first run the
   licence is the whole window until it has been answered. */
.welcome-scrim {
  position: fixed;
  inset: 0;
  z-index: 300;
  background: rgba(4, 8, 8, 0.86);
  display: grid;
  place-items: center;
  padding: 32px;
}

.welcome {
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 11px;
  box-shadow: var(--shadow);
  width: min(760px, 100%);
  max-height: 88vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.welcome-head {
  display: flex;
  align-items: center;
  gap: 18px;
  padding: 22px 26px 18px;
  border-bottom: 1px solid var(--line);
  background: var(--surface-2);
}

.welcome-mark { flex: none; }
.welcome-heading { min-width: 0; }
.welcome-title { font-size: 17px; font-weight: 600; color: var(--ink); }
.welcome-sub { font-size: 12.5px; color: var(--ink-soft); margin-top: 4px; line-height: 1.5; }

.welcome-body { padding: 20px 26px; overflow-y: auto; }

.welcome-lead {
  margin: 0 0 16px;
  font-size: 12.5px;
  line-height: 1.65;
  color: var(--ink-soft);
}

.welcome-foot {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 26px;
  border-top: 1px solid var(--line);
  background: var(--surface-2);
}

.welcome-foot .grow { flex: 1; }
.welcome-note { font-size: 11px; color: var(--ink-faint); }

/* The licence verbatim, so its own line breaks are kept and it can be
   selected and copied like the text file it is. */
.licence-text {
  margin: 0;
  padding: 16px 18px;
  max-height: 46vh;
  overflow-y: auto;
  background: var(--surface-3);
  border: 1px solid var(--line);
  border-radius: 6px;
  font-family: var(--mono);
  font-size: 11px;
  line-height: 1.6;
  color: var(--ink-soft);
  white-space: pre-wrap;
  -webkit-user-select: text;
  user-select: text;
}

.notes { font-size: 12.5px; line-height: 1.65; }
.notes-head {
  margin: 18px 0 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--accent);
  letter-spacing: 0.3px;
}
.notes-head:first-child { margin-top: 0; }
.notes-text { margin: 0 0 10px; color: var(--ink-soft); }
.notes-bullet {
  display: flex;
  gap: 9px;
  margin-bottom: 7px;
  color: var(--ink-soft);
}
.notes-bullet.nested { padding-left: 20px; }
.notes-bullet strong { color: var(--ink); font-weight: 600; }
.notes-dot { flex: none; color: var(--accent); }

.give {
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 16px 18px;
  margin-bottom: 14px;
  background: var(--surface-3);
}

.give-head {
  display: flex;
  align-items: center;
  gap: 9px;
  font-size: 13px;
  font-weight: 600;
  color: var(--ink);
}

.give-note { margin: 7px 0 12px; font-size: 11.5px; color: var(--ink-soft); line-height: 1.55; }

.give-details { display: flex; flex-direction: column; gap: 2px; }

.give-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 5px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}

.give-row:last-child { border-bottom: none; }
.give-row .k { flex: none; width: 130px; color: var(--ink-soft); }
.give-row .v {
  flex: 1;
  font-family: var(--mono);
  color: var(--ink);
  -webkit-user-select: text;
  user-select: text;
}

/* The About page's row of things to open. */
.about-actions { display: flex; justify-content: center; gap: 10px; flex-wrap: wrap; }
.about-actions .about-attr-btn { margin: 24px 0 0; }

/* A new version, said quietly and out of the way. */
.chip.link {
  border: 0;
  background: transparent;
  color: var(--accent-bright);
  font: inherit;
  cursor: default;
  padding: 0;
}
.chip.link:hover { text-decoration: underline; }
"##;

/// The light palette, as rules that can stand alone or sit inside a query.
///
/// Emitted after the main stylesheet and nothing else, so it wins purely by
/// order and every rule written against the tokens follows it without knowing
/// a second theme exists. Only the tokens are restated; anything that reads a
/// raw colour rather than a token would have to be fixed where it does so, not
/// here.
fn light_rules() -> String {
    root_block(Palette::Light)
}

/// Which palette to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Follow whatever the desktop asks for, and change with it.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ORDER: [ThemeChoice; 3] =
        [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    pub fn from_label(label: &str) -> Self {
        Self::ORDER
            .into_iter()
            .find(|choice| choice.label() == label)
            .unwrap_or_default()
    }

    /// The stylesheet to emit after the main one.
    ///
    /// Following the desktop is a media query rather than something read over
    /// D-Bus and polled: the engine already knows the answer, and a query keeps
    /// up on its own when the desktop switches while the application is open.
    pub fn overlay(self) -> String {
        match self {
            ThemeChoice::Dark => String::new(),
            ThemeChoice::Light => light_rules(),
            ThemeChoice::System => {
                format!("@media (prefers-color-scheme: light) {{\n{}\n}}", light_rules())
            }
        }
    }

    /// Which palette to read a literal colour out of.
    ///
    /// System reads as dark, and that is a compromise worth naming rather than
    /// hiding. Following the desktop is a media query, and a media query is
    /// answered by the engine at paint time; nothing in this process is told
    /// what the desktop asked for, so there is no honest way to answer it here.
    /// Dark is the sheet's own `:root`, so it is what gets painted whenever the
    /// query is not met, which makes it the right guess rather than an
    /// arbitrary one. The cost is a desktop set to light with the theme left on
    /// System: the chrome lightens and the colours written into SVG attributes
    /// stay dark. Closing that gap means the window telling the application
    /// which scheme it was given, which is a different change in a different
    /// place; an explicit Light or Dark is exact today.
    pub fn palette(self) -> Palette {
        match self {
            ThemeChoice::Light => Palette::Light,
            ThemeChoice::Dark | ThemeChoice::System => Palette::Dark,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The palette exactly as the stylesheet carried it before any of this,
    /// captured from the file itself and never touched since.
    ///
    /// The whole argument for lifting the palette into Rust is that the
    /// generated sheet is the sheet that was there before, and an argument is
    /// worth no more than what checks it.
    const AS_IT_WAS: &str = include_str!("palette-as-it-was.css");

    /// The same CSS with every comment taken out, so the parser below only has
    /// to understand declarations.
    fn without_comments(css: &str) -> String {
        let mut kept = String::with_capacity(css.len());
        let mut rest = css;
        while let Some(opened) = rest.find("/*") {
            kept.push_str(&rest[..opened]);
            match rest[opened..].find("*/") {
                Some(closed) => rest = &rest[opened + closed + 2..],
                None => return kept,
            }
        }
        kept.push_str(rest);
        kept
    }

    /// Every `:root` block in some CSS, each as its declarations in the order
    /// they are written.
    ///
    /// Order and not just membership, because the sheet is a cascade: two
    /// declarations of the same property mean the later one, and a comparison
    /// that sorted them would call two different stylesheets the same.
    ///
    /// Crude on purpose, in the same way as `rule_for` below. Neither block has
    /// a nested brace or a semicolon inside a value, so a block runs to the
    /// next `}` and a declaration to the next `;`, and nothing here has to
    /// parse CSS.
    fn root_blocks(css: &str) -> Vec<Vec<(String, String)>> {
        css.split(":root {")
            .skip(1)
            .map(|block| {
                block
                    .split('}')
                    .next()
                    .unwrap_or_default()
                    .split(';')
                    .filter_map(|declaration| declaration.split_once(':'))
                    .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
                    .filter(|(name, _)| name.starts_with("--"))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_generated_dark_palette_says_what_the_sheet_used_to_say() {
        let was = without_comments(AS_IT_WAS);
        let was = root_blocks(&was);
        let now = root_blocks(&root_block(Palette::Dark));
        assert_eq!(now[0], was[0]);
    }

    #[test]
    fn the_generated_light_palette_says_what_the_overlay_used_to_say() {
        let was = without_comments(AS_IT_WAS);
        let was = root_blocks(&was);
        let now = root_blocks(&root_block(Palette::Light));
        assert_eq!(now[0], was[1]);
    }

    /// The light overlay restates only what it changes, and that is the whole
    /// mechanism: a property it never mentions keeps what the sheet gave it.
    /// A generator that helpfully emitted all forty three would still paint
    /// correctly, so nothing else here would notice it had stopped being an
    /// overlay.
    #[test]
    fn the_light_overlay_restates_only_the_tokens_it_changes() {
        let dark = root_blocks(&root_block(Palette::Dark)).remove(0);
        let light = root_blocks(&root_block(Palette::Light)).remove(0);
        assert!(light.len() < dark.len());
        for (name, _) in &light {
            let stated = PALETTE.iter().find(|token| token.name == name).unwrap();
            assert!(stated.light.is_some(), "{name} is restated without changing");
        }
    }

    #[test]
    fn the_sheet_is_the_palette_followed_by_the_rules_written_against_it() {
        assert!(CSS.starts_with(":root {"));
        assert!(CSS.contains("--grid-header: #171d1e;"));
        assert!(CSS.contains("* { box-sizing: border-box; }"));
    }

    #[test]
    fn a_token_that_names_another_resolves_to_the_colour_it_names() {
        // --focus is var(--accent) in both palettes, which is the only
        // indirection the palette actually has.
        assert_eq!(Palette::Dark.paint("--focus"), "#81b5b5");
        assert_eq!(Palette::Light.paint("--focus"), "#2f5f5e");
    }

    #[test]
    fn every_colour_in_the_palette_resolves_to_something_a_paint_attribute_can_use() {
        for token in &PALETTE {
            if token.kind != TokenKind::Colour {
                continue;
            }
            for palette in [Palette::Dark, Palette::Light] {
                let resolved = palette
                    .colour(token.name)
                    .unwrap_or_else(|| panic!("{} resolves to nothing", token.name));
                assert!(
                    !resolved.contains("var("),
                    "{} is still a name rather than a colour: {resolved}",
                    token.name
                );
            }
        }
    }

    #[test]
    fn the_light_palette_answers_with_light_values() {
        assert_eq!(Palette::Dark.paint("--line"), "#27302f");
        assert_eq!(Palette::Light.paint("--line"), "#d2dedd");
    }

    /// A token the light palette leaves alone still has to answer, and answer
    /// with the value the sheet gave it rather than with nothing.
    #[test]
    fn a_token_the_light_palette_leaves_alone_keeps_the_sheets_value() {
        assert_eq!(Palette::Light.paint("--on-bar"), "#f2f7f7");
        assert_eq!(Palette::Dark.paint("--on-bar"), "#f2f7f7");
    }

    #[test]
    fn a_font_stack_is_not_offered_as_a_paint() {
        // The reason the table records what a token holds instead of assuming
        // it is a colour. Handed to an SVG fill, this list of families is not a
        // paint, and the shape would come out black with nothing to say why.
        assert!(Palette::Dark.colour("--font").is_none());
        assert!(Palette::Dark.colour("--mono").is_none());
        assert!(Palette::Dark.colour("--shadow").is_none());
        assert!(Palette::Dark.value("--font").unwrap().contains("Inter"));
    }

    #[test]
    fn a_name_the_palette_does_not_have_is_not_a_colour() {
        assert!(Palette::Dark.colour("--nothing-of-the-sort").is_none());
        assert_eq!(Palette::Dark.paint("--nothing-of-the-sort"), "currentColor");
    }

    #[test]
    fn a_value_that_is_not_naming_a_token_is_handed_back_untouched() {
        assert_eq!(Palette::Dark.literal("#123456"), "#123456");
        assert_eq!(Palette::Dark.literal("none"), "none");
        assert_eq!(Palette::Dark.literal("transparent"), "transparent");
        assert_eq!(Palette::Dark.literal("var(--line)"), "#27302f");
        assert_eq!(Palette::Light.literal("var(--line)"), "#d2dedd");
    }

    #[test]
    fn following_the_desktop_reads_as_the_palette_the_sheet_itself_states() {
        assert_eq!(ThemeChoice::System.palette(), Palette::Dark);
        assert_eq!(ThemeChoice::Dark.palette(), Palette::Dark);
        assert_eq!(ThemeChoice::Light.palette(), Palette::Light);
    }

    /// Every token the drawing code asks for by name, gathered from the source
    /// itself so a name added later is covered without anyone updating a list.
    #[test]
    fn every_token_the_charts_paint_with_is_a_colour_in_the_palette() {
        // An unknown name falls back to currentColor rather than failing, which
        // is right at run time and useless as a warning. This is what catches
        // it, and it is the only thing that does.
        let mut wrong = Vec::new();
        for file in [
            include_str!("gantt.rs"),
            include_str!("cursors.rs"),
            include_str!("views.rs"),
        ] {
            for chunk in file.split(".paint(\"").skip(1) {
                if let Some(name) = chunk.split('"').next()
                    && Palette::Dark.colour(name).is_none()
                {
                    wrong.push(name.to_string());
                }
            }
        }
        assert!(wrong.is_empty(), "these are not colours in the palette: {wrong:?}");
    }

    #[test]
    fn following_the_desktop_is_the_default() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }

    #[test]
    fn dark_needs_no_overlay_because_it_is_what_the_sheet_already_says() {
        assert!(ThemeChoice::Dark.overlay().is_empty());
    }

    #[test]
    fn choosing_light_applies_it_whatever_the_desktop_says() {
        let overlay = ThemeChoice::Light.overlay();
        assert!(overlay.contains("--surface: #ffffff"));
        assert!(
            !overlay.contains("prefers-color-scheme"),
            "an explicit choice is not conditional on the desktop"
        );
    }

    #[test]
    fn following_the_desktop_only_lightens_when_it_asks() {
        let overlay = ThemeChoice::System.overlay();
        assert!(overlay.starts_with("@media (prefers-color-scheme: light)"));
        assert!(overlay.contains("--surface: #ffffff"));
    }

    #[test]
    fn every_choice_survives_a_round_trip_through_its_label() {
        for choice in ThemeChoice::ORDER {
            assert_eq!(ThemeChoice::from_label(choice.label()), choice);
        }
    }

    /// The rule for one class, from the opening brace to the closing one.
    ///
    /// Crude on purpose: the sheet is a string constant with no nesting, so a
    /// rule runs from its selector to the next `}` and nothing has to parse
    /// CSS to find it.
    fn rule_for(class: &str) -> Option<&'static str> {
        let opened = CSS.find(&format!(".{class} {{"))?;
        let rest = &CSS[opened..];
        let closed = rest.find('}')?;
        Some(&rest[..closed])
    }

    /// Whether any of a class list is positioned the way it needs to be.
    fn is_placed(classes: &str, how: &str) -> bool {
        classes
            .split_whitespace()
            .filter_map(rule_for)
            .any(|rule| rule.contains(how))
    }

    /// Every panel the code places by writing coordinates into its style has
    /// to be taken out of the flow by the sheet, or the coordinates mean
    /// nothing.
    ///
    /// This is not hypothetical. `.ctxmenu` used to position itself, and when
    /// that moved to the `.ctx-stack` a context menu shares with its mini
    /// toolbar, the pickers kept writing `left` and `top` onto a panel that no
    /// longer answered to them. The panel laid out in the flow underneath a
    /// window that is already the full height and was never seen again, while
    /// its scrim went on covering the window and swallowing the clicks meant
    /// for the cell underneath: both ways into a Predecessors or Resources
    /// cell dead, and nothing on screen to say why.
    #[test]
    fn a_panel_placed_by_hand_is_taken_out_of_the_flow() {
        for (classes, how) in [
            // Against the window: the predecessor and resource pickers, a
            // context menu with its mini toolbar, and the lists a dropdown
            // drops. None of these has a positioned ancestor to hang from.
            (crate::popups::ANCHORED_CLASS, "position: fixed"),
            ("ctx-stack", "position: fixed"),
            ("dd-list", "position: fixed"),
            // Against a pane instead, which is the whole point: a peer's
            // pointer is written in that pane's own scrolling coordinates, so
            // it has to be absolute inside it rather than fixed to the window.
            ("cursor", "position: absolute"),
            ("cursors", "position: absolute"),
            ("grid-pane", "position: relative"),
        ] {
            assert!(
                is_placed(classes, how),
                "\"{classes}\" is placed by hand, so the sheet has to give one \
                 of its classes {how}"
            );
        }
    }
}

