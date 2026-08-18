//! Finding the machine's own word lists, and keeping a copy of what we found.
//!
//! Nothing is shipped with the application. English word lists tend to be GPL
//! or otherwise encumbered, and bundling one would put a licence on the product
//! that it does not want. Reading the lists a user already has installed is a
//! different matter entirely, so the dictionary is assembled from those on
//! first use and cached in the application's own directory.
//!
//! That also means the dictionary is only as good as the machine it is built
//! on. A box with no word list installed gets an empty one, and the checker
//! says so rather than reporting every word in the plan as a mistake.

use std::collections::HashSet;
use std::path::PathBuf;

use aop_core::spelling::Dictionary;

/// Where the assembled copy is kept.
fn cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("alterion-open-project").join("dictionary.txt"))
}

/// Where the user's own additions live, kept apart from the assembled copy so
/// that rebuilding the latter never discards the former.
fn additions_path() -> Option<PathBuf> {
    cache_path().map(|path| path.with_file_name("dictionary-added.txt"))
}

/// Word lists this platform is likely to have.
///
/// Ordered so that a fuller list is preferred, but all of them are read: a
/// machine with several installed gets the union rather than whichever was
/// found first.
#[cfg(target_os = "linux")]
fn sources() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = [
        "/usr/share/dict/words",
        "/usr/share/dict/american-english",
        "/usr/share/dict/british-english",
        "/usr/share/dict/cracklib-small",
    ]
    .iter()
    .map(PathBuf::from)
    .filter(|path| path.exists())
    .collect();

    // Hunspell and myspell ship `.dic` files, which are a word list with
    // affix codes after a slash.
    for directory in ["/usr/share/hunspell", "/usr/share/myspell/dicts"] {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("dic") {
                found.push(path);
            }
        }
    }

    found
}

#[cfg(target_os = "macos")]
fn sources() -> Vec<PathBuf> {
    // macOS ships a word list at this path as standard.
    ["/usr/share/dict/words", "/usr/share/dict/web2"]
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

#[cfg(target_os = "windows")]
fn sources() -> Vec<PathBuf> {
    // Windows has no word list file; its spelling support is a COM API. Until
    // that is wired up, only the user's own additions are known, so the
    // checker reports that it has no dictionary rather than flagging
    // everything.
    Vec::new()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn sources() -> Vec<PathBuf> {
    ["/usr/share/dict/words"]
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

/// Pull the words out of one file.
///
/// Handles both plain lists and hunspell `.dic` files, whose entries carry
/// affix codes after a slash and whose first line is a count rather than a
/// word.
fn words_from(path: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let dic = path.extension().and_then(|e| e.to_str()) == Some("dic");

    text.lines()
        .skip(usize::from(dic))
        .map(|line| line.split('/').next().unwrap_or(line).trim())
        .filter(|word| {
            word.len() >= 2 && word.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-')
        })
        .map(|word| word.to_lowercase())
        .collect()
}

/// Build the dictionary from whatever this machine has, and cache it.
fn assemble() -> Vec<String> {
    let mut words: HashSet<String> = HashSet::new();
    // Whatever the machine already had, plus anything fetched on request.
    for source in sources().into_iter().chain(downloaded()) {
        words.extend(words_from(&source));
    }

    let mut words: Vec<String> = words.into_iter().collect();
    words.sort_unstable();

    if let Some(path) = cache_path()
        && let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        let _ = std::fs::write(&path, words.join("\n"));
    }

    words
}

/// The user's own additions.
pub fn additions() -> Vec<String> {
    additions_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.lines().map(|line| line.trim().to_lowercase()).collect())
        .unwrap_or_default()
}

/// Remember a word the user says is fine.
pub fn remember(word: &str) {
    let Some(path) = additions_path() else { return };
    let mut words = additions();
    let word = word.trim().to_lowercase();
    if word.is_empty() || words.contains(&word) {
        return;
    }
    words.push(word);
    words.sort_unstable();

    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let _ = std::fs::write(path, words.join("\n"));
}

/// Load the dictionary, building it from the machine's word lists the first
/// time and reading the cached copy after that.
pub fn load() -> Dictionary {
    let cached = cache_path().and_then(|path| std::fs::read_to_string(path).ok());
    let words: Vec<String> = match cached {
        Some(text) if !text.trim().is_empty() => {
            text.lines().map(|line| line.to_string()).collect()
        }
        _ => assemble(),
    };

    let mut dictionary = Dictionary::from_words(words);
    for word in additions() {
        dictionary.add(&word);
    }
    dictionary
}

/// Throw the cached copy away so the next load rebuilds it, for when a user
/// has just installed a word list.
pub fn rebuild() -> Dictionary {
    if let Some(path) = cache_path() {
        let _ = std::fs::remove_file(path);
    }
    load()
}

// ------------------------------------------------------------- downloading

/// A dictionary that can be fetched on request.
///
/// Pinned to one commit rather than a branch, so the bytes a checksum was taken
/// against are the bytes that arrive, however the upstream repository moves on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Available {
    pub code: &'static str,
    pub name: &'static str,
    /// Path within the dictionaries repository.
    path: &'static str,
    /// SHA-256 of the `.dic` file, so a truncated or substituted download is
    /// refused rather than quietly installed.
    sha256: &'static str,
    /// Roughly how big, for warning before a slow download.
    pub bytes: usize,
}

