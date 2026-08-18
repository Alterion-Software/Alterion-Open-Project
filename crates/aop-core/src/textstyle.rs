//! Text formatting, at the two levels Project offers it.
//!
//! **Text Styles** format a *kind* of row: every critical task, every summary,
//! every milestone, the row and column headings. Set the critical style once
//! and every task that goes critical picks it up, including the ones that go
//! critical next week. That is the level worth having, because a plan is
//! re-scheduled constantly and hand-formatting cannot keep up with it.
//!
//! The **Font group** formats the rows that happen to be selected, and nothing
//! else. It is the escape hatch: one row that needs to stand out for a reason
//! no category describes. Following Project, direct formatting of a row beats
//! whatever its category says, so the escape hatch actually works.
//!
//! Everything here treats an empty string and a zero size as "not set, use the
//! theme's", never as black and never as 0pt. That is the whole reason a
//! formatted plan still reads in both a dark and a light palette: a planner who
//! only ever wanted critical rows in bold has not, as a side effect, pinned
//! every other row to whatever colour the palette happened to be on the day
//! they clicked. `to_css` emits a declaration only for a property that was
//! actually chosen, so the stylesheet keeps the rest and nothing has to fight
//! anything.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Project, Task};

/// How a run of text is drawn. Every field has an "inherit" value, and that
/// value is the `Default`, so a fresh style changes nothing at all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    /// A CSS font stack. Empty means the theme's.
    #[serde(default)]
    pub family: String,
    /// Zero means the theme's.
    #[serde(default)]
    pub size_pt: f32,
    /// Ink. Empty means the theme's. A palette token such as `var(--ink-soft)`
    /// is as welcome as a literal colour, and travels between palettes better.
    #[serde(default)]
    pub colour: String,
    /// Fill behind the row. Empty means the theme's.
    #[serde(default)]
    pub background: String,
}

impl TextStyle {
    /// Whether the style asks for nothing, and so is not worth storing.
    pub fn is_unset(&self) -> bool {
        *self == TextStyle::default()
    }

    /// Lay `over` on top of this style, taking from it only what it sets.
    ///
    /// The three flags accumulate rather than replace. A plain `bool` cannot
    /// tell "leave it alone" apart from "switch it off", and of the two
    /// readings only accumulation is safe: if `false` meant "off", a style set
    /// up purely to recolour critical rows would silently strip the bold that
    /// the All style had asked for on every row in the plan.
    pub fn layered(&self, over: &TextStyle) -> TextStyle {
        TextStyle {
            bold: self.bold || over.bold,
            italic: self.italic || over.italic,
            underline: self.underline || over.underline,
            family: pick(&self.family, &over.family),
            // Guarding on `> 0.0` rather than `!= 0.0` also rejects a negative
            // or a NaN that got in through a hand-edited file.
            size_pt: if over.size_pt > 0.0 {
                over.size_pt
            } else {
                self.size_pt
            },
            colour: pick(&self.colour, &over.colour),
            background: pick(&self.background, &over.background),
        }
    }

    /// The inline style for a row, holding declarations for the chosen
    /// properties and for nothing else.
    pub fn to_css(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.bold {
            parts.push("font-weight: 700;".into());
        }
        if self.italic {
            parts.push("font-style: italic;".into());
        }
        if self.underline {
            parts.push("text-decoration: underline;".into());
        }
        if let Some(family) = usable(&self.family) {
            parts.push(format!("font-family: {family};"));
        }
        if self.size_pt > 0.0 {
            parts.push(format!("font-size: {}pt;", number(self.size_pt)));
        }
        if let Some(colour) = usable(&self.colour) {
            parts.push(format!("color: {colour};"));
        }
        if let Some(background) = usable(&self.background) {
            parts.push(format!("background: {background};"));
        }
        parts.join(" ")
    }
}

/// Which kind of row a style applies to, matching Project's Item to Change
/// list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StyleTarget {
    /// Every row, and the headings too. The floor the rest lay over.
    All,
    /// Noncritical tasks, in Project's wording.
    Normal,
    Critical,
    Summary,
    Milestone,
    /// The gutter down the left of the table.
    RowHeader,
    /// The titles along the top of the table.
    ColumnHeader,
}

