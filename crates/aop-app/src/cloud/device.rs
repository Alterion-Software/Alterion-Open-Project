//! What machine this is, from the machine itself.
//!
//! The sign in server binds a session to the device it was issued to, so a
//! stolen token is worth nothing anywhere else. The server half of that already
//! exists in `alterion-auth`: it hashes what the client sends with a per session
//! salt, compares in constant time, and refuses outright when the anchor is
//! empty. Its component shape was designed for a web client, which is why the
//! fields are named after things a browser can see.
//!
//! A desktop application is in a much better position than a browser. It can
//! read the actual hardware rather than infer it from a canvas, so that is what
//! goes in each field. The shape is kept exactly as the server expects it; only
//! the source of each value is better.
//!
//! ```text
//!   field         tier          Linux                 macOS                Windows
//!   ------------  ------------  --------------------  -------------------  ---------------------
//!   anchor        exact, fatal  /etc/machine-id       IOPlatformUUID       MachineGuid
//!   webgl         exact         DMI board vendor/name hw.model             baseboard + system UUID
//!   platform      exact         os + architecture     os + architecture    os + architecture
//!   cpu           exact         /proc/cpuinfo         machdep.cpu.brand    Win32_Processor Name
//!   screen        drifts        host name             host name            host name
//!   pixel_ratio   drifts        DMI ids if readable   (empty)              (empty)
//! ```
//!
//! Two tiers, and the split matters. The exact tier is what the sealed token
//! blob's key is derived from and what the server compares: a different machine
//! must not match. The drifting tier is everything a person can change without
//! becoming a different person, so renaming a laptop or having a value become
//! readable that was not before does not sign anybody out.
//!
//! Everything listed is readable by an ordinary user. Nothing here asks for
//! administrator rights, and the sources that need them (`product_uuid` and
//! `board_serial` on most distributions) are read only if they happen to be
//! readable and sit in the drifting tier for exactly that reason.
//!
//! Nothing in here is ever printed. A fingerprint identifies a machine, and a
//! component of one in a log is a component of one in a log.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The shape `alterion-auth` stores and compares, field for field.
///
/// The names are the server's and are not changed here: the point is that the
/// existing server side validates this without a line of difference. What each
/// one carries on a desktop machine is set out in the table above.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceComponents {
    /// The stable per install identifier. Never empty: an empty anchor is what
    /// the server calls private mode and refuses, so it is a hard failure here
    /// rather than something quietly sent as a blank.
    pub anchor: String,
    /// Board and firmware identity. Named for a browser's renderer string
    /// because that is the field the server compares exactly.
    pub webgl: String,
    pub platform: String,
    pub cpu: String,
    /// Drifts: the host name, which people change.
    pub screen: String,
    /// Drifts: identifiers that are only readable on some machines.
    pub pixel_ratio: String,
}

/// Kept out of the printed form entirely. A fingerprint is not a secret in the
/// way a token is, but it identifies a machine, and it has no business in a log
/// or a crash report.
impl std::fmt::Debug for DeviceComponents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceComponents")
            .field("anchor", &"(withheld)")
            .field("webgl", &"(withheld)")
            .field("platform", &"(withheld)")
            .field("cpu", &"(withheld)")
            .field("screen", &"(withheld)")
            .field("pixel_ratio", &"(withheld)")
            .finish()
    }
}

/// Why the machine could not be identified.
///
/// One case, because there is only one thing that is fatal: the anchor. Carries
/// what was looked at, so the message names a file the user can go and check
/// rather than saying that something went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoAnchor {
    /// Where the anchor was looked for, in the order it was looked for.
    pub looked_at: Vec<String>,
}

impl std::fmt::Display for NoAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "This machine could not be identified, so signing in has stopped rather than \
             carry on without the protection that identity provides. \
             Nothing could be read from: {}.",
            self.looked_at.join(", ")
        )
    }
}

impl DeviceComponents {
    /// The parts that must match exactly, in a fixed order.
    ///
    /// Everything that hashes or derives a key goes through here, so the two
    /// can never disagree about what counts as the same machine. The separator
    /// is a byte none of the sources contains, so two different sets of
    /// components cannot run together into one identical string.
    fn exact_tier(&self) -> String {
        format!(
            "anchor={}\u{1f}webgl={}\u{1f}platform={}\u{1f}cpu={}",
            self.anchor, self.webgl, self.platform, self.cpu
        )
    }