/// The commit the checksums below were taken against.
const PINNED: &str = "f2ff99058268502bdcf4cad25c1ca2935ad8aa7d";

/// Dictionaries offered for download, from the LibreOffice collection.
///
/// Nothing is shipped and nothing is fetched without being asked for: this is
/// the same arrangement as installing a dictionary through a package manager,
/// which is what keeps a word list's licence off this product.
pub const CATALOGUE: [Available; 6] = [
    Available {
        code: "en_GB",
        name: "English (United Kingdom)",
        path: "en/en_GB",
        sha256: "04e90f34f5263bf26780e9c4a442e9ad16584e227af49ddd1b3b21b01df5b29c",
        bytes: 1_230_571,
    },
    Available {
        code: "en_US",
        name: "English (United States)",
        path: "en/en_US",
        sha256: "f0b1a234bd178bdd01875b2a392a9647f888b8fe879f79c52aae62c2759b3647",
        bytes: 551_762,
    },
    Available {
        code: "en_ZA",
        name: "English (South Africa)",
        path: "en/en_ZA",
        sha256: "a438af6c6bfa9b25208d72c6ebf53507e178d6e6d285895e97870c36193433b1",
        bytes: 998_279,
    },
    Available {
        code: "en_AU",
        name: "English (Australia)",
        path: "en/en_AU",
        sha256: "aa07c46571f306b79fc1bc534357ed357af15687381b26f891ba66e8a2caed89",
        bytes: 554_336,
    },
    Available {
        code: "de_DE",
        name: "German (Germany)",
        path: "de/de_DE_frami",
        sha256: "4ca3c958b0e5545910999bc246f668840bf8ede3df8e5e6790d05edd5a586c38",
        bytes: 4_356_903,
    },
    Available {
        code: "es_ES",
        name: "Spanish (Spain)",
        path: "es/es_ES",
        sha256: "6975dddec3d5d2c676069537bc67b4b5f786c65c5d4cf6703a82acf779ac9ec1",
        bytes: 715_989,
    },
];

impl Available {
    fn url(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/LibreOffice/dictionaries/{PINNED}/{}.dic",
            self.path
        )
    }

    /// Where a fetched copy is kept.
    pub fn local_path(&self) -> Option<PathBuf> {
        cache_path().map(|path| path.with_file_name(format!("{}.dic", self.code)))
    }

    pub fn is_installed(&self) -> bool {
        self.local_path().is_some_and(|path| path.exists())
    }
}

/// Every dictionary already fetched.
fn downloaded() -> Vec<PathBuf> {
    CATALOGUE
        .iter()
        .filter_map(|entry| entry.local_path())
        .filter(|path| path.exists())
        .collect()
}

/// Fetch one dictionary and keep it.
///
/// The download is checked against its recorded digest before anything is
/// written. A word list is only text, but it becomes part of what the
/// application tells the user is correct, and silently accepting whatever
/// arrived would make that meaningless.
pub fn download(entry: &Available) -> Result<usize, String> {
    let Some(path) = entry.local_path() else {
        return Err("No configuration directory to keep it in.".into());
    };

    let body = ureq::get(entry.url())
        .call()
        .map_err(|error| format!("Could not reach the dictionary server: {error}"))?
        .body_mut()
        .read_to_vec()
        .map_err(|error| format!("The download did not finish: {error}"))?;

    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(&body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest != entry.sha256 {
        return Err(format!(
            "The download does not match its checksum, so it has not been kept. Expected {}, got {}.",
            &entry.sha256[..12],
            &digest[..12]
        ));
    }

    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return Err("Could not create the configuration directory.".into());
    }
    std::fs::write(&path, &body).map_err(|error| format!("Could not save it: {error}"))?;

    // The assembled copy is now out of date.
    if let Some(cache) = cache_path() {
        let _ = std::fs::remove_file(cache);
    }
    Ok(body.len())
}

/// Forget a downloaded dictionary.
pub fn remove(entry: &Available) {
    if let Some(path) = entry.local_path() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(cache) = cache_path() {
        let _ = std::fs::remove_file(cache);
    }
}

