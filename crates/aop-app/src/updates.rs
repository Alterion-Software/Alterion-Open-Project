//! Finding a newer release, and replacing this copy with it where that is
//! this copy's business to do.
//!
//! The first question is not "is there a new version" but "who owns these
//! files". An application installed by a package manager must not update
//! itself: the files under `/usr` belong to pacman, and writing over them
//! leaves a system whose package database describes something that is no
//! longer there. The next `pacman -Syu` then either puts the old version back
//! over the new one or refuses outright on a conflict, and either way somebody
//! is left with a machine that disagrees with itself. So an install a package
//! manager owns is told a new version exists and told how to get it, which is
//! a more useful answer than a button that would break it.
//!
//! ```text
//!   where is the running binary?
//!     /usr, /opt, a package manager's file  ->  say how, never touch it
//!     inside a .app bundle                  ->  say how, the .dmg is the unit
//!     could not be worked out               ->  say how, nothing on a guess
//!     anywhere else                         ->  ours to replace
//! ```
//!
//! # Two files, and no API
//!
//! A check reads two plain files.
//!
//! ```text
//!   latest        one line, the version, nothing else
//!   SHA256SUMS    a digest and a filename per line, sha256sum's own format
//! ```
//!
//! Both live at a fixed address in GitLab's generic package registry, under
//! the literal version string `latest`. The registry takes any string where a
//! version goes, so that slot is a permanent address which no release
//! semantics can move. This is not a nicety: GitHub's own `/releases/latest/`
//! skips pre-releases, and a pre-release is exactly the sort of release an
//! updater matters most for, while GitLab's release permalink needs direct
//! asset links the generic registry does not produce. A slot that is simply
//! always there sidesteps both.
//!
//! A routine check fetches `latest` and stops there, so the usual cost of
//! having update checks switched on is a dozen bytes. Only a version that is
//! actually newer is worth fetching the manifest for.
//!
//! `SHA256SUMS` is the manifest, and reading it is how this finds out whether
//! there is a build for this platform at all. That matters more than it
//! sounds: the macOS and Windows artefacts are built by workflows somebody
//! presses by hand after the release already exists, so there is a real window
//! where `latest` says 1.0.1 and no disk image has been attached yet, and
//! there are releases where a workflow failed and one never appears. A missing
//! line is the truthful answer to "is there a build for me", and it is said
//! out loud.
//!
//! Once the manifest has named a file, building an address around that name is
//! fine, and it is the only kind of address building that is. The rule being
//! kept is that no *filename* is ever invented from a naming convention: the
//! manifest is what vouches for a file existing, so a name it did not give is
//! a file nobody said was there.
//!
//! # What the digest proves, and what it does not
//!
//! Checking the download against `SHA256SUMS` proves the bytes that arrived
//! are the bytes that manifest names. It does not prove the manifest is
//! genuine. The only thing standing behind that is HTTPS to a known host:
//! `SHA256SUMS` is not signed, so nothing here authenticates a release, and no
//! comment in this file should claim otherwise. What can be said truthfully is
//! that the artefact is checked against the published checksum, and that a
//! download which does not match it is never run.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dioxus::prelude::*;
use sha2::{Digest, Sha256};

use crate::state::AppState;
use crate::welcome::RUNNING;

/// The project on GitLab, url encoded, and its mirror on GitHub.
///
/// GitLab is the origin and is asked first everywhere below. GitHub is a
/// mirror: it may lag, and for a given release it may not have happened yet.
const GITLAB_PROJECT: &str = "alterion-software%2Falterion-open-project";
const GITHUB_REPO: &str = "Alterion-Software/Alterion-Open-Project";

/// The generic package the GitLab half of a release uploads its bytes into.
const GITLAB_PACKAGE: &str = "alterion-open-project";

/// The registry slot the two descriptor files live in.
///
/// A literal string standing where a version goes, which is what makes the
/// address permanent. `release.sh` writes this slot from both `--publish` and
/// `--manifest`, through one function, so neither path can update it and the
/// other forget.
const FIXED_SLOT: &str = "latest";

/// The one line file naming the newest version.
const LATEST_NAME: &str = "latest";

/// The manifest, in `sha256sum` format.
const SUMS_NAME: &str = "SHA256SUMS";

/// What the executable is called inside a release tarball.
const BINARY_NAME: &str = "alterion-open-project";

/// How long after a start up before anything is asked.
///
/// Long enough to be behind the splash and whatever the first plan does, so a
/// slow name resolution is never part of how long the window takes to appear.
pub const STARTUP_DELAY_SECONDS: u64 = 4;

/// An update check happens behind somebody's back, so it gives up rather than
/// hanging about. Nothing is waiting on the answer.
const REQUEST_TIMEOUT_SECONDS: u64 = 15;

/// How much of a download to accept. Generous next to any artefact this
/// project publishes, and still a bound: without one a server that keeps
/// sending is a server that exhausts memory.
const MAX_ARTEFACT: u64 = 192 * 1024 * 1024;

/// A manifest is a few lines, and one that is not is not a manifest.
const MAX_MANIFEST: u64 = 256 * 1024;

/// `latest` holds a version and a newline. Anything beyond this is something
/// else wearing its name, such as a captive portal's login page.
const MAX_LATEST: u64 = 1024;

