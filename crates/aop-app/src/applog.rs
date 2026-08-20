//! A written record of what the application did, beside the settings.
//!
//! Two of the things this program does cannot be watched while they happen.
//! Signing back in occurs before there is a window to say anything in, and
//! live editing is a conversation between a timer, a queue and a socket
//! thread that no part of the interface can be asked about afterwards. Both
//! have failed silently, and both cost a day each to find by reading code,
//! because the one thing that reading cannot tell you is which branch was
//! actually taken.
//!
//! So the branches say so themselves, into `log.log` in the configuration
//! directory. The rule that makes it worth having is that a decision **not**
//! to do something is written down as loudly as doing it: the failure that
//! costs the most is the one where something works once and then quietly
//! stops, and a log of successes cannot show that.
//!
//! ```text
//!   <config root>/log.log      this run
//!   <config root>/log.log.1    the run before it
//! ```
//!
//! One session per file rather than one long file, so it cannot grow without
//! bound and so the session being asked about is the only one in it.
//!
//! **The previous run is kept, and that is the whole point of two files.** A
//! fault that happens while the application is closing is written in the last
//! lines this file ever gets, and the next start up is what erases them. That
//! is not hypothetical: a session that vanishes between runs can only be
//! explained by what happened at the end of the run before, and a log cut open
//! at start up destroys exactly that evidence, having been added to capture
//! it. Keeping one behind costs a rename.
//!
//! Three rules hold everywhere in here:
//!
//! * **Never a token.** Not a token, not part of one, not its length. That a
//!   session was restored and for whom is the useful fact; what was in it is
//!   not. The same goes for the live socket's address, which carries an
//!   access token in its query string.
//! * **Never a reason to fail.** A log file that cannot be opened is not an
//!   error. Everything here degrades to doing nothing at all.
//! * **Never a flood.** Presence goes out about eight times a second while a
//!   mouse is moving, and a log that records each one buries everything else.
//!   The repetitive lines are counted and summarised; `AOP_LOG_VERBOSE=1`
//!   turns each one back on for when that is what is wanted.

use std::fmt::Write as _;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// How often the live timer says it is still turning.
///
/// It exists to answer one question, which is whether the loop is running at
/// all, and that question does not need answering eight times a second.
pub const HEARTBEAT_MILLIS: u64 = 5_000;

/// How often the repetitive live traffic is summarised.
///
/// Shorter than the heartbeat because a gap in it is the interesting shape:
/// a pointer that stopped being sent while a mouse was still moving should be
/// visible as a hole in the log rather than as a slower line.
pub const PRESENCE_SUMMARY_MILLIS: u64 = 2_000;

/// Where the lines go, opened once and kept.
///
/// `None` is a perfectly ordinary outcome: no configuration directory, a
/// read-only home, a file somebody else owns. Every write then does nothing.
static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

/// When the process started, so every line carries an offset that can be
/// subtracted in the head. Wall clock goes on the first line and nowhere else.
static START: OnceLock<Instant> = OnceLock::new();

/// Open the log and write the first line.
///
/// Called once from `main`, before anything that might have something to say.
/// Doing it here rather than lazily is what makes "truncated at start up"
/// true: the first thing to log would otherwise decide when the file is cut.
pub fn start(version: &str) {
    START.get_or_init(Instant::now);
    let _ = sink();
    line(format_args!(
        "start: Alterion Open Project {version} on {} {}, {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
    ));
}

/// Whether anything is being recorded at all.
///
/// Asked before a line is built, so a build with nowhere to write pays a
/// relaxed atomic load and no formatting.
pub fn on() -> bool {
    sink().is_some()
}

/// Whether every repetitive line was asked for.
pub fn verbose() -> bool {
    static VERBOSE: OnceLock<bool> = OnceLock::new();
    *VERBOSE.get_or_init(|| {
        std::env::var_os("AOP_LOG_VERBOSE").is_some_and(|value| value != "0" && !value.is_empty())
    })
}

fn sink() -> Option<&'static Mutex<File>> {
    SINK.get_or_init(open).as_ref()
}

