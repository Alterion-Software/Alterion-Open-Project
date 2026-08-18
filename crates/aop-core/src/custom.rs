//! Custom fields, the way Project does them.
//!
//! Project gives every plan a fixed set of spare fields, `Text1` to `Text30`,
//! `Number1` to `Number20` and so on, which a planner renames and puts to
//! whatever use they have. That is deliberately not "add a field with any name
//! you like": the slots are fixed, so a view, a filter or an export written
//! against `Text3` keeps working when the plan is passed to someone else, and
//! two plans can be compared field by field.
//!
//! What a slot can carry beyond a value:
//!
//! * a **name**, so `Text3` reads as "Department"
//! * a **lookup list**, so the column offers the departments that exist rather
//!   than accepting any typo
//! * a **rollup**, deciding what a summary row shows: the sum of its children,
//!   the maximum, the average
//! * **indicators**, showing a mark in place of the value when it meets some
//!   test, so a column of numbers reads at a glance
//!
//! Formulas are the one part of Project's version not here. They are an
//! expression language with its own function library, and half of one would be
//! worse than none.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The sorts of spare field a plan has, and how many of each.
///
/// The counts match Project's so that a plan moving between the two keeps its
/// fields in the same slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CustomKind {
    Text,
    Number,
    Flag,
    Date,
    Duration,
    Cost,
    Start,
    Finish,
}

impl CustomKind {
    pub const ALL: [CustomKind; 8] = [
        CustomKind::Text,
        CustomKind::Number,
        CustomKind::Flag,
        CustomKind::Date,
        CustomKind::Duration,
        CustomKind::Cost,
        CustomKind::Start,
        CustomKind::Finish,
    ];

    /// How many slots of this sort a plan has.
    pub fn count(self) -> u8 {
        match self {
            CustomKind::Text => 30,
            CustomKind::Number => 20,
            CustomKind::Flag => 20,
            CustomKind::Date
            | CustomKind::Duration
            | CustomKind::Cost
            | CustomKind::Start
            | CustomKind::Finish => 10,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CustomKind::Text => "Text",
            CustomKind::Number => "Number",
            CustomKind::Flag => "Flag",
            CustomKind::Date => "Date",
            CustomKind::Duration => "Duration",
            CustomKind::Cost => "Cost",
            CustomKind::Start => "Start",
            CustomKind::Finish => "Finish",
        }
    }

    /// Whether values of this sort can be added up.
    fn is_numeric(self) -> bool {
        matches!(
            self,
            CustomKind::Number | CustomKind::Cost | CustomKind::Duration
        )
    }

    /// Which rollups make sense. Offering Sum on a date would be offering
    /// nonsense.
    pub fn rollups(self) -> Vec<Rollup> {
        let mut out = vec![Rollup::None];
        match self {
            CustomKind::Flag => out.extend([Rollup::And, Rollup::Or, Rollup::Count]),
            CustomKind::Text => out.push(Rollup::Count),
            CustomKind::Date | CustomKind::Start | CustomKind::Finish => {
                out.extend([Rollup::Min, Rollup::Max, Rollup::Count])
            }
            _ => out.extend([Rollup::Sum, Rollup::Max, Rollup::Min, Rollup::Average, Rollup::Count]),
        }
        out
    }
}

/// Which slot: `Text3`, `Number1`, `Flag12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Slot {
    pub kind: CustomKind,
    /// One based, matching how the fields are named.
    pub number: u8,
}

impl Slot {
    pub fn new(kind: CustomKind, number: u8) -> Self {
        Slot { kind, number }
    }

    /// The name Project would give it, used whenever it has not been renamed.
    pub fn default_title(&self) -> String {
        format!("{}{}", self.kind.label(), self.number)
    }
}

/// What a summary row shows for a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rollup {
    /// Nothing: the summary row shows only what was typed into it.
    None,
    Sum,
    Max,
    Min,
    Average,
    /// True only when every child is true.
    And,
    /// True when any child is.
    Or,
    /// How many children have a value at all.
    Count,
}

impl Rollup {
    pub fn label(self) -> &'static str {
        match self {
            Rollup::None => "None",
            Rollup::Sum => "Sum",
            Rollup::Max => "Maximum",
            Rollup::Min => "Minimum",
            Rollup::Average => "Average",
            Rollup::And => "And",
            Rollup::Or => "Or",
            Rollup::Count => "Count",
        }
    }
}

/// How an indicator decides whether it applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Test {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    Contains,
    IsEmpty,
    IsNotEmpty,
}