    /// The material a token blob's key is derived from.
    ///
    /// The exact tier and nothing else. Deriving from a value that is allowed
    /// to drift would mean a renamed laptop could no longer open its own stored
    /// tokens, which is a lockout dressed up as security.
    pub fn key_material(&self) -> Vec<u8> {
        self.exact_tier().into_bytes()
    }

    /// What goes in the `X-Device-Fingerprint` header, which the server expects
    /// as hex.
    ///
    /// A digest rather than the components themselves: the server only needs
    /// something that is the same on the same machine and different on another,
    /// and there is no reason to put a hardware inventory on the wire.
    pub fn fingerprint_hex(&self) -> String {
        use sha2::{Digest, Sha256};
        crate::cloud::tokens::to_hex(&Sha256::digest(self.exact_tier().as_bytes()))
    }
}

/// This machine, worked out once and remembered.
///
/// Some of the sources are subprocesses, and on Windows one of them is
/// PowerShell, which is not fast. It is also pointless to ask twice: the answer
/// cannot change while the process is running, and it must not, or a token
/// sealed at start up would not open at the end of the session.
pub fn components() -> Result<&'static DeviceComponents, &'static NoAnchor> {
    static RESOLVED: OnceLock<Result<DeviceComponents, NoAnchor>> = OnceLock::new();
    RESOLVED.get_or_init(collect).as_ref()
}

/// The fingerprint header value for this machine.
pub fn fingerprint_hex() -> Result<String, &'static NoAnchor> {
    components().map(DeviceComponents::fingerprint_hex)
}

/// Trim a value read from a file or a command, and treat whitespace as absent.
fn tidy(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Read a small file, if it is there and readable.
///
/// Absence is normal for several of these, so it is not distinguished from a
/// permission failure: the caller treats both as "this machine does not offer
/// that one".
#[cfg(any(target_os = "linux", test))]
fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().and_then(|text| tidy(&text))
}

/// Run a command and take its output, if it runs at all.
#[cfg(not(target_os = "linux"))]
fn run(program: &str, arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    tidy(&String::from_utf8_lossy(&output.stdout))
}

/// The operating system and the instruction set, which no machine changes
/// without becoming a different one.
fn platform() -> String {
    format!("{}|{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The host name, which is in the drifting tier because people rename machines.
fn host_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .and_then(|name| tidy(&name))
        .or_else(|| {
            #[cfg(target_os = "linux")]
            {
                read_file("/etc/hostname")
            }
            #[cfg(not(target_os = "linux"))]
            {
                run("hostname", &[])
            }
        })
        .unwrap_or_default()
}

// ------------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
fn collect() -> Result<DeviceComponents, NoAnchor> {
    // systemd writes the first, dbus the second, and a machine with neither is
    // one that cannot be told apart from any other.
    const ANCHORS: [&str; 2] = ["/etc/machine-id", "/var/lib/dbus/machine-id"];

    let anchor = ANCHORS.iter().find_map(|path| read_file(path));
    let Some(anchor) = anchor else {
        return Err(NoAnchor {
            looked_at: ANCHORS.iter().map(|path| path.to_string()).collect(),
        });
    };

    // World readable on every distribution that populates DMI at all, which is
    // why these and not `product_uuid`, which is root only nearly everywhere.
    let board = ["board_vendor", "board_name", "sys_vendor", "product_name"]
        .iter()
        .map(|name| read_file(&format!("/sys/class/dmi/id/{name}")).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("|");

    // The privileged ones. Readable by root, and on a few machines by anyone,
    // so they are taken when they are offered and sit where a change of
    // readability cannot lock anybody out.
    let privileged = ["product_uuid", "board_serial"]
        .iter()
        .map(|name| read_file(&format!("/sys/class/dmi/id/{name}")).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("|");

    Ok(DeviceComponents {
        anchor,
        webgl: board,
        platform: platform(),
        cpu: cpu_from_proc(),
        screen: host_name(),
        pixel_ratio: privileged,
    })
}

/// The processor model and how many of it there are.
#[cfg(target_os = "linux")]
fn cpu_from_proc() -> String {
    let Some(text) = read_file("/proc/cpuinfo") else {
        return String::new();
    };

    let model = text
        .lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split_once(':'))
        .and_then(|(_, value)| tidy(value))
        .unwrap_or_default();
    let cores = text.lines().filter(|line| line.starts_with("processor")).count();

    format!("{model}|{cores}")
}

// ------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn collect() -> Result<DeviceComponents, NoAnchor> {
    // The platform UUID, which is what every Apple tool means by the identity
    // of a Mac, and which any user can read.
    let anchor = run("ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"]).and_then(|text| {
        text.lines()
            .find(|line| line.contains("IOPlatformUUID"))
            .and_then(|line| line.split('=').nth(1))
            .map(|value| value.trim().trim_matches('"').to_string())
            .and_then(|value| tidy(&value))
    });

    let Some(anchor) = anchor else {
        return Err(NoAnchor {
            looked_at: vec!["ioreg -rd1 -c IOPlatformExpertDevice (IOPlatformUUID)".into()],
        });
    };

    Ok(DeviceComponents {
        anchor,
        webgl: run("sysctl", &["-n", "hw.model"]).unwrap_or_default(),
        platform: platform(),
        cpu: format!(
            "{}|{}",
            run("sysctl", &["-n", "machdep.cpu.brand_string"]).unwrap_or_default(),
            run("sysctl", &["-n", "hw.physicalcpu"]).unwrap_or_default(),
        ),
        screen: host_name(),
        pixel_ratio: String::new(),
    })
}