fn open() -> Option<Mutex<File>> {
    // A test run must not truncate the log of the copy somebody is using, and
    // the configuration directory is a real one during tests.
    if cfg!(test) {
        return None;
    }
    let path = crate::settings::config_root()?.join("log.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    rotate(&path);
    File::create(&path).ok().map(Mutex::new)
}

/// What the run before this one wrote.
///
/// The suffix goes after the whole name rather than replacing the extension,
/// so the two files sort together and neither is mistaken for something a
/// person is meant to open by double clicking.
fn previous(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    path.with_file_name(name)
}

/// Move the finished run's log aside before this one starts writing.
///
/// A rename rather than a copy: the previous process is gone, nothing holds
/// the file, and a rename either happens or does not, where a copy can stop
/// half way and leave two partial logs and no whole one. Failure is ignored
/// like everything else here. There being no previous log is the ordinary case
/// on a first run, and it is not worth a word.
fn rotate(path: &Path) {
    let _ = std::fs::rename(path, previous(path));
}

/// Write one line, with the seconds since start up in front of it.
pub fn line(what: std::fmt::Arguments<'_>) {
    let Some(sink) = sink() else {
        return;
    };
    let elapsed = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    let mut text = String::with_capacity(96);
    // A formatting failure into a String cannot happen, and if it somehow did
    // the line is still worth writing as far as it got.
    let _ = write!(text, "[{elapsed:9.3}] {what}");
    // A poisoned lock means another thread panicked mid-write. The file is
    // still a file, and refusing to log from then on would hide whatever came
    // after the panic, which is the part worth reading.
    let mut file = match sink.lock() {
        Ok(file) => file,
        Err(poisoned) => poisoned.into_inner(),
    };
    let _ = writeln!(file, "{text}");
}

/// One line, if there is anywhere to put it.
macro_rules! applog {
    ($($arg:tt)*) => {
        if $crate::applog::on() {
            $crate::applog::line(format_args!($($arg)*));
        }
    };
}

/// One line, only when every repetitive line was asked for.
macro_rules! applog_verbose {
    ($($arg:tt)*) => {
        if $crate::applog::verbose() && $crate::applog::on() {
            $crate::applog::line(format_args!($($arg)*));
        }
    };
}

pub(crate) use {applog, applog_verbose};

/// A counter for something that happens too often to write down each time.
///
/// The flood is the point of it: presence is capped at eight a second and the
/// live timer turns eight times a second, so writing each one down would push
/// the connection and the refusals that actually explain a fault off the top
/// of the file. What is wanted from the repetitive lines is only that they
/// are still happening and roughly how fast, which is one line every few
/// seconds.
pub struct Tally {
    what: &'static str,
    count: AtomicU64,
    /// Milliseconds since start up at the last summary, so the whole thing
    /// stays lock free and usable from a `static`.
    last: AtomicU64,
    every_millis: u64,
}

impl Tally {
    pub const fn new(what: &'static str, every_millis: u64) -> Tally {
        Tally {
            what,
            count: AtomicU64::new(0),
            last: AtomicU64::new(0),
            every_millis,
        }
    }

    /// Count one, and write a summary if it is time for one.
    ///
    /// `detail` describes the most recent occurrence, so the summary says what
    /// the last one looked like as well as how many there were.
    pub fn note(&self, detail: std::fmt::Arguments<'_>) {
        if !on() {
            return;
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        let elapsed = START.get_or_init(Instant::now).elapsed();
        let millis = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        let last = self.last.load(Ordering::Relaxed);
        let window = millis.saturating_sub(last);
        if window < self.every_millis {
            return;
        }
        // Whoever wins the swap writes the line. A loser has already had its
        // occurrence counted, so nothing is lost by it staying quiet.
        if self
            .last
            .compare_exchange(last, millis, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        let since = self.count.swap(0, Ordering::Relaxed);
        line(format_args!(
            "{}: {since} in the last {:.1}s, latest {detail}",
            self.what,
            window as f64 / 1000.0,
        ));
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aop-applog-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory to work in");
        dir
    }

    #[test]
    fn the_previous_log_sits_beside_the_current_one() {
        // The suffix goes after the whole name, so the pair sort together and
        // the older one is still recognisably a log rather than a `.1` file.
        assert_eq!(
            previous(Path::new("/somewhere/log.log")),
            PathBuf::from("/somewhere/log.log.1")
        );
    }

    #[test]
    fn a_finished_run_is_kept_rather_than_overwritten() {
        let dir = scratch("kept");
        let log = dir.join("log.log");

        std::fs::write(&log, "the run that ended").expect("write");
        rotate(&log);

        assert!(!log.exists(), "the current name is free for the new run");
        assert_eq!(
            std::fs::read_to_string(previous(&log)).expect("the previous run"),
            "the run that ended",
            "what the last run wrote has to survive the next start up, because \
             a fault while closing is written in its final lines"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_one_run_is_kept_behind() {
        let dir = scratch("one");
        let log = dir.join("log.log");

        std::fs::write(&log, "oldest").expect("write");
        rotate(&log);
        std::fs::write(&log, "newest").expect("write");
        rotate(&log);

        // Two files, never three. The run before last is not worth unbounded
        // disk, and the question is always about the run that just ended.
        assert_eq!(
            std::fs::read_to_string(previous(&log)).expect("the previous run"),
            "newest"
        );
        assert!(!dir.join("log.log.1.1").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_run_with_nothing_to_keep_is_not_a_failure() {
        let dir = scratch("first");
        let log = dir.join("log.log");

        // Nothing there yet. Rotating must be silent, not an error, or the
        // first start on a new machine would report a fault about a file that
        // has never existed.
        rotate(&log);
        assert!(!previous(&log).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