/// The row categories, least specific first.
///
/// A row is usually several of these at once, so the ladder decides what a
/// critical milestone inside a summary ends up looking like. It is ordered by
/// how much each one narrows the row down:
///
/// * `All` describes nothing in particular, so it is the floor.
/// * `Normal` and `Critical` split the plan in two, so between them they
///   describe every row and neither is a distinction the planner drew.
/// * `Summary` and `Milestone` describe what the row *is*: shapes the planner
///   built by hand, a small minority of the plan, and stable. Criticality is a
///   scheduling result that flips whenever a date moves, so letting it repaint
///   phase headings would mean the outline changed appearance for reasons that
///   have nothing to do with the outline.
///
/// `Summary` and `Milestone` never both match, since a summary spans its
/// children and so is never drawn as a marker however its own duration reads.
const ROW_PRECEDENCE: [StyleTarget; 5] = [
    StyleTarget::All,
    StyleTarget::Normal,
    StyleTarget::Critical,
    StyleTarget::Summary,
    StyleTarget::Milestone,
];

impl StyleTarget {
    pub const ALL: [StyleTarget; 7] = [
        StyleTarget::All,
        StyleTarget::Normal,
        StyleTarget::Critical,
        StyleTarget::Summary,
        StyleTarget::Milestone,
        StyleTarget::RowHeader,
        StyleTarget::ColumnHeader,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StyleTarget::All => "All",
            StyleTarget::Normal => "Noncritical Tasks",
            StyleTarget::Critical => "Critical Tasks",
            StyleTarget::Summary => "Summary Tasks",
            StyleTarget::Milestone => "Milestones",
            StyleTarget::RowHeader => "Row Headings",
            StyleTarget::ColumnHeader => "Column Headings",
        }
    }

    /// Whether this describes a task row rather than a piece of table chrome.
    pub fn is_row_category(self) -> bool {
        !matches!(self, StyleTarget::RowHeader | StyleTarget::ColumnHeader)
    }

    /// Whether one row answers to this target.
    fn matches(self, project: &Project, index: usize) -> bool {
        let Some(task) = project.tasks.get(index) else {
            return false;
        };
        match self {
            StyleTarget::All => true,
            StyleTarget::Normal => !task.scheduled.critical,
            StyleTarget::Critical => task.scheduled.critical,
            StyleTarget::Summary => project.is_summary(index),
            StyleTarget::Milestone => project.is_marker(index),
            // Chrome, not a task row, so it never joins the ladder.
            StyleTarget::RowHeader | StyleTarget::ColumnHeader => false,
        }
    }
}

/// What a plan has chosen for each kind of row.
///
/// Only the targets that were actually given a style are held, so a plan
/// nobody has formatted carries an empty map rather than seven blank entries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextStyles {
    #[serde(default)]
    by_target: BTreeMap<StyleTarget, TextStyle>,
}

impl TextStyles {
    pub fn new() -> Self {
        TextStyles::default()
    }

    /// Whether the plan has been formatted at all.
    pub fn is_empty(&self) -> bool {
        self.by_target.is_empty()
    }

    /// What was chosen for one target, before anything is laid over it.
    pub fn get(&self, target: StyleTarget) -> Option<&TextStyle> {
        self.by_target.get(&target)
    }

    /// Choose a style for a target. A style that asks for nothing clears the
    /// entry instead of storing it, which is what keeps a saved plan free of
    /// entries that were opened in the dialog and cancelled out of.
    pub fn set(&mut self, target: StyleTarget, style: TextStyle) {
        if style.is_unset() {
            self.by_target.remove(&target);
        } else {
            self.by_target.insert(target, style);
        }
    }

    pub fn clear(&mut self, target: StyleTarget) {
        self.by_target.remove(&target);
    }

    /// The style for one target on its own, over the All style. This is the
    /// route for the headings, which are not rows and so have no index.
    pub fn style_of(&self, target: StyleTarget) -> TextStyle {
        let base = self.style(StyleTarget::All);
        if target == StyleTarget::All {
            base
        } else {
            base.layered(&self.style(target))
        }
    }

    pub fn css_of(&self, target: StyleTarget) -> String {
        self.style_of(target).to_css()
    }

    /// Everything that applies to one row, resolved down to a single style.
    ///
    /// The categories are laid on in `ROW_PRECEDENCE` order so the most
    /// specific one wins, and the row's own direct formatting goes on last:
    /// Project treats formatting applied to a selection as an override of the
    /// category, and it would be no use as an escape hatch otherwise.
    pub fn style_for(&self, project: &Project, index: usize) -> TextStyle {
        let mut resolved = TextStyle::default();
        for target in ROW_PRECEDENCE {
            if target.matches(project, index) {
                resolved = resolved.layered(&self.style(target));
            }
        }
        match project.tasks.get(index) {
            Some(task) => resolved.layered(&row_style(task)),
            None => resolved,
        }
    }