// ----------------------------------------------------------------- Windows

#[cfg(target_os = "windows")]
fn collect() -> Result<DeviceComponents, NoAnchor> {
    // Written when Windows was installed and readable by any user. The 64 bit
    // view is asked for explicitly so a 32 bit build is not quietly sent to the
    // WOW6432Node copy, which holds a different value.
    let anchor = run(
        "reg.exe",
        &[
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
            "/reg:64",
        ],
    )
    .and_then(|text| {
        text.lines()
            .find(|line| line.contains("MachineGuid"))
            .and_then(|line| line.split_whitespace().last())
            .and_then(tidy)
    });

    let Some(anchor) = anchor else {
        return Err(NoAnchor {
            looked_at: vec![r"HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid".into()],
        });
    };

    // One PowerShell start up rather than three: it is the slowest thing in
    // this module by a wide margin, and this runs once per session.
    let hardware = run(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$p=Get-CimInstance Win32_ComputerSystemProduct;\
             $b=Get-CimInstance Win32_BaseBoard;\
             $c=Get-CimInstance Win32_Processor | Select-Object -First 1;\
             \"$($b.Manufacturer)|$($b.Product)|$($p.UUID)\";\
             \"$($c.Name)|$($c.NumberOfCores)\"",
        ],
    )
    .unwrap_or_default();

    let mut lines = hardware.lines();
    let board = lines.next().unwrap_or_default().trim().to_string();
    let cpu = lines.next().unwrap_or_default().trim().to_string();

    Ok(DeviceComponents {
        anchor,
        webgl: board,
        platform: platform(),
        cpu,
        screen: host_name(),
        pixel_ratio: String::new(),
    })
}