// ------------------------------------------------------- how it got here

/// How this copy was installed, which decides whether it may replace itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// A package manager owns these files and is the only thing that should
    /// write them.
    Packaged {
        name: &'static str,
        /// What to run instead.
        command: &'static str,
    },
    /// Under a system prefix, so put there for everybody by
    /// `install.sh --system` or by packaging this does not recognise. Not this
    /// account's files to rewrite even where no package database mentions
    /// them.
    SystemWide,
    /// Inside a macOS application bundle. One binary within a bundle cannot be
    /// swapped without invalidating the bundle's signature, and the release is
    /// a disk image rather than a binary, so the image is the unit.
    Bundle,
    /// Ours to replace: a user local install, a plain extracted binary, or a
    /// build run out of its own target directory.
    SelfManaged,
    /// The running program's path could not be read. Nothing gets replaced on
    /// a guess, so this is treated exactly like an install somebody else owns.
    Unknown,
}

/// Prefixes that belong to the system rather than to one user.
///
/// `/usr/local` is under `/usr`, which is where `install.sh --system` puts
/// things, and `/usr/bin` is where the AUR package does.
const SYSTEM_PREFIXES: [&str; 7] = [
    "/usr/",
    "/opt/",
    "/bin/",
    "/sbin/",
    "/snap/",
    "/nix/store/",
    // Flatpak mounts the application's own tree here.
    "/app/",
];

/// Work out how this copy was installed rather than assuming.
pub fn detect() -> Install {
    let Ok(exe) = std::env::current_exe() else {
        return Install::Unknown;
    };
    // Through any symlink first: `/usr/bin/name` pointing into `/opt` is one
    // install, and only the real path says which.
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let path = exe.to_string_lossy().replace('\\', "/");

    if path.contains(".app/Contents/MacOS/") {
        return Install::Bundle;
    }
    if !SYSTEM_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return Install::SelfManaged;
    }
    // Only asked once the path is one a package might own, so a user local
    // install never spawns anything at start up.
    if let Some(packaged) = package_manager_for(&exe) {
        return packaged;
    }
    Install::SystemWide
}

/// Ask whether a package manager claims this file.
///
/// Asked of the manager rather than inferred from the path, because the path
/// says where a file is and only the package database says who put it there.
#[cfg(target_os = "linux")]
fn package_manager_for(path: &Path) -> Option<Install> {
    use std::process::{Command, Stdio};

    let owned = Command::new("pacman")
        .arg("-Qo")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    owned.then_some(Install::Packaged {
        name: "pacman",
        command: "sudo pacman -Syu",
    })
}

#[cfg(not(target_os = "linux"))]
fn package_manager_for(_path: &Path) -> Option<Install> {
    None
}

impl Install {
    /// Whether this copy may replace its own files, and has a kind of artefact
    /// it could replace them with.
    pub fn self_updates(self) -> bool {
        self == Install::SelfManaged && wanted_asset().is_some_and(|(_, kind)| kind.installable())
    }

    /// How to get the new version, for the copies that must not fetch it
    /// themselves. Phrased as the thing to do, not as a refusal.
    pub fn advice(self) -> String {
        match self {
            Install::Packaged { name, command } => format!(
                "This copy was installed by {name}, which owns its files. Update it the way it was \
                 installed: {command}. Replacing those files from here would be undone by the next \
                 system upgrade, or would make that upgrade fail on a conflict."
            ),
            Install::SystemWide => {
                "This copy is installed for everyone on this machine, so its files are not this \
                 account's to rewrite. Update it by running install.sh again, or through whatever \
                 package installed it."
                    .into()
            }
            Install::Bundle => {
                "This copy is a macOS application bundle. Download the new disk image and drag it \
                 across, which keeps the bundle and its signature intact."
                    .into()
            }
            Install::Unknown => {
                "Where this copy is installed could not be worked out, so nothing here will \
                 overwrite it. Update it the way you installed it."
                    .into()
            }
            Install::SelfManaged => {
                "Download the new release and put it where this one is.".into()
            }
        }
    }
}

// ------------------------------------------------------------- addresses

/// One file in GitLab's generic package registry.
///
/// `version` is whatever string the slot is named after. For the two
/// descriptor files that is the literal `latest`; for an artefact it is the
/// release's own version.
fn gitlab_slot(version: &str, name: &str) -> String {
    format!(
        "https://gitlab.com/api/v4/projects/{GITLAB_PROJECT}/packages/generic/{GITLAB_PACKAGE}/{version}/{name}"
    )
}

/// One file attached to a GitHub release.
fn github_download(version: &str, name: &str) -> String {
    format!("https://github.com/{GITHUB_REPO}/releases/download/v{version}/{name}")
}

/// Where a descriptor file is asked for, in order.
///
/// The fixed GitLab slot first, because it is the origin and because it
/// answers whatever the newest release happens to be, pre-release or not.
/// GitHub's `/releases/latest/` is kept behind it as a second try and is only
/// ever a fallback: it skips pre-releases, so it says nothing at all for the
/// releases this matters most for, and it is a mirror that may not have the
/// release yet.
fn descriptor_urls(name: &str) -> [String; 2] {
    [
        gitlab_slot(FIXED_SLOT, name),
        format!("https://github.com/{GITHUB_REPO}/releases/latest/download/{name}"),
    ]
}