impl Test {
    pub const ALL: [Test; 7] = [
        Test::Equals,
        Test::NotEquals,
        Test::GreaterThan,
        Test::LessThan,
        Test::Contains,
        Test::IsEmpty,
        Test::IsNotEmpty,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Test::Equals => "equals",
            Test::NotEquals => "does not equal",
            Test::GreaterThan => "is greater than",
            Test::LessThan => "is less than",
            Test::Contains => "contains",
            Test::IsEmpty => "is empty",
            Test::IsNotEmpty => "is not empty",
        }
    }

    /// Whether a value satisfies this test.
    ///
    /// Numbers are compared as numbers when both sides look like one, so that
    /// 9 does not sort above 10 the way it would as text.
    pub fn matches(self, value: &str, against: &str) -> bool {
        let value = value.trim();
        let against = against.trim();
        match self {
            Test::IsEmpty => value.is_empty(),
            Test::IsNotEmpty => !value.is_empty(),
            Test::Equals => value.eq_ignore_ascii_case(against),
            Test::NotEquals => !value.eq_ignore_ascii_case(against),
            Test::Contains => value.to_lowercase().contains(&against.to_lowercase()),
            Test::GreaterThan | Test::LessThan => {
                match (value.parse::<f64>(), against.parse::<f64>()) {
                    (Ok(a), Ok(b)) => {
                        if self == Test::GreaterThan {
                            a > b
                        } else {
                            a < b
                        }
                    }
                    // Not numbers, so fall back to how they sort as words.
                    _ => {
                        if self == Test::GreaterThan {
                            value > against
                        } else {
                            value < against
                        }
                    }
                }
            }
        }
    }
}

/// A mark shown in place of a value when it meets a test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Indicator {
    pub test: Test,
    /// What the test compares against. Unused by the empty tests.
    #[serde(default)]
    pub against: String,
    /// The mark itself.
    pub glyph: String,
    /// What it means, for anyone reading the column who did not set it up.
    #[serde(default)]
    pub meaning: String,
}

/// One allowed value in a lookup list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookupValue {
    pub value: String,
    #[serde(default)]
    pub description: String,
}

/// A spare field that has been put to use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomField {
    pub slot: Slot,
    /// What it has been renamed to. Empty means it is still `Text3`.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub lookup: Vec<LookupValue>,
    /// Whether anything outside the lookup list is refused.
    #[serde(default)]
    pub lookup_only: bool,
    /// The value a new task starts with.
    #[serde(default)]
    pub default: String,
    pub rollup: Rollup,
    #[serde(default)]
    pub indicators: Vec<Indicator>,
}

impl CustomField {
    pub fn new(slot: Slot) -> Self {
        CustomField {
            slot,
            title: String::new(),
            lookup: Vec::new(),
            lookup_only: false,
            default: String::new(),
            rollup: Rollup::None,
            indicators: Vec::new(),
        }
    }

    /// What to put at the top of the column.
    pub fn title(&self) -> String {
        if self.title.trim().is_empty() {
            self.slot.default_title()
        } else {
            self.title.clone()
        }
    }

    /// Whether the field has been given a purpose, and so is worth offering as
    /// a column. An untouched slot is noise in a column picker.
    pub fn is_in_use(&self) -> bool {
        !self.title.trim().is_empty()
            || !self.lookup.is_empty()
            || !self.indicators.is_empty()
            || self.rollup != Rollup::None
    }

    /// Whether a value is allowed.
    pub fn accepts(&self, value: &str) -> bool {
        if !self.lookup_only || self.lookup.is_empty() || value.trim().is_empty() {
            return true;
        }
        self.lookup
            .iter()
            .any(|entry| entry.value.eq_ignore_ascii_case(value.trim()))
    }

    /// The mark to show instead of the value, if any applies.
    ///
    /// The first matching indicator wins, so the order they are listed in is
    /// the order they are tested, and a catch-all belongs at the bottom.
    pub fn indicator_for(&self, value: &str) -> Option<&Indicator> {
        self.indicators
            .iter()
            .find(|indicator| indicator.test.matches(value, &indicator.against))
    }