    /// The inline style for a table row.
    pub fn css_for(&self, project: &Project, index: usize) -> String {
        self.style_for(project, index).to_css()
    }

    fn style(&self, target: StyleTarget) -> TextStyle {
        self.by_target.get(&target).cloned().unwrap_or_default()
    }
}

// ---- the Font group -----------------------------------------------------

/// What the Font group has been used to set on one row.
///
/// The plan has room on a task for its two colours, so those are the two the
/// buttons can hold. Weight and size chosen for a single row have nowhere to
/// live in the file yet and would be lost on save, which is worse than not
/// offering them.
pub fn row_style(task: &Task) -> TextStyle {
    TextStyle {
        colour: task.text_colour.clone(),
        background: task.fill_colour.clone(),
        bold: task.bold,
        italic: task.italic,
        underline: task.underline,
        family: task.font_family.clone(),
        size_pt: task.font_size_pt,
    }
}

/// Apply the Font group to one row, leaving alone whatever the style does not
/// set so that clicking only the fill button does not also wipe the ink.
pub fn paint_row(task: &mut Task, style: &TextStyle) {
    if !style.colour.trim().is_empty() {
        task.text_colour = style.colour.trim().to_string();
    }
    if !style.background.trim().is_empty() {
        task.fill_colour = style.background.trim().to_string();
    }
    // Emphasis accumulates for the same reason it does when layering: a bare
    // bool cannot tell "leave alone" apart from "switch off".
    task.bold |= style.bold;
    task.italic |= style.italic;
    task.underline |= style.underline;
    if !style.family.trim().is_empty() {
        task.font_family = style.family.trim().to_string();
    }
    if style.size_pt > 0.0 {
        task.font_size_pt = style.size_pt;
    }
}

/// Strip a row back to whatever its category says, undoing the Font group.
pub fn clear_row_style(task: &mut Task) {
    task.text_colour.clear();
    task.fill_colour.clear();
    task.bold = false;
    task.italic = false;
    task.underline = false;
    task.font_family.clear();
    task.font_size_pt = 0.0;
}

/// Which of the three emphasis marks a ribbon button is toggling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Bold,
    Italic,
    Underline,
}

/// Turn one emphasis mark on or off across a set of rows.
///
/// Separate from `paint_row` on purpose. Painting accumulates, because a style
/// that says nothing about bold must not strip it; a toggle has to be able to
/// switch the mark off, which means writing `false` deliberately.
///
/// Returns whether the mark ended up on, so the caller can say which way it went.
pub fn toggle_emphasis(project: &mut Project, rows: &[usize], mark: Emphasis) -> bool {
    let reads = |task: &Task| match mark {
        Emphasis::Bold => task.bold,
        Emphasis::Italic => task.italic,
        Emphasis::Underline => task.underline,
    };

    // Off only when every selected row already has it, so a mixed selection
    // goes all on rather than half off, which is what a planner expects.
    let turning_on = !rows
        .iter()
        .filter_map(|index| project.tasks.get(*index))
        .all(reads);

    for index in rows {
        if let Some(task) = project.tasks.get_mut(*index) {
            match mark {
                Emphasis::Bold => task.bold = turning_on,
                Emphasis::Italic => task.italic = turning_on,
                Emphasis::Underline => task.underline = turning_on,
            }
        }
    }
    turning_on
}

// ---- the format painter -------------------------------------------------

/// A style lifted from one row, to be brushed onto others.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Painter {
    pub style: TextStyle,
}

/// Lift the look of a row.
///
/// What is picked up is the *resolved* style, not the rule that produced it: a
/// planner clicking the painter on a red critical row is pointing at the red,
/// and expects the next row they brush to go red whether or not it is critical.
/// Copying the rule instead would make the painter do nothing visible on rows
/// the rule does not match.
pub fn pick_up(styles: &TextStyles, project: &Project, index: usize) -> Painter {
    Painter {
        style: styles.style_for(project, index),
    }
}

impl Painter {
    /// Brush the lifted style onto a row.
    pub fn brush(&self, project: &mut Project, index: usize) {
        if let Some(task) = project.tasks.get_mut(index) {
            paint_row(task, &self.style);
        }
    }
}

// ---- helpers ------------------------------------------------------------

/// The overriding value when it says something, otherwise the one underneath.
fn pick(base: &str, over: &str) -> String {
    if over.trim().is_empty() {
        base.trim().to_string()
    } else {
        over.trim().to_string()
    }
}