/// Where an artefact is fetched from, in order.
///
/// Both addresses are built around a filename the manifest has already
/// vouched for, which is what makes building them legitimate. Neither invents
/// a name from a convention.
fn artefact_urls(version: &str, name: &str) -> [String; 2] {
    [gitlab_slot(version, name), github_download(version, name)]
}

/// Where a person would go to fetch a release by hand.
fn release_page(version: &str) -> String {
    format!("https://gitlab.com/alterion-software/alterion-open-project/-/releases/v{version}")
}

// ---------------------------------------------------------- what is out there

/// One artefact from the manifest: what it is called, and what it has to hash
/// to.
///
/// No address of its own. Where it can be fetched from follows from the
/// version and this name, and there is more than one such place, so keeping a
/// single URL here would be recording one of the answers as though it were the
/// answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artefact {
    pub name: String,
    /// The digest `SHA256SUMS` records for it, lower case hexadecimal.
    pub digest: String,
}

/// A release newer than the one running, and what this copy can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub version: String,
    pub install: Install,
    /// The file this platform installs from, or nothing when the release has
    /// no build for this platform. Absent is a real answer and is said out
    /// loud: those artefacts are built by hand after the release exists, so a
    /// version can be out with a platform's build not yet attached.
    pub artefact: Option<Artefact>,
    /// Where to fetch it by hand, for every case this copy will not do it.
    pub page: String,
}

impl Found {
    /// Whether this copy can install this release itself.
    pub fn installable(&self) -> bool {
        self.install.self_updates() && self.artefact.is_some()
    }

    /// Why it cannot, when it cannot, in words meant for the person reading.
    pub fn why_not(&self) -> Option<String> {
        if self.artefact.is_none() {
            return Some(format!(
                "Version {} is out, but it has no build for this platform. Those are published \
                 separately and one may not have been attached yet.",
                self.version
            ));
        }
        (!self.install.self_updates()).then(|| self.install.advice())
    }
}

/// Look for a release newer than the one running.
///
/// `Ok(None)` means there is not one, which is the ordinary answer and not
/// worth telling anybody about. Blocking from beginning to end, so it belongs
/// on a worker thread.
pub fn check() -> Result<Option<Found>, String> {
    let latest = fetch_first(&descriptor_urls(LATEST_NAME), MAX_LATEST)?;
    let version = read_latest(&latest)
        .ok_or_else(|| "the version file did not hold a version".to_string())?;

    // The whole point of `latest` being one line: on the ordinary answer,
    // nothing further is fetched at all.
    if !is_newer(&version, RUNNING) {
        return Ok(None);
    }

    // The manifest for a specific release sits in that release's own slot, not
    // in the fixed one. The fixed slot's copy describes whatever is newest,
    // which is the same thing here and would not be if a release landed
    // between the two requests.
    let sums = fetch_first(&artefact_urls(&version, SUMS_NAME), MAX_MANIFEST)?;
    let artefact = wanted_asset()
        .and_then(|(tail, _)| line_for(&sums, tail))
        .map(|(name, digest)| Artefact { name, digest });

    Ok(Some(Found {
        install: detect(),
        artefact,
        page: release_page(&version),
        version,
    }))
}

/// The version `latest` names.
///
/// The first line that says something, so a trailing newline or a stray blank
/// one is not a parse failure. Anything that is not a version reads as nothing
/// rather than as a version: an error page fetched from the wrong address must
/// not become an update offer.
fn read_latest(body: &str) -> Option<String> {
    let line = body.lines().map(str::trim).find(|line| !line.is_empty())?;
    // A `v` prefix is not expected here, but stripping one costs nothing and
    // saves a release that has one from silently never being offered.
    let version = line.trim_start_matches('v');
    parts_of(version).map(|_| version.to_string())
}

/// Read the first of these addresses that answers.
///
/// Every failure is carried, not just the last one. "GitLab did not resolve,
/// and GitHub answered with status 404" is a diagnosis; "could not check for
/// updates" is not.
fn fetch_first(urls: &[String], limit: u64) -> Result<String, String> {
    let mut trouble = Vec::new();
    for url in urls {
        match get(url, limit) {
            Ok(body) => return Ok(body),
            Err(why) => trouble.push(why),
        }
    }
    Err(trouble.join(", and "))
}

fn get(url: &str, limit: u64) -> Result<String, String> {
    ureq::get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .call()
        .map_err(|error| {
            format!(
                "{} could not be asked ({})",
                host_of(url),
                crate::cloud::oauth::describe(&error)
            )
        })?
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_string()
        .map_err(|_| format!("the reply from {} did not finish arriving", host_of(url)))
}

/// The host part of an address, for saying which one failed.
fn host_of(url: &str) -> &str {
    url.split('/').nth(2).unwrap_or(url)
}

// ------------------------------------------------------------ the manifest