    /// Summarise a set of child values.
    ///
    /// Returns nothing when the field does not roll up, or when no child had a
    /// value: an empty summary cell is honest, a zero is not.
    pub fn roll_up(&self, children: &[String]) -> Option<String> {
        let present: Vec<&String> = children
            .iter()
            .filter(|value| !value.trim().is_empty())
            .collect();

        match self.rollup {
            Rollup::None => None,
            Rollup::Count => Some(present.len().to_string()),
            Rollup::And | Rollup::Or => {
                if present.is_empty() {
                    return None;
                }
                let truthy = |value: &str| {
                    matches!(
                        value.trim().to_lowercase().as_str(),
                        "yes" | "true" | "1" | "on"
                    )
                };
                let result = if self.rollup == Rollup::And {
                    present.iter().all(|value| truthy(value))
                } else {
                    present.iter().any(|value| truthy(value))
                };
                Some(if result { "Yes" } else { "No" }.to_string())
            }
            Rollup::Sum | Rollup::Average | Rollup::Max | Rollup::Min => {
                if self.slot.kind.is_numeric() {
                    let numbers: Vec<f64> = present
                        .iter()
                        .filter_map(|value| value.trim().parse::<f64>().ok())
                        .collect();
                    if numbers.is_empty() {
                        return None;
                    }
                    let result = match self.rollup {
                        Rollup::Sum => numbers.iter().sum(),
                        Rollup::Average => numbers.iter().sum::<f64>() / numbers.len() as f64,
                        Rollup::Max => numbers.iter().copied().fold(f64::MIN, f64::max),
                        _ => numbers.iter().copied().fold(f64::MAX, f64::min),
                    };
                    // Whole numbers read better without a trailing zero.
                    Some(if (result.fract()).abs() < f64::EPSILON {
                        format!("{result:.0}")
                    } else {
                        format!("{result:.2}")
                    })
                } else {
                    // Dates and text sort rather than add.
                    let mut sorted: Vec<&String> = present.clone();
                    sorted.sort();
                    match self.rollup {
                        Rollup::Max => sorted.last().map(|value| value.to_string()),
                        Rollup::Min => sorted.first().map(|value| value.to_string()),
                        _ => None,
                    }
                }
            }
        }
    }
}

/// Every custom field a plan has set up, by slot.
pub type CustomFields = BTreeMap<Slot, CustomField>;