/// Which word lists were found, for telling the user why the dictionary is the
/// size it is.
pub fn describe_sources() -> Vec<String> {
    sources()
        .into_iter()
        .chain(downloaded())
        .map(|path| path.display().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_word_list_is_read_line_by_line() {
        let dir = std::env::temp_dir().join("aop-dict-plain");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("words");
        std::fs::write(&path, "alpha\nbeta\nx\n12345\ngamma\n").unwrap();

        let words = words_from(&path);
        assert!(words.contains(&"alpha".to_string()));
        assert!(words.contains(&"gamma".to_string()));
        assert!(!words.contains(&"x".to_string()), "too short to be useful");
        assert!(!words.contains(&"12345".to_string()), "not a word");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hunspell_file_drops_its_count_line_and_affix_codes() {
        // The first line is how many entries follow, and each entry may carry
        // affix flags after a slash. Both would otherwise become "words".
        let dir = std::env::temp_dir().join("aop-dict-hunspell");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("en_GB.dic");
        std::fs::write(&path, "3\ncompliance/SM\nreview/DGS\nschedule\n").unwrap();

        let words = words_from(&path);
        assert_eq!(words, vec!["compliance", "review", "schedule"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn words_are_lowercased_so_lookups_do_not_depend_on_the_source() {
        let dir = std::env::temp_dir().join("aop-dict-case");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("words");
        std::fs::write(&path, "Compliance\nREVIEW\n").unwrap();
        assert_eq!(words_from(&path), vec!["compliance", "review"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_yields_nothing_rather_than_failing() {
        assert!(words_from(std::path::Path::new("/nonexistent/words")).is_empty());
    }
}

#[cfg(test)]
mod download_tests {
    use super::*;

    #[test]
    fn every_offered_dictionary_has_a_full_checksum() {
        // A short or empty digest would make the verification pass by accident.
        for entry in CATALOGUE {
            assert_eq!(entry.sha256.len(), 64, "{} has no usable digest", entry.code);
            assert!(entry.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(entry.bytes > 0);
        }
    }

    #[test]
    fn the_url_is_pinned_to_a_commit_not_a_branch() {
        // Against a branch the bytes could change under the checksum, and every
        // download would start failing for reasons nobody could see.
        let url = CATALOGUE[0].url();
        assert!(url.contains(PINNED), "got {url}");
        assert!(!url.contains("/master/"), "a branch would move: {url}");
        assert!(url.starts_with("https://"), "not over plain http");
    }

    /// Actually fetches over the network, so it only runs when asked for:
    /// `cargo test -p aop-app -- --ignored fetches`.
    #[test]
    #[ignore = "reaches the network"]
    fn fetches_a_real_dictionary_and_verifies_it() {
        let entry = CATALOGUE.iter().find(|e| e.code == "en_ZA").unwrap();
        remove(entry);
        let bytes = download(entry).expect("download");
        assert_eq!(bytes, entry.bytes, "exactly what the catalogue promised");
        assert!(entry.is_installed());

        let dictionary = load();
        assert!(dictionary.len() > 50_000, "got {} words", dictionary.len());
        assert!(dictionary.contains("compliance"));
        assert!(!dictionary.contains("complaince"));
        let suggestions = dictionary.suggest("complaince", 8);
        eprintln!("suggestions for complaince: {suggestions:?}");
        assert!(
            suggestions.contains(&"compliance".to_string()),
            "got {suggestions:?}"
        );
    }

    #[test]
    #[ignore = "reaches the network"]
    fn a_download_that_does_not_match_its_checksum_is_refused() {
        // The whole point of the digest: a substituted file must not be kept.
        let mut tampered = *CATALOGUE.iter().find(|e| e.code == "en_US").unwrap();
        tampered.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        remove(&tampered);
        let outcome = download(&tampered);
        assert!(outcome.is_err(), "it should have been refused");
        assert!(!tampered.is_installed(), "and nothing written");
    }

    #[test]
    fn no_two_entries_share_a_code_or_a_file() {
        let mut codes: Vec<&str> = CATALOGUE.iter().map(|e| e.code).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), before, "a shared code would overwrite a download");
    }
}

#[cfg(test)]
mod addition_tests {
    use super::*;

    #[test]
    fn a_remembered_word_is_written_to_the_list_and_read_back() {
        // Remembering it only in memory would lose it the moment the
        // application closed, which is the opposite of "add to dictionary".
        let Some(path) = additions_path() else { return };
        let before = std::fs::read_to_string(&path).unwrap_or_default();

        let probe = "zzzprobeword";
        remember(probe);
        assert!(additions().contains(&probe.to_string()), "written to the list");
        assert!(load().contains(probe), "and known to the checker");

        remember(probe);
        let count = additions().iter().filter(|w| *w == probe).count();
        assert_eq!(count, 1, "remembering twice does not duplicate it");

        // Put the user's own list back exactly as it was.
        if before.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else {
            let _ = std::fs::write(&path, before);
        }
    }
}
