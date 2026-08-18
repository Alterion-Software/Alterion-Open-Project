//! Checking the words in a plan.
//!
//! No word list is shipped with the application. English word lists are
//! variously GPL or otherwise encumbered, and bundling one would put a licence
//! on this crate that its own does not want. Instead the dictionary is built
//! from whatever the machine already has and cached, which is a question of
//! reading files the user already owns rather than redistributing them.
//!
//! This module is the part that has no opinion about where words come from: it
//! takes a set of words and finds what is not in it. Finding the word lists is
//! the caller's job, because that is the part that differs per platform.

use std::collections::HashSet;

use crate::model::Project;

/// A set of known words.
#[derive(Debug, Clone, Default)]
pub struct Dictionary {
    words: HashSet<String>,
}

impl Dictionary {
    /// Build from anything that yields words. Case and surrounding punctuation
    /// are normalised here so that callers do not each have to remember to.
    pub fn from_words<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Dictionary {
            words: words
                .into_iter()
                .filter_map(|word| normalise(word.as_ref()))
                .collect(),
        }
    }

    pub fn add(&mut self, word: &str) {
        if let Some(word) = normalise(word) {
            self.words.insert(word);
        }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn contains(&self, word: &str) -> bool {
        normalise(word).is_some_and(|word| self.words.contains(&word))
    }

    /// Words close enough to be what was meant.
    ///
    /// Only within an edit or two, and only of a similar length, because a
    /// suggestion three edits away is not a suggestion, it is a different word.
    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let Some(target) = normalise(word) else {
            return Vec::new();
        };
        let allowed = if target.chars().count() <= 4 { 1 } else { 2 };

        let mut found: Vec<(usize, &String)> = self
            .words
            .iter()
            .filter(|candidate| {
                candidate.len().abs_diff(target.len()) <= allowed
                    && candidate.as_bytes().first() == target.as_bytes().first()
            })
            .filter_map(|candidate| {
                let distance = edit_distance(&target, candidate, allowed)?;
                (distance > 0).then_some((distance, candidate))
            })
            .collect();

        // Closest first; then the word of the most similar length, since a
        // misspelling is usually the same size as what was meant rather than
        // several letters shorter; then alphabetical so the list does not
        // reshuffle itself between runs.
        found.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| {
                    a.1.chars()
                        .count()
                        .abs_diff(target.chars().count())
                        .cmp(&b.1.chars().count().abs_diff(target.chars().count()))
                })
                .then_with(|| a.1.cmp(b.1))
        });
        found
            .into_iter()
            .take(limit)
            .map(|(_, word)| word.clone())
            .collect()
    }
}

/// Reduce a word to the form the dictionary holds, or nothing if it is not a
/// word at all.
///
/// Anything with a digit in it is left alone: `PO-4471`, `v2` and `3ds` are
/// identifiers, and flagging them would bury the real mistakes.
fn normalise(word: &str) -> Option<String> {
    let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
    if trimmed.len() < 2 || trimmed.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !trimmed.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-') {
        return None;
    }
    Some(trimmed.to_lowercase())
}

/// Edit distance counting a swapped pair of letters as one mistake, abandoned
/// once it exceeds `limit`.
///
/// Two letters typed in the wrong order is the most common typo there is, and
/// plain Levenshtein charges two edits for it. That is enough to push the word
/// actually meant below a crowd of unrelated words that happen to be two edits
/// away: `complaince` ranks `complain`, `complaint` and `complaisance` above
/// `compliance`. Counting the swap once puts the right word first.
///
/// The bound is what keeps this usable against a dictionary of a hundred
/// thousand words: most candidates are abandoned after a row or two.
fn edit_distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }

    // Three rows, because a transposition looks two back on both axes.
    let mut before_previous = vec![0usize; b.len() + 1];
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];

    for (i, &ac) in a.iter().enumerate() {
        current[0] = i + 1;
        let mut row_best = current[0];
        for (j, &bc) in b.iter().enumerate() {
            let cost = usize::from(ac != bc);
            let mut best = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);

            // The swap: this letter is the previous one of the other word and
            // vice versa.
            if i > 0 && j > 0 && ac == b[j - 1] && a[i - 1] == bc {
                best = best.min(before_previous[j - 1] + 1);
            }

            current[j + 1] = best;
            row_best = row_best.min(best);
        }
        if row_best > limit {
            return None;
        }
        std::mem::swap(&mut before_previous, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }

    let distance = previous[b.len()];
    (distance <= limit).then_some(distance)
}

/// Where in the plan a word was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    TaskName(usize),
    TaskNotes(usize),
    ResourceName(usize),
    ProjectName,
}