/// Find this platform's line in the manifest.
///
/// Matched by the end of the filename, because the version sits in the middle
/// of it: reconstructing the whole name here would be a second place the
/// naming has to be kept in step with `release.sh`, and the two would drift.
///
/// The format is `sha256sum`'s own: the digest, whitespace, then the name,
/// which in binary mode is preceded by an asterisk. Split on whitespace rather
/// than parsed, since that is all there is to it.
fn line_for(sums: &str, tail: &str) -> Option<(String, String)> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let digest = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        if !name.ends_with(tail) {
            return None;
        }
        // A line whose first field is not a digest is not a digest line,
        // whatever else it looks like.
        (digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| (name.to_string(), digest.to_ascii_lowercase()))
    })
}

/// Check a download against the digest published for it.
///
/// This proves the bytes are the bytes `SHA256SUMS` names. It does not prove
/// that manifest is genuine, which nothing here does: it is unsigned, and
/// HTTPS to a known host is the whole of the trust. What it does rule out is
/// running an artefact that is not the one published, whether it was altered
/// in transit, served from a cache, or simply truncated.
pub fn verify(bytes: &[u8], want: &str) -> Result<(), String> {
    let got: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if !got.eq_ignore_ascii_case(want.trim()) {
        return Err(format!(
            "The download does not match the checksum published for it, so it has not been \
             installed. Expected {}, got {}.",
            &want.trim()[..want.trim().len().min(12)],
            &got[..12]
        ));
    }
    Ok(())
}

// ------------------------------------------------------ comparing versions

/// A version split into the pieces that are compared.
type Parts = (u64, u64, u64, Option<String>);

/// Split `1.2.3-beta` into its numbers and its pre-release, or nothing when it
/// is not a version at all.
fn parts_of(version: &str) -> Option<Parts> {
    // Build metadata takes no part in ordering, so it is dropped here.
    let version = version.split('+').next()?.trim();
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_string())),
        _ => (version, None),
    };

    let mut numbers = core.split('.');
    let major = numbers.next()?.parse().ok()?;
    // A tag of `2` means 2.0.0. Insisting on all three would refuse it.
    let minor = numbers.next().unwrap_or("0").parse().ok()?;
    let patch = numbers.next().unwrap_or("0").parse().ok()?;
    if numbers.next().is_some() {
        return None;
    }
    Some((major, minor, patch, pre))
}

/// Order two versions the way semantic versioning does.
///
/// Two things here are not obvious and both are why this is not a string
/// comparison. `1.0.10` is newer than `1.0.9`, which comparing text gets
/// backwards. And a pre-release comes *before* the same numbers without one,
/// so `1.0.0-beta` is older than `1.0.0`; text puts it after, and every beta
/// would then refuse the release it was a beta of.
fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let (Some(left), Some(right)) = (parts_of(a), parts_of(b)) else {
        return Ordering::Equal;
    };
    let numbers = (left.0, left.1, left.2).cmp(&(right.0, right.1, right.2));
    if numbers != Ordering::Equal {
        return numbers;
    }

    match (&left.3, &right.3) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => compare_pre(left, right),
    }
}

/// Compare two pre-release strings identifier by identifier.
///
/// Numeric identifiers compare as numbers, so `beta.10` is after `beta.9`
/// rather than before it, and a numeric identifier ranks below a textual one.
fn compare_pre(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            // Fewer identifiers means the earlier release, all else equal.
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(one), Some(two)) => {
                let order = match (one.parse::<u64>(), two.parse::<u64>()) {
                    (Ok(one), Ok(two)) => one.cmp(&two),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => one.cmp(two),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// Whether `candidate` is a release worth offering to somebody on `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    compare(candidate, current) == std::cmp::Ordering::Greater
}

// ------------------------------------------------------------- the artefact

/// What is done with the artefact this platform downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A tarball with the binary inside, swapped into place here.
    Tarball,
    /// An installer, which is run rather than copied. Checked here and then
    /// handed over, because running it closes this application.
    Installer,
    /// A disk image, which a person mounts and drags across. Worth knowing
    /// exists, and not something this can install.
    Image,
}

impl Kind {
    /// Whether this application can put such an artefact in place itself.
    fn installable(self) -> bool {
        matches!(self, Kind::Tarball | Kind::Installer)
    }
}

/// How this platform's artefact is recognised in the manifest, and what
/// becomes of it.
///
/// One function asking `cfg!` rather than three behind `#[cfg]`, so every arm
/// is compiled everywhere. That keeps the whole table readable side by side,
/// and means a change to the Windows naming is caught by a Linux build rather
/// than by a Windows user.
fn wanted_asset() -> Option<(&'static str, Kind)> {
    if cfg!(target_os = "linux") {
        Some(("-x86_64-linux.tar.gz", Kind::Tarball))
    } else if cfg!(target_os = "windows") {
        Some(("-setup.exe", Kind::Installer))
    } else if cfg!(target_os = "macos") {
        Some((".dmg", Kind::Image))
    } else {
        None
    }
}

/// What an update left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Installed {
    /// The binary has been replaced, and the previous one kept at this path in
    /// case the new one turns out not to start.
    Replaced { kept: PathBuf },
    /// The installer has been downloaded and checked. Running it is a separate
    /// and deliberate step, since it closes this application.
    Downloaded { installer: PathBuf },
}