/// A value fit to be written into an inline `style` attribute.
///
/// Colours and font stacks come out of a saved plan, and a plan can arrive from
/// anywhere. A value carrying a semicolon, a quote or a brace could close the
/// declaration it sits in and open another, so anything outside the small set
/// of characters a colour or a font stack actually needs is refused whole
/// rather than escaped. A refused value then falls back to the theme, which is
/// exactly where an unset one lands, so the row still renders.
fn usable(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    // Parentheses stay in: `var(--ink-soft)` and `rgba(...)` need them, and a
    // palette token is the form worth encouraging.
    let safe = value
        .chars()
        .all(|c| c.is_alphanumeric() || " #,.%()-_".contains(c));
    safe.then_some(value)
}

/// A point size without a pointless trailing zero.
fn number(size: f32) -> String {
    if size.fract().abs() < f32::EPSILON {
        format!("{size:.0}")
    } else {
        format!("{size}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A phase heading with a plain task and a milestone under it.
    fn plan() -> Project {
        let mut project = Project::default();
        project.push_task("Phase", 0);
        project.push_task("Work", 480);
        project.push_task("Sign-off", 0);
        project.tasks[1].outline_level = 1;
        project.tasks[2].outline_level = 1;
        project
    }

    fn bold() -> TextStyle {
        TextStyle {
            bold: true,
            ..TextStyle::default()
        }
    }

    fn coloured(colour: &str) -> TextStyle {
        TextStyle {
            colour: colour.into(),
            ..TextStyle::default()
        }
    }

    #[test]
    fn an_unset_property_inherits_the_theme_rather_than_defaulting_to_black() {
        // The point of the whole module: asking for bold must not also pin the
        // colour, the size and the family to whatever the defaults happen to
        // be, or the plan stops working in the other palette.
        assert_eq!(bold().to_css(), "font-weight: 700;");
        assert_eq!(TextStyle::default().to_css(), "");
        assert_eq!(TextStyles::new().css_for(&plan(), 1), "");
    }

    #[test]
    fn css_carries_only_the_properties_that_were_chosen() {
        let style = TextStyle {
            italic: true,
            size_pt: 9.0,
            colour: "var(--ink-soft)".into(),
            ..TextStyle::default()
        };
        assert_eq!(
            style.to_css(),
            "font-style: italic; font-size: 9pt; color: var(--ink-soft);"
        );
        assert!(!style.to_css().contains("font-family"));
        assert!(!style.to_css().contains("background"));
    }

    #[test]
    fn a_zero_size_means_the_themes_size_and_not_zero_points() {
        let style = TextStyle {
            size_pt: 0.0,
            ..coloured("#d8e7e8")
        };
        assert!(!style.to_css().contains("font-size"));
        assert_eq!(style.to_css(), "color: #d8e7e8;");
    }

    #[test]
    fn a_critical_row_takes_the_critical_style_and_a_noncritical_row_does_not() {
        let mut project = plan();
        project.tasks[1].scheduled.critical = true;

        let mut styles = TextStyles::new();
        styles.set(StyleTarget::Critical, coloured("var(--danger)"));

        assert_eq!(styles.css_for(&project, 1), "color: var(--danger);");
        project.tasks[1].scheduled.critical = false;
        assert_eq!(styles.css_for(&project, 1), "");
    }

    #[test]
    fn a_critical_summary_is_styled_as_a_summary_because_structure_beats_schedule_state() {
        // Criticality flips whenever a date moves. Letting it repaint phase
        // headings would change the look of the outline for reasons that have
        // nothing to do with the outline.
        let mut project = plan();
        project.tasks[0].scheduled.critical = true;

        let mut styles = TextStyles::new();
        styles.set(StyleTarget::Critical, coloured("var(--danger)"));
        styles.set(StyleTarget::Summary, coloured("var(--accent)"));

        assert_eq!(styles.style_for(&project, 0).colour, "var(--accent)");
    }

    #[test]
    fn a_critical_milestone_is_styled_as_a_milestone() {
        let mut project = plan();
        project.tasks[2].scheduled.critical = true;

        let mut styles = TextStyles::new();
        styles.set(StyleTarget::Critical, coloured("var(--danger)"));
        styles.set(StyleTarget::Milestone, coloured("var(--accent-bright)"));

        assert_eq!(styles.style_for(&project, 2).colour, "var(--accent-bright)");
    }

    #[test]
    fn a_summary_never_answers_to_the_milestone_style_despite_having_no_duration() {
        // A summary spans its children, so its own zero duration says nothing
        // about how the row is drawn.
        let project = plan();
        assert_eq!(project.tasks[0].duration_minutes, 0);

        let mut styles = TextStyles::new();
        styles.set(StyleTarget::Milestone, coloured("var(--accent-bright)"));

        assert_eq!(styles.css_for(&project, 0), "");
        assert_eq!(styles.style_for(&project, 2).colour, "var(--accent-bright)");
    }

    #[test]
    fn emphasis_accumulates_down_the_ladder_because_a_bool_cannot_say_not_set() {
        // Recolouring critical rows must not strip the bold that All asked for
        // on every row in the plan.
        let mut project = plan();
        project.tasks[1].scheduled.critical = true;

        let mut styles = TextStyles::new();
        styles.set(StyleTarget::All, bold());
        styles.set(
            StyleTarget::Critical,
            TextStyle {
                italic: true,
                ..coloured("var(--danger)")
            },
        );

        let resolved = styles.style_for(&project, 1);
        assert!(resolved.bold, "the All style still applies underneath");
        assert!(resolved.italic);
        assert_eq!(resolved.colour, "var(--danger)");
    }

    #[test]
    fn direct_row_formatting_beats_the_category_it_belongs_to() {
        // Project's own rule, and the Font group is useless without it.
        let mut project = plan();
        project.tasks[1].text_colour = "#ffcc00".into();

        let mut styles = TextStyles::new();
        styles.set(StyleTarget::All, coloured("var(--ink)"));

        assert_eq!(styles.style_for(&project, 1).colour, "#ffcc00");
        assert_eq!(styles.style_for(&project, 0).colour, "var(--ink)");
    }

    #[test]
    fn the_headings_resolve_without_a_row_and_never_leak_onto_one() {
        // Chrome has no index, so it resolves through `css_of`, and it must
        // stay out of the ladder that resolves a task row.
        let mut styles = TextStyles::new();
        styles.set(StyleTarget::All, coloured("var(--ink)"));
        styles.set(StyleTarget::ColumnHeader, bold());
        styles.set(StyleTarget::RowHeader, coloured("var(--danger)"));

        assert_eq!(
            styles.css_of(StyleTarget::ColumnHeader),
            "font-weight: 700; color: var(--ink);"
        );
        assert!(!StyleTarget::RowHeader.is_row_category());
        assert_eq!(styles.css_for(&plan(), 1), "color: var(--ink);");
    }

    #[test]
    fn a_style_that_asks_for_nothing_is_dropped_so_an_untouched_plan_saves_nothing() {
        let mut styles = TextStyles::new();
        styles.set(StyleTarget::Critical, bold());
        assert!(!styles.is_empty());

        styles.set(StyleTarget::Critical, TextStyle::default());
        assert!(styles.is_empty(), "a cancelled dialog leaves no trace");
    }

    #[test]
    fn a_value_carrying_css_punctuation_falls_back_to_the_theme() {
        // A plan can arrive from anywhere, and this string goes straight into
        // an inline style attribute.
        let style = coloured("red; background: url(nope)");
        assert_eq!(style.to_css(), "");
        let token = coloured("rgba(129, 181, 181, 0.42)");
        assert_eq!(token.to_css(), "color: rgba(129, 181, 181, 0.42);");
    }

    #[test]
    fn styles_survive_a_round_trip_through_the_saved_plan() {
        let mut styles = TextStyles::new();
        styles.set(
            StyleTarget::Summary,
            TextStyle {
                bold: true,
                size_pt: 10.5,
                family: "Inter".into(),
                ..coloured("var(--accent)")
            },
        );

        let text = serde_json::to_string(&styles).unwrap();
        let back: TextStyles = serde_json::from_str(&text).unwrap();
        assert_eq!(back, styles);
        assert_eq!(back.style_of(StyleTarget::Summary).size_pt, 10.5);
    }

    #[test]
    fn the_format_painter_lifts_the_look_of_a_row_not_the_rule_behind_it() {
        // Clicking the painter on a red critical row is pointing at the red,
        // so the next row brushed goes red whether or not it is critical.
        let mut project = plan();
        project.tasks[1].scheduled.critical = true;

        let mut styles = TextStyles::new();
        styles.set(StyleTarget::Critical, coloured("var(--danger)"));

        let painter = pick_up(&styles, &project, 1);
        assert_eq!(painter.style.colour, "var(--danger)");

        painter.brush(&mut project, 0);
        assert_eq!(project.tasks[0].text_colour, "var(--danger)");
        assert_eq!(styles.style_for(&project, 0).colour, "var(--danger)");

        clear_row_style(&mut project.tasks[0]);
        assert_eq!(styles.css_for(&project, 0), "");
    }
}