// ------------------------------------------------------------------- other

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn collect() -> Result<DeviceComponents, NoAnchor> {
    // No known way to identify the machine, and inventing one would be worse
    // than saying so: a value made up per run makes every start look like a new
    // device, and an empty one makes every machine look like the same device.
    Err(NoAnchor {
        looked_at: vec![format!("no known identifier on {}", std::env::consts::OS)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DeviceComponents {
        DeviceComponents {
            anchor: "9f2c1e7a4b8d4c3e9a1f0b7d6c5e4a3b".into(),
            webgl: "Acme|X570|Acme Inc|Desktop".into(),
            platform: "linux|x86_64".into(),
            cpu: "Acme Ryzen 9 5950X 16-Core Processor|32".into(),
            screen: "workstation".into(),
            pixel_ratio: String::new(),
        }
    }

    // ------------------------------------------------------------ stability

    #[test]
    fn this_machine_can_identify_itself() {
        // Every other test here is about what happens once it can, so if this
        // fails on a build machine the rest say nothing.
        let outcome = components();
        assert!(
            outcome.is_ok(),
            "no anchor on this machine: {:?}",
            outcome.err()
        );
        assert!(!outcome.expect("components").anchor.is_empty());
    }

    #[test]
    fn the_fingerprint_is_the_same_twice_running() {
        // If it is not, every launch looks like a new device, the refresh is
        // refused, and the user is signed out on every start with nothing to
        // explain why.
        let first = fingerprint_hex().expect("an anchor");
        let second = fingerprint_hex().expect("an anchor");
        assert_eq!(first, second);
    }

    #[test]
    fn the_fingerprint_is_the_same_in_a_fresh_process() {
        // The one above shares a cache, so it would pass even if the sources
        // were unstable. This re-runs the collection from nothing, which is
        // what a second launch of the application does.
        let once = collect().expect("an anchor").fingerprint_hex();
        let again = collect().expect("an anchor").fingerprint_hex();
        assert_eq!(once, again, "the sources themselves have to be stable");
        assert_eq!(once, fingerprint_hex().expect("an anchor"));
    }

    #[test]
    fn the_fingerprint_is_hex_the_server_can_decode() {
        // The server hex decodes the header and rejects anything that will not.
        let hex = fingerprint_hex().expect("an anchor");
        assert_eq!(hex.len(), 64, "a SHA-256 digest");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    // ----------------------------------------------------------------- tiers

    #[test]
    fn the_key_material_is_the_exact_tier_and_only_that() {
        let mut drifted = sample();
        drifted.screen = "renamed-laptop".into();
        drifted.pixel_ratio = "a value that became readable".into();
        assert_eq!(sample().key_material(), drifted.key_material());
        assert_eq!(sample().fingerprint_hex(), drifted.fingerprint_hex());
    }

    #[test]
    fn a_different_machine_is_a_different_fingerprint() {
        for change in [
            |c: &mut DeviceComponents| c.anchor = "another install".into(),
            |c: &mut DeviceComponents| c.webgl = "another board".into(),
            |c: &mut DeviceComponents| c.platform = "windows|x86_64".into(),
            |c: &mut DeviceComponents| c.cpu = "another processor|8".into(),
        ] {
            let mut other = sample();
            change(&mut other);
            assert_ne!(sample().fingerprint_hex(), other.fingerprint_hex());
            assert_ne!(sample().key_material(), other.key_material());
        }
    }

    #[test]
    fn two_component_sets_cannot_run_together_into_one() {
        // Without a separator the fields would concatenate, and one machine's
        // board could end up indistinguishable from another's anchor.
        let mut shifted = sample();
        shifted.anchor = format!("{}{}", sample().anchor, sample().webgl);
        shifted.webgl = String::new();
        assert_ne!(sample().key_material(), shifted.key_material());
    }

    // -------------------------------------------------------------- absence

    #[test]
    fn a_missing_anchor_names_what_could_not_be_read() {
        // The user should be able to go and look at the thing that is missing.
        let failure = NoAnchor {
            looked_at: vec!["/etc/machine-id".into(), "/var/lib/dbus/machine-id".into()],
        };
        let message = failure.to_string();
        assert!(message.contains("/etc/machine-id"), "got {message}");
        assert!(message.contains("/var/lib/dbus/machine-id"), "got {message}");
        assert!(message.len() > 60, "too terse to act on: {message}");
    }

    #[test]
    fn a_source_that_is_not_there_is_normal_rather_than_fatal() {
        // Only the anchor is fatal. A machine with no DMI, or with the
        // privileged identifiers locked away, still signs in.
        assert!(read_file("/nonexistent/dmi/id/product_uuid").is_none());
        assert!(read_file("/proc/self/environ/not-a-file").is_none());

        let mut sparse = sample();
        sparse.webgl = String::new();
        sparse.pixel_ratio = String::new();
        assert!(!sparse.fingerprint_hex().is_empty());
    }

    #[test]
    fn whitespace_is_absence_rather_than_a_value() {
        assert_eq!(tidy("  \n "), None);
        assert_eq!(tidy(" abc \n"), Some("abc".to_string()));
    }

    // ------------------------------------------------------------- privacy

    #[test]
    fn printing_the_components_does_not_print_the_machine() {
        let printed = format!("{:?}", sample());
        assert!(!printed.contains("9f2c1e7a"), "got {printed}");
        assert!(!printed.contains("X570"), "got {printed}");
        assert!(!printed.contains("workstation"), "got {printed}");
    }

    #[test]
    fn the_platform_is_the_one_this_was_built_for() {
        let platform = platform();
        assert!(platform.contains(std::env::consts::OS));
        assert!(platform.contains('|'));
    }
}