/// Fetch a release's artefact, check it, and put it in place.
///
/// Blocking from beginning to end, so it belongs on a worker thread. Every
/// refusal is worded as what went wrong rather than as a status, because the
/// only reader is somebody who pressed a button and is owed a reason.
pub fn install(found: &Found) -> Result<Installed, String> {
    let Some(artefact) = &found.artefact else {
        return Err(format!(
            "Version {} has no build for this platform, so there is nothing to install.",
            found.version
        ));
    };
    let Some((_, kind)) = wanted_asset() else {
        return Err("There is no artefact for this platform to install from.".into());
    };

    let bytes = download(&artefact_urls(&found.version, &artefact.name))?;
    verify(&bytes, &artefact.digest)?;

    match kind {
        Kind::Tarball => {
            let binary = entry_in_tar(&decompress(&bytes)?, BINARY_NAME).ok_or_else(|| {
                format!(
                    "{} does not contain {BINARY_NAME}, so there is nothing to install.",
                    artefact.name
                )
            })?;
            swap_in(&binary).map(|kept| Installed::Replaced { kept })
        }
        Kind::Installer => {
            keep_installer(&artefact.name, &bytes).map(|installer| Installed::Downloaded {
                installer,
            })
        }
        Kind::Image => Err(
            "A disk image is mounted and dragged across by hand, so this one has not been \
             downloaded."
                .into(),
        ),
    }
}

/// Fetch the artefact from the first address that will give it up.
///
/// Falling back host to host is safe here in a way it would not be without the
/// manifest: whichever one answers, the bytes still have to hash to what
/// `SHA256SUMS` said, so a second address is another chance at the same file
/// rather than a second file that would be accepted on different terms.
fn download(urls: &[String]) -> Result<Vec<u8>, String> {
    let mut trouble = Vec::new();
    for url in urls {
        match fetch_bytes(url) {
            Ok(bytes) => return Ok(bytes),
            Err(why) => trouble.push(why),
        }
    }
    Err(format!("The download did not succeed: {}.", trouble.join(", and ")))
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    ureq::get(url)
        .config()
        // No global timeout on the transfer itself: a slow connection is not a
        // failure, and cutting one off part way through would look like one.
        .build()
        .call()
        .map_err(|error| {
            format!(
                "{} could not be reached ({})",
                host_of(url),
                crate::cloud::oauth::describe(&error)
            )
        })?
        .body_mut()
        .with_config()
        .limit(MAX_ARTEFACT)
        .read_to_vec()
        .map_err(|_| format!("the transfer from {} did not finish", host_of(url)))
}

/// Undo the gzip wrapper around a tarball.
fn decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|_| "The download is not a readable archive.".to_string())?;
    Ok(out)
}

/// Pull one named file out of a tar archive.
///
/// Written out rather than taken as a dependency, and narrower than a tar
/// reader on purpose: it compares the names in the archive and never writes
/// one. A name inside an archive is written by whoever made the archive, and
/// `../../` in one is how an extractor becomes a way to write anywhere on the
/// disk. This one has no such path to take, because the only place the
/// extracted bytes ever go is a path this program built itself.
fn entry_in_tar(archive: &[u8], want: &str) -> Option<Vec<u8>> {
    let mut at = 0usize;
    while at + 512 <= archive.len() {
        let header = &archive[at..at + 512];
        // A run of zero blocks marks the end of the archive.
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }

        let name = text_field(header, 0, 100);
        let size = octal_field(header, 124, 12)?;
        let kind = header[156];
        at = at.checked_add(512)?;

        let end = at.checked_add(size)?;
        if end > archive.len() {
            return None;
        }
        // `0` and a nul byte both mean an ordinary file. Everything else is a
        // directory, a link or an extension header, and none of those is a
        // binary to install.
        if (kind == b'0' || kind == 0) && name == want {
            return Some(archive[at..end].to_vec());
        }
        // Entry bodies are padded out to a whole number of blocks.
        at = at.checked_add(size.div_ceil(512).checked_mul(512)?)?;
    }
    None
}

/// A nul terminated text field from a tar header.
fn text_field(header: &[u8], at: usize, len: usize) -> String {
    let field = &header[at..at + len];
    let end = field.iter().position(|byte| *byte == 0).unwrap_or(len);
    String::from_utf8_lossy(&field[..end]).trim().to_string()
}

/// An octal number field from a tar header.
fn octal_field(header: &[u8], at: usize, len: usize) -> Option<usize> {
    let text = text_field(header, at, len);
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    usize::from_str_radix(text, 8).ok()
}