impl Place {
    pub fn label(self) -> &'static str {
        match self {
            Place::TaskName(_) => "Task name",
            Place::TaskNotes(_) => "Notes",
            Place::ResourceName(_) => "Resource name",
            Place::ProjectName => "Project name",
        }
    }

    /// The row this belongs to, for taking the user there.
    pub fn row(self) -> Option<usize> {
        match self {
            Place::TaskName(row) | Place::TaskNotes(row) => Some(row),
            _ => None,
        }
    }
}

/// A word the dictionary does not know.
#[derive(Debug, Clone, PartialEq)]
pub struct Misspelling {
    pub word: String,
    pub place: Place,
    /// The text it was found in, so the user can see it in context.
    pub context: String,
    pub suggestions: Vec<String>,
}

/// Split text into candidate words.
///
/// Splits on case changes as well as spaces, so `CamelCase` and run-together
/// names are checked rather than dismissed as one unknown word.
fn words_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-') {
        if chunk.is_empty() {
            continue;
        }
        let mut current = String::new();
        let mut previous_lower = false;
        for c in chunk.chars() {
            if c.is_uppercase() && previous_lower && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            previous_lower = c.is_lowercase();
            current.push(c);
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

/// Find every word in the plan the dictionary does not know.
///
/// A word is reported once per place it appears, and words already told to be
/// ignored are left out entirely.
pub fn check(project: &Project, dictionary: &Dictionary, ignored: &HashSet<String>) -> Vec<Misspelling> {
    let mut found = Vec::new();
    let mut seen: HashSet<(String, usize)> = HashSet::new();

    let mut scan = |text: &str, place: Place, found: &mut Vec<Misspelling>| {
        if text.trim().is_empty() {
            return;
        }
        for word in words_in(text) {
            let Some(key) = normalise(&word) else { continue };
            if dictionary.contains(&key) || ignored.contains(&key) {
                continue;
            }
            // The same typo in the same place is one problem, not several.
            let marker = (key.clone(), place.row().unwrap_or(usize::MAX));
            if !seen.insert(marker) {
                continue;
            }
            found.push(Misspelling {
                suggestions: dictionary.suggest(&key, 5),
                word,
                place,
                context: text.trim().to_string(),
            });
        }
    };

    scan(&project.name, Place::ProjectName, &mut found);
    for (index, task) in project.tasks.iter().enumerate() {
        scan(&task.name, Place::TaskName(index), &mut found);
        scan(&task.notes, Place::TaskNotes(index), &mut found);
    }
    for (index, resource) in project.resources.iter().enumerate() {
        scan(&resource.name, Place::ResourceName(index), &mut found);
    }

    found
}

/// Replace one word in a piece of text, leaving the rest alone.
pub fn replace_word(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.to_lowercase().find(&from.to_lowercase()) {
        let before = &rest[..at];
        let matched = &rest[at..at + from.len()];
        let after = &rest[at + from.len()..];

        // Only a whole word, so correcting "on" does not maul "iron".
        let starts_clean = before.chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let ends_clean = after.chars().next().is_none_or(|c| !c.is_alphanumeric());

        out.push_str(before);
        if starts_clean && ends_clean {
            out.push_str(&match_case(matched, to));
        } else {
            out.push_str(matched);
        }
        rest = after;
    }

    out.push_str(rest);
    out
}

/// Give the replacement the shape of what it replaces, so correcting a word
/// that opened a sentence does not lower-case it.
fn match_case(original: &str, replacement: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) && original.chars().count() > 1 {
        return replacement.to_uppercase();
    }
    if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut chars = replacement.chars();
        return match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
    }
    replacement.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;
    use chrono::NaiveDate;

    fn dictionary() -> Dictionary {
        Dictionary::from_words([
            "compliance", "research", "payment", "processing", "review",
            "deliver", "schedule", "the", "and", "receive", "separate", "plan",
            "ship", "upgrade", "to", "authentication", "needed",
        ])
    }

    fn plan(names: &[&str]) -> Project {
        let start = NaiveDate::from_ymd_opt(2026, 1, 5)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let mut project = Project::blank(start);
        project.name = "Plan".into();
        project.tasks.clear();
        for name in names {
            let id = project.allocate_task_id();
            project.tasks.push(Task::new(id, *name, 480));
        }
        project
    }

    #[test]
    fn a_known_word_is_not_flagged() {
        let found = check(&plan(&["Compliance research"]), &dictionary(), &HashSet::new());
        assert!(found.is_empty());
    }

    #[test]
    fn a_typo_is_flagged_with_the_word_that_was_meant() {
        let found = check(&plan(&["Complaince research"]), &dictionary(), &HashSet::new());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].word, "Complaince");
        assert!(
            found[0].suggestions.contains(&"compliance".to_string()),
            "got {:?}",
            found[0].suggestions
        );
    }

    #[test]
    fn anything_with_a_digit_in_it_is_left_alone() {
        // PO-4471, v2, 3ds are identifiers. Flagging them buries the real
        // mistakes under noise.
        let found = check(
            &plan(&["Ship PO-4471", "Upgrade to v2", "Authentication 3ds"]),
            &dictionary(),
            &HashSet::new(),
        );
        let flagged: Vec<&str> = found.iter().map(|m| m.word.as_str()).collect();
        assert!(!flagged.contains(&"PO-4471"));
        assert!(!flagged.contains(&"v2"));
        assert!(!flagged.contains(&"3ds"));
    }

    #[test]
    fn run_together_words_are_checked_separately() {
        // Otherwise a whole CamelCase name reads as one unknown word and its
        // real typo goes unreported.
        assert_eq!(
            words_in("PaymentProcessing"),
            vec!["Payment".to_string(), "Processing".to_string()]
        );
        let found = check(&plan(&["PaymentProcessing"]), &dictionary(), &HashSet::new());
        assert!(found.is_empty(), "both halves are known words");
    }

    #[test]
    fn an_ignored_word_stays_ignored() {
        let ignored: HashSet<String> = ["kyc".to_string()].into_iter().collect();
        let found = check(&plan(&["KYC review"]), &dictionary(), &ignored);
        assert!(found.is_empty());
    }

    #[test]
    fn the_same_typo_in_one_place_is_reported_once() {
        let found = check(
            &plan(&["Complaince and complaince"]),
            &dictionary(),
            &HashSet::new(),
        );
        assert_eq!(found.len(), 1, "it is one problem, not two");
    }

    #[test]
    fn a_word_is_reported_separately_in_each_place_it_appears() {
        let found = check(
            &plan(&["Complaince research", "Complaince review"]),
            &dictionary(),
            &HashSet::new(),
        );
        assert_eq!(found.len(), 2, "each row is its own correction");
    }

    #[test]
    fn suggestions_are_close_by_and_ordered_closest_first() {
        let dict = dictionary();
        let suggestions = dict.suggest("recieve", 5);
        assert_eq!(suggestions.first().map(String::as_str), Some("receive"));
    }

    #[test]
    fn a_word_nothing_resembles_gets_no_suggestions_rather_than_bad_ones() {
        // A suggestion three edits away is a different word, not a correction.
        assert!(dictionary().suggest("zzzzqqxv", 5).is_empty());
    }

    #[test]
    fn replacing_a_word_leaves_the_rest_of_the_text_alone() {
        assert_eq!(
            replace_word("Complaince research and complaince", "complaince", "compliance"),
            "Compliance research and compliance"
        );
    }

    #[test]
    fn replacing_only_touches_whole_words() {
        // Correcting "on" must not maul "iron".
        assert_eq!(replace_word("iron on the ore", "on", "in"), "iron in the ore");
    }

    #[test]
    fn a_correction_keeps_the_shape_of_what_it_replaces() {
        assert_eq!(replace_word("Complaince", "complaince", "compliance"), "Compliance");
        assert_eq!(replace_word("COMPLAINCE", "complaince", "compliance"), "COMPLIANCE");
    }

    #[test]
    fn the_edit_distance_gives_up_once_it_is_too_far() {
        assert_eq!(edit_distance("cat", "cat", 2), Some(0));
        assert_eq!(edit_distance("cat", "cot", 2), Some(1));
        assert_eq!(edit_distance("cat", "dog", 2), None);
    }

    #[test]
    fn two_letters_the_wrong_way_round_counts_as_one_mistake() {
        // The commonest typo there is. Charged as two, the word actually meant
        // sinks below every unrelated word that happens to be two edits away.
        assert_eq!(edit_distance("complaince", "compliance", 2), Some(1));
        assert_eq!(edit_distance("teh", "the", 2), Some(1));
        assert_eq!(edit_distance("recieve", "receive", 2), Some(1));
    }

    #[test]
    fn a_transposed_word_is_suggested_ahead_of_unrelated_near_misses() {
        let dict = Dictionary::from_words([
            "compliance", "complain", "complaint", "complaints", "complaisance",
        ]);
        assert_eq!(
            dict.suggest("complaince", 3).first().map(String::as_str),
            Some("compliance")
        );
    }

    #[test]
    fn notes_and_resource_names_are_checked_too() {
        let mut project = plan(&["Review"]);
        project.tasks[0].notes = "Complaince needed".into();
        let found = check(&project, &dictionary(), &HashSet::new());
        assert!(found.iter().any(|m| matches!(m.place, Place::TaskNotes(0))));
    }
}