/// The values one task carries.
pub type CustomValues = BTreeMap<Slot, String>;

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: CustomKind, rollup: Rollup) -> CustomField {
        let mut field = CustomField::new(Slot::new(kind, 1));
        field.rollup = rollup;
        field
    }

    #[test]
    fn the_slot_counts_match_the_ones_a_plan_is_expected_to_have() {
        // A view or an export written against Text3 has to find Text3 in a plan
        // that has been passed between tools.
        assert_eq!(CustomKind::Text.count(), 30);
        assert_eq!(CustomKind::Number.count(), 20);
        assert_eq!(CustomKind::Flag.count(), 20);
        assert_eq!(CustomKind::Date.count(), 10);
        assert_eq!(CustomKind::Cost.count(), 10);
    }

    #[test]
    fn an_unnamed_slot_shows_its_slot_name() {
        assert_eq!(CustomField::new(Slot::new(CustomKind::Text, 3)).title(), "Text3");
    }

    #[test]
    fn a_renamed_slot_shows_the_name_it_was_given() {
        let mut field = CustomField::new(Slot::new(CustomKind::Text, 3));
        field.title = "Department".into();
        assert_eq!(field.title(), "Department");
    }

    #[test]
    fn an_untouched_slot_is_not_offered_as_a_column() {
        // Thirty unused Text fields in a column picker is thirty rows of noise.
        assert!(!CustomField::new(Slot::new(CustomKind::Text, 1)).is_in_use());
        let mut named = CustomField::new(Slot::new(CustomKind::Text, 1));
        named.title = "Department".into();
        assert!(named.is_in_use());
    }

    #[test]
    fn a_restricted_field_refuses_anything_off_its_list() {
        let mut field = CustomField::new(Slot::new(CustomKind::Text, 1));
        field.lookup = vec![
            LookupValue { value: "Delivery".into(), description: String::new() },
            LookupValue { value: "Finance".into(), description: String::new() },
        ];
        field.lookup_only = true;

        assert!(field.accepts("Delivery"));
        assert!(field.accepts("finance"), "matching is not case sensitive");
        assert!(!field.accepts("Legal"));
        assert!(field.accepts(""), "clearing a cell is always allowed");
    }

    #[test]
    fn an_unrestricted_field_takes_anything_even_with_a_list() {
        let mut field = CustomField::new(Slot::new(CustomKind::Text, 1));
        field.lookup = vec![LookupValue { value: "Delivery".into(), description: String::new() }];
        assert!(field.accepts("Legal"), "the list is a suggestion, not a rule");
    }

    #[test]
    fn numbers_roll_up_by_arithmetic() {
        let values = ["10".to_string(), "20".to_string(), "30".to_string()];
        assert_eq!(field(CustomKind::Number, Rollup::Sum).roll_up(&values).as_deref(), Some("60"));
        assert_eq!(field(CustomKind::Number, Rollup::Max).roll_up(&values).as_deref(), Some("30"));
        assert_eq!(field(CustomKind::Number, Rollup::Min).roll_up(&values).as_deref(), Some("10"));
        assert_eq!(field(CustomKind::Number, Rollup::Average).roll_up(&values).as_deref(), Some("20"));
    }

    #[test]
    fn an_empty_summary_stays_empty_rather_than_becoming_zero() {
        // A zero says the children summed to nothing, which is a different
        // claim from nobody having filled it in.
        let values = ["".to_string(), "  ".to_string()];
        assert_eq!(field(CustomKind::Number, Rollup::Sum).roll_up(&values), None);
    }

    #[test]
    fn a_field_that_does_not_roll_up_summarises_nothing() {
        let values = ["10".to_string(), "20".to_string()];
        assert_eq!(field(CustomKind::Number, Rollup::None).roll_up(&values), None);
    }

    #[test]
    fn flags_roll_up_by_logic_not_arithmetic() {
        let mixed = ["Yes".to_string(), "No".to_string()];
        let all = ["Yes".to_string(), "true".to_string()];
        assert_eq!(field(CustomKind::Flag, Rollup::And).roll_up(&mixed).as_deref(), Some("No"));
        assert_eq!(field(CustomKind::Flag, Rollup::Or).roll_up(&mixed).as_deref(), Some("Yes"));
        assert_eq!(field(CustomKind::Flag, Rollup::And).roll_up(&all).as_deref(), Some("Yes"));
    }

    #[test]
    fn dates_roll_up_by_ordering_rather_than_adding() {
        let values = ["2026-03-01".to_string(), "2026-01-05".to_string()];
        let field = field(CustomKind::Date, Rollup::Min);
        assert_eq!(field.roll_up(&values).as_deref(), Some("2026-01-05"));
    }

    #[test]
    fn counting_reports_only_the_children_that_have_a_value() {
        let values = ["a".to_string(), "".to_string(), "c".to_string()];
        assert_eq!(field(CustomKind::Text, Rollup::Count).roll_up(&values).as_deref(), Some("2"));
    }

    #[test]
    fn only_the_rollups_that_make_sense_are_offered() {
        // Summing a date is not a thing to offer someone.
        assert!(!CustomKind::Date.rollups().contains(&Rollup::Sum));
        assert!(CustomKind::Date.rollups().contains(&Rollup::Min));
        assert!(CustomKind::Flag.rollups().contains(&Rollup::And));
        assert!(!CustomKind::Flag.rollups().contains(&Rollup::Sum));
        assert!(CustomKind::Cost.rollups().contains(&Rollup::Sum));
    }

    #[test]
    fn numbers_are_compared_as_numbers_not_as_words() {
        // As text, "9" sorts above "10", which would make the indicator wrong
        // in exactly the range people care about.
        assert!(Test::GreaterThan.matches("10", "9"));
        assert!(!Test::LessThan.matches("10", "9"));
    }

    #[test]
    fn the_first_matching_indicator_wins() {
        // So the order they are listed in is the order they are tested, and a
        // catch-all belongs at the bottom.
        let mut field = CustomField::new(Slot::new(CustomKind::Number, 1));
        field.indicators = vec![
            Indicator {
                test: Test::GreaterThan,
                against: "80".into(),
                glyph: "red".into(),
                meaning: "over budget".into(),
            },
            Indicator {
                test: Test::IsNotEmpty,
                against: String::new(),
                glyph: "green".into(),
                meaning: "fine".into(),
            },
        ];

        assert_eq!(field.indicator_for("95").map(|i| i.glyph.as_str()), Some("red"));
        assert_eq!(field.indicator_for("20").map(|i| i.glyph.as_str()), Some("green"));
        assert!(field.indicator_for("").is_none());
    }

    #[test]
    fn contains_and_equals_ignore_case() {
        assert!(Test::Equals.matches("Delivery", "delivery"));
        assert!(Test::Contains.matches("Delivery Team", "team"));
        assert!(Test::NotEquals.matches("Delivery", "Finance"));
    }
}