/// Put the new binary where the running one is, in one step that cannot leave
/// a half written file behind the name.
///
/// The old binary is copied aside *before* anything is moved, so there is
/// something to go back to whether the swap fails or the new binary turns out
/// not to start. The staging file is written next to the target rather than in
/// a temporary directory, because a rename is only atomic within one
/// filesystem and `/tmp` very often is not the same one.
fn swap_in(binary: &[u8]) -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|_| "Where this program is installed could not be read.".to_string())?;
    let directory = exe
        .parent()
        .ok_or_else(|| "This program is not in a directory it can be replaced in.".to_string())?;
    let name = exe
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| BINARY_NAME.to_string());

    let staged = directory.join(format!(".{name}.new"));
    std::fs::write(&staged, binary).map_err(|error| {
        format!(
            "The new version could not be written to {}: {error}",
            directory.display()
        )
    })?;
    if let Err(error) = make_runnable(&staged) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }

    let kept = directory.join(format!("{name}.old"));
    let _ = std::fs::remove_file(&kept);
    if let Err(error) = std::fs::copy(&exe, &kept) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!(
            "The current version could not be set aside, so it has been left alone: {error}"
        ));
    }

    // The one step that changes anything. Until it lands the name still points
    // at the working binary, and if it fails nothing has moved.
    if let Err(error) = std::fs::rename(&staged, &exe) {
        let _ = std::fs::remove_file(&staged);
        let _ = std::fs::remove_file(&kept);
        return Err(format!(
            "The new version could not be put in place, so this one has been left as it was: {error}"
        ));
    }
    Ok(kept)
}

/// Give a freshly written binary the permission to be one.
#[cfg(unix)]
fn make_runnable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("The new version could not be made executable: {error}"))
}

#[cfg(not(unix))]
fn make_runnable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Keep a checked installer somewhere it can be run from.
fn keep_installer(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, bytes)
        .map_err(|error| format!("The installer could not be saved: {error}"))?;
    Ok(path)
}

/// Hand the installer to the system and let it take over.
pub fn run_installer(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("The installer would not start: {error}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("Installers are only run on Windows.".into())
    }
}

// -------------------------------------------------------- starting the work
//
// Everything above blocks, and none of it may run where the interface does.
// The shape is the one the collaborate machinery uses: gather what the work
// needs, hand it to a thread, take the answer back where the state can be
// written.

/// Look for a newer release, off the interface's thread.
///
/// Silent when update checks are switched off, which is what "honoured
/// everywhere" means: there is no path that asks anyway because the user
/// pressed something.
pub fn ask_in_background(mut state: Signal<AppState>) {
    if !state.read().update_check || state.read().updating {
        return;
    }
    state.write().updating = true;
    crate::cloud::off_thread(check, move |outcome| match outcome {
        Some(outcome) => state.write().update_landed(outcome),
        // A worker that did not come back is not worth a word to anybody. The
        // next check is the next start.
        None => state.write().updating = false,
    });
}

/// Fetch, check and install the release that was found, off the interface's
/// thread.
///
/// Asked for explicitly every time. Nothing here runs because a check found
/// something; it runs because somebody pressed the button that says so.
pub fn install_in_background(mut state: Signal<AppState>) {
    let Some(found) = state.read().update_found.clone() else {
        return;
    };
    if state.read().update_blocked().is_some() {
        return;
    }
    state.write().updating = true;
    crate::cloud::off_thread(
        move || install(&found),
        move |outcome| match outcome {
            Some(outcome) => state.write().install_landed(outcome),
            None => {
                let mut writer = state.write();
                writer.updating = false;
                writer.update_message = Some(
                    "The update stopped unexpectedly. Nothing has been replaced.".into(),
                );
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- comparing versions ---------------------------------------------

    #[test]
    fn a_pre_release_is_older_than_the_release_it_leads_to() {
        // Comparing the strings would say the opposite, and every beta would
        // then refuse the release it was a beta of.
        assert!(is_newer("1.0.0", "1.0.0-beta"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn ten_is_newer_than_nine_rather_than_earlier_in_the_alphabet() {
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(!is_newer("1.0.9", "1.0.10"));
        assert!(is_newer("1.10.0", "1.9.0"));
    }

    #[test]
    fn newer_numbers_win_over_older_ones() {
        assert!(is_newer("1.1.0", "1.0.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn pre_release_numbers_compare_as_numbers() {
        assert!(is_newer("1.0.0-beta.10", "1.0.0-beta.9"));
        assert!(is_newer("1.0.0-beta.2", "1.0.0-alpha.9"));
    }

    #[test]
    fn something_that_is_not_a_version_is_never_newer() {
        assert!(!is_newer("nightly", "1.0.0"));
        assert!(!is_newer("", "1.0.0"));
        assert!(!is_newer("1.0.0.0", "1.0.0"));
    }

    // ---- the latest file ------------------------------------------------

    #[test]
    fn latest_is_one_line_and_read_as_one() {
        assert_eq!(read_latest("1.0.1\n"), Some("1.0.1".to_string()));
        assert_eq!(read_latest("  1.0.1  \n\n"), Some("1.0.1".to_string()));
        assert_eq!(read_latest("v1.0.1\n"), Some("1.0.1".to_string()));
    }

    #[test]
    fn something_that_is_not_a_version_does_not_become_an_update() {
        // A captive portal's login page, an error document, an empty file:
        // none of those is a release, and none may look like one.
        for body in ["", "\n\n", "<html>Sign in to the network</html>", "Not Found"] {
            assert_eq!(read_latest(body), None, "{body:?}");
        }
    }

    // ---- the manifest ---------------------------------------------------

    const MANIFEST: &str = "\
a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90  alterion-open-project-1.0.1-x86_64-linux.tar.gz
c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2  AlterionOpenProject-1.0.1-setup.exe
e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4 *AlterionOpenProject-1.0.1.dmg
";

    #[test]
    fn the_platforms_line_is_found_by_the_end_of_its_name() {
        // By suffix, because the version sits in the middle of the name and
        // rebuilding that string here would drift from release.sh.
        let (name, digest) = line_for(MANIFEST, "-x86_64-linux.tar.gz").expect("a linux line");
        assert_eq!(name, "alterion-open-project-1.0.1-x86_64-linux.tar.gz");
        assert!(digest.starts_with("a1b2c3d4"));

        let (name, _) = line_for(MANIFEST, "-setup.exe").expect("a windows line");
        assert_eq!(name, "AlterionOpenProject-1.0.1-setup.exe");
    }

    #[test]
    fn binary_mode_puts_an_asterisk_before_the_name_and_it_is_not_part_of_it() {
        let (name, _) = line_for(MANIFEST, ".dmg").expect("a macos line");
        assert_eq!(name, "AlterionOpenProject-1.0.1.dmg");
    }

    #[test]
    fn a_platform_with_no_line_has_no_build_rather_than_a_guessed_address() {
        // The window this exists for: the release is out, that platform's
        // workflow has not been run yet or failed, and no file is attached.
        // Constructing a name and hoping would turn this into a 404.
        let partial = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90  \
                       alterion-open-project-1.0.1-x86_64-linux.tar.gz\n";
        assert!(line_for(partial, "-setup.exe").is_none());
        assert!(line_for(partial, ".dmg").is_none());
        assert!(line_for(partial, "-x86_64-linux.tar.gz").is_some());
    }

    #[test]
    fn a_line_whose_first_field_is_not_a_digest_is_not_a_digest_line() {
        assert!(line_for("not-a-digest  thing-setup.exe\n", "-setup.exe").is_none());
        assert!(line_for("# a comment about setup.exe\n", "-setup.exe").is_none());
        assert!(line_for("", "-setup.exe").is_none());
    }

    // ---- checking a download --------------------------------------------

    fn digest_of(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn a_checksum_mismatch_refuses_the_update() {
        // The whole reason the manifest is fetched: bytes that are not the
        // bytes it names must never be run.
        let want = digest_of(b"the real artefact");
        let refused = verify(b"something else entirely", &want);
        assert!(refused.is_err(), "a substituted download must be refused");
        assert!(
            refused.unwrap_err().contains("does not match the checksum"),
            "and it must say why"
        );

        assert!(verify(b"the real artefact", &want).is_ok());
    }

    #[test]
    fn a_truncated_download_fails_the_same_check() {
        let want = digest_of(b"a whole artefact, every byte of it");
        assert!(verify(b"a whole artefact", &want).is_err());
    }

    #[test]
    fn the_digest_is_compared_whatever_case_the_manifest_wrote_it_in() {
        let want = digest_of(b"artefact").to_ascii_uppercase();
        assert!(verify(b"artefact", &want).is_ok());
    }

    // ---- how it was installed -------------------------------------------

    #[test]
    fn a_package_managed_install_refuses_to_self_update_and_says_how_instead() {
        let pacman = Install::Packaged {
            name: "pacman",
            command: "sudo pacman -Syu",
        };
        assert!(!pacman.self_updates(), "pacman owns those files");
        let advice = pacman.advice();
        assert!(advice.contains("pacman -Syu"), "got {advice}");
        assert!(advice.contains("pacman"), "it has to name what installed it");
    }

    #[test]
    fn nothing_that_is_not_plainly_ours_replaces_itself() {
        // The default when a question cannot be answered is to leave the files
        // alone, so every one of these has to refuse.
        for install in [
            Install::Packaged {
                name: "pacman",
                command: "sudo pacman -Syu",
            },
            Install::SystemWide,
            Install::Bundle,
            Install::Unknown,
        ] {
            assert!(!install.self_updates(), "{install:?} must not overwrite itself");
            assert!(
                !install.advice().is_empty(),
                "{install:?} still has to say how to update"
            );
        }
    }

    #[test]
    fn a_system_wide_install_is_recognised_from_where_it_lives() {
        // The two routes: `install.sh --system` and the AUR package.
        for path in [
            "/usr/bin/alterion-open-project",
            "/usr/local/bin/alterion-open-project",
        ] {
            assert!(
                SYSTEM_PREFIXES.iter().any(|prefix| path.starts_with(prefix)),
                "{path} should read as a system install"
            );
        }
        assert!(
            !SYSTEM_PREFIXES
                .iter()
                .any(|prefix| "/home/somebody/.local/bin/alterion-open-project".starts_with(prefix))
        );
    }

    #[test]
    fn a_release_with_no_build_for_this_platform_says_so_rather_than_offering_one() {
        let found = Found {
            version: "1.0.1".into(),
            install: Install::SelfManaged,
            artefact: None,
            page: "https://example.test/releases/v1.0.1".into(),
        };
        assert!(!found.installable());
        let why = found.why_not().expect("a reason");
        assert!(why.contains("no build for this platform"), "got {why}");
    }

    #[test]
    fn a_package_managed_copy_is_told_how_even_when_a_build_exists() {
        let found = Found {
            version: "1.0.1".into(),
            install: Install::Packaged {
                name: "pacman",
                command: "sudo pacman -Syu",
            },
            artefact: Some(Artefact {
                name: "alterion-open-project-1.0.1-x86_64-linux.tar.gz".into(),
                digest: "0".repeat(64),
            }),
            page: "https://example.test/releases/v1.0.1".into(),
        };
        assert!(
            !found.installable(),
            "a build existing is not permission to overwrite pacman's files"
        );
        assert!(found.why_not().expect("a reason").contains("pacman -Syu"));
    }

    // ---- reading an archive ---------------------------------------------

    /// Build a one entry tar archive, the way `tar` lays one out.
    fn tar_with(name: &str, body: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:011o}\0", body.len());
        header[124..124 + size.len()].copy_from_slice(size.as_bytes());
        header[156] = b'0';

        let mut archive = header.to_vec();
        archive.extend_from_slice(body);
        archive.resize(512 + body.len().div_ceil(512) * 512, 0);
        // The two zero blocks that end an archive.
        archive.extend_from_slice(&[0u8; 1024]);
        archive
    }

    #[test]
    fn the_named_entry_comes_out_of_the_archive() {
        let archive = tar_with("alterion-open-project", b"ELF and then some");
        assert_eq!(
            entry_in_tar(&archive, "alterion-open-project"),
            Some(b"ELF and then some".to_vec())
        );
    }

    #[test]
    fn an_archive_without_the_binary_yields_nothing_rather_than_something_else() {
        let archive = tar_with("alterion-open-project.desktop", b"[Desktop Entry]");
        assert_eq!(entry_in_tar(&archive, "alterion-open-project"), None);
    }

    #[test]
    fn a_truncated_archive_is_refused_rather_than_read_past_its_end() {
        let mut archive = tar_with("alterion-open-project", b"a body that is cut off");
        archive.truncate(520);
        assert_eq!(entry_in_tar(&archive, "alterion-open-project"), None);
        assert_eq!(entry_in_tar(&[], "alterion-open-project"), None);
    }

    #[test]
    fn a_gzip_round_trip_comes_back_the_same() {
        use flate2::write::GzEncoder;
        use std::io::Write;

        let archive = tar_with("alterion-open-project", b"contents");
        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&archive).expect("compress");
        let compressed = encoder.finish().expect("finish");

        assert_eq!(decompress(&compressed).expect("decompress"), archive);
        assert!(decompress(b"not a gzip stream at all").is_err());
    }

    // ---- addresses ------------------------------------------------------

    #[test]
    fn every_address_asked_for_is_an_encrypted_one() {
        // HTTPS to a known host is the whole of the trust behind the manifest,
        // since it is not signed. Plain http would remove even that.
        for url in descriptor_urls(LATEST_NAME)
            .into_iter()
            .chain(artefact_urls("1.0.1", SUMS_NAME))
            .chain([release_page("1.0.1")])
        {
            assert!(url.starts_with("https://"), "{url}");
        }
    }

    #[test]
    fn the_origin_is_asked_before_the_mirror() {
        // GitLab is where releases are cut. GitHub is a mirror that may lag,
        // may not have a given release at all, and whose own "latest" address
        // skips exactly the pre-releases an updater matters most for.
        let [first, second] = descriptor_urls(LATEST_NAME);
        assert!(first.contains("gitlab.com"), "got {first}");
        assert!(second.contains("github.com"), "got {second}");

        let [first, second] = artefact_urls("1.0.1", "thing.tar.gz");
        assert!(first.contains("gitlab.com"), "got {first}");
        assert!(second.contains("github.com"), "got {second}");
    }

    #[test]
    fn the_descriptors_sit_in_a_slot_no_release_can_move() {
        // A literal string where a version goes. That is what makes the
        // address permanent, and it is why neither host's notion of "latest
        // release" is involved in finding it.
        let [gitlab, _] = descriptor_urls(LATEST_NAME);
        assert!(gitlab.ends_with("/packages/generic/alterion-open-project/latest/latest"));
        let [gitlab, _] = descriptor_urls(SUMS_NAME);
        assert!(gitlab.ends_with("/packages/generic/alterion-open-project/latest/SHA256SUMS"));
        assert!(
            !gitlab.contains("releases/permalink"),
            "the release permalink needs direct asset links the registry does not produce"
        );
    }

    #[test]
    fn an_artefact_address_is_only_ever_built_round_a_name_the_manifest_gave() {
        // The rule being kept: no filename is invented from a convention. The
        // name here came out of SHA256SUMS, which is what vouches for the file
        // existing at all.
        let (name, _) = line_for(MANIFEST, "-x86_64-linux.tar.gz").expect("a linux line");
        for url in artefact_urls("1.0.1", &name) {
            assert!(url.ends_with(&name), "{url}");
        }
    }

    /// Actually reaches the release hosts, so it only runs when asked for:
    /// `cargo test -p aop-app -- --ignored asks`.
    #[test]
    #[ignore = "reaches the network"]
    fn asks_the_real_hosts_what_they_have() {
        match check() {
            Ok(Some(found)) => eprintln!(
                "{} is out, install is {:?}, artefact {:?}",
                found.version, found.install, found.artefact
            ),
            Ok(None) => eprintln!("nothing newer than {RUNNING}"),
            Err(why) => eprintln!("could not ask: {why}"),
        }
    }
}
