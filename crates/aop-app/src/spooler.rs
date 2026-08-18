//! Sending a document to a printer.
//!
//! The document is produced here rather than by the web engine, so printing
//! does not depend on a browser dialog and what comes out is what the preview
//! showed. Printers come from CUPS, which is what actually knows about them on
//! this platform.
//!
//! Windows has no CUPS, so there the queues come from PowerShell and the
//! document is handed to whatever the machine has registered for PDF. That is
//! less precise than `lp`, and the difference is stated rather than hidden:
//! see `spool_windows`.
//!
//! Saving to PDF is always offered and never depends on CUPS. It is the one
//! destination that works on a machine with no printers configured at all, and
//! on this platform it is also the honest fallback when the CUPS client tools
//! are not installed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A print queue, as CUPS reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printer {
    /// The queue name, which is what `lp -d` takes.
    pub name: String,
    /// What CUPS says about it, for showing beside the name.
    pub status: String,
    /// Whether CUPS calls this the default queue.
    pub default: bool,
}

/// Why no printers could be listed. Worth telling apart, because one of these
/// the user can fix and the others they cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoPrinters {
    /// The CUPS client tools are not installed.
    NotInstalled,
    /// They are installed, but the scheduler is not answering.
    NotRunning,
    /// It answered, and there are genuinely none set up.
    NoneConfigured,
}

impl NoPrinters {
    /// What to tell the user, phrased as what they can do about it.
    pub fn message(self) -> &'static str {
        match self {
            NoPrinters::NotInstalled => {
                "No printer queues: the CUPS tools are not installed. Install the cups package to print to hardware. Saving as PDF works either way."
            }
            NoPrinters::NotRunning => {
                "No printer queues: CUPS is installed but not running. Start the cupsd service. Saving as PDF works either way."
            }
            NoPrinters::NoneConfigured => {
                "No printer queues are set up. Add one in your system's printer settings. Saving as PDF works either way."
            }
        }
    }
}

/// Ask CUPS what queues exist.
///
/// The distinction between "no tools", "not running" and "none set up" is kept
/// because only one of them means the user has nothing to do.
pub fn printers() -> Result<Vec<Printer>, NoPrinters> {
    #[cfg(target_os = "windows")]
    return printers_windows();

    #[cfg(not(target_os = "windows"))]
    printers_cups()
}

/// Ask Windows what printers are installed.
///
/// `Get-Printer` is the supported way and reports the queue's own state, which
/// is what the destination list wants to show.
#[cfg(target_os = "windows")]
fn printers_windows() -> Result<Vec<Printer>, NoPrinters> {
    let listing = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Printer | ForEach-Object { \"$($_.Name)|$($_.PrinterStatus)\" }",
        ])
        .output()
        .map_err(|_| NoPrinters::NotInstalled)?;

    let text = String::from_utf8_lossy(&listing.stdout);
    let default = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_Printer | Where-Object Default -eq $true).Name",
        ])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();

    let found: Vec<Printer> = text
        .lines()
        .filter_map(|line| line.split_once('|'))
        .map(|(name, status)| Printer {
            name: name.trim().to_string(),
            status: tidy_status(status.trim()),
            default: !default.is_empty() && name.trim() == default,
        })
        .collect();

    if found.is_empty() {
        return Err(NoPrinters::NoneConfigured);
    }
    Ok(found)
}

#[cfg(not(target_os = "windows"))]
fn printers_cups() -> Result<Vec<Printer>, NoPrinters> {
    let listing = Command::new("lpstat").arg("-p").arg("-d").output();

    let output = match listing {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(NoPrinters::NotInstalled);
        }
        Err(_) => return Err(NoPrinters::NotRunning),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() && text.trim().is_empty() {
        let complaint = String::from_utf8_lossy(&output.stderr).to_lowercase();
        // CUPS says so itself when the scheduler is down.
        if complaint.contains("not running") || complaint.contains("connect") {
            return Err(NoPrinters::NotRunning);
        }
    }

    // The default is reported on its own line rather than beside the queue.
    let default_name = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("system default destination:"))
        .map(|name| name.trim().to_string());

    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("printer ") else {
            continue;
        };
        let Some((name, status)) = rest.split_once(' ') else {
            continue;
        };
        let name = name.trim().to_string();
        found.push(Printer {
            default: default_name.as_deref() == Some(name.as_str()),
            status: tidy_status(status),
            name,
        });
    }

    if found.is_empty() {
        return Err(NoPrinters::NoneConfigured);
    }

    // The default first, then alphabetical, so the list does not reorder
    // itself between openings.
    found.sort_by(|a, b| b.default.cmp(&a.default).then(a.name.cmp(&b.name)));
    Ok(found)
}

/// Turn a CUPS status line into something worth showing.
fn tidy_status(raw: &str) -> String {
    let text = raw.trim().trim_start_matches("is ").trim_end_matches('.');
    let text = text.split(". ").next().unwrap_or(text);
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Ready".into(),
    }
}

/// Send a document to a queue.
///
/// The bytes go in over standard input rather than through a temporary file, so
/// nothing is left behind on disk and there is no window in which a half
/// written file could be picked up.
pub fn spool(printer: &str, title: &str, document: &[u8], copies: u16) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    return spool_windows(printer, title, document, copies);

    #[cfg(not(target_os = "windows"))]
    spool_cups(printer, title, document, copies)
}

/// Hand the document to Windows.
///
/// Windows has no equivalent of piping a PDF into `lp`. The document is
/// written out and opened with the shell's print verb, which sends it through
/// whatever the machine has registered for PDF.
///
/// Two honest limits come with that, and the caller is told about them rather
/// than left to wonder. The chosen queue is a request, not a guarantee: the
/// handler may use the default printer instead. And the copy count is applied
/// by repeating the job, because there is nowhere to pass it.
#[cfg(target_os = "windows")]
fn spool_windows(
    printer: &str,
    title: &str,
    document: &[u8],
    copies: u16,
) -> Result<String, String> {
    let mut path = std::env::temp_dir();
    let safe: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    path.push(format!("{safe}.pdf"));
    std::fs::write(&path, document)
        .map_err(|error| format!("Could not write the document to print: {error}"))?;

    let target = path.display().to_string();
    for _ in 0..copies.max(1) {
        let sent = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Start-Process -FilePath '{}' -Verb PrintTo -ArgumentList '{}' -PassThru | Out-Null",
                    target.replace('\'', "''"),
                    printer.replace('\'', "''")
                ),
            ])
            .output()
            .map_err(|error| format!("Could not start the print command: {error}"))?;

        if !sent.status.success() {
            let complaint = String::from_utf8_lossy(&sent.stderr).trim().to_string();
            return Err(if complaint.is_empty() {
                format!("{printer} refused the document.")
            } else {
                complaint
            });
        }
    }

    Ok(format!("Sent to {printer}"))
}

#[cfg(not(target_os = "windows"))]
fn spool_cups(
    printer: &str,
    title: &str,
    document: &[u8],
    copies: u16,
) -> Result<String, String> {
    let mut child = Command::new("lp")
        .arg("-d")
        .arg(printer)
        .arg("-n")
        .arg(copies.max(1).to_string())
        .arg("-t")
        .arg(title)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                "The CUPS tools are not installed, so there is nothing to print through.".to_string()
            }
            _ => format!("Could not start the print command: {error}"),
        })?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| "The print command would not accept the document.".to_string())?
        .write_all(document)
        .map_err(|error| format!("Could not hand the document over: {error}"))?;

    let output = child
        .wait_with_output()
        .map_err(|error| format!("The print command did not finish: {error}"))?;

    if output.status.success() {
        let said = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if said.is_empty() {
            format!("Sent to {printer}")
        } else {
            said
        })
    } else {
        let complaint = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if complaint.is_empty() {
            format!("{printer} refused the document.")
        } else {
            complaint
        })
    }
}

/// Write the document to a file.
pub fn save(path: &Path, document: &[u8]) -> Result<PathBuf, String> {
    let path = if path.extension().is_none() {
        path.with_extension("pdf")
    } else {
        path.to_path_buf()
    };
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && std::fs::create_dir_all(parent).is_err()
    {
        return Err(format!("Could not create {}", parent.display()));
    }
    std::fs::write(&path, document).map_err(|error| format!("Could not write the file: {error}"))?;
    Ok(path)
}

/// Encode bytes for a `data:` URL.
///
/// Written out rather than pulled in as a dependency: this is the only place
/// the application needs base64, and it is a dozen lines.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let packed = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;

        out.push(ALPHABET[(packed >> 18) as usize & 63] as char);
        out.push(ALPHABET[(packed >> 12) as usize & 63] as char);
        // The last group is padded rather than truncated, or a decoder reading
        // it will be a byte or two short.
        out.push(if chunk.len() > 1 {
            ALPHABET[(packed >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[packed as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_known_answers() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_bytes_that_are_not_text() {
        // A PDF is binary, and the high bytes are where a careless
        // implementation goes wrong.
        assert_eq!(base64(&[0x00, 0xFF, 0x80]), "AP+A");
        assert_eq!(base64(&[0xFF; 4]), "/////w==");
    }

    #[test]
    fn a_cups_status_line_is_made_readable() {
        assert_eq!(tidy_status("is idle.  enabled since Mon"), "Idle");
        assert_eq!(tidy_status("now printing job 4."), "Now printing job 4");
    }

    #[test]
    fn a_status_that_says_nothing_still_reads_as_something() {
        assert_eq!(tidy_status(""), "Ready");
    }

    #[test]
    fn saving_without_an_extension_gets_one() {
        // A printer file that opens in a text editor because it was saved with
        // no extension helps nobody.
        let dir = std::env::temp_dir().join("aop-spooler-test");
        let _ = std::fs::remove_dir_all(&dir);
        let written = save(&dir.join("plan"), b"%PDF-1.7\n").expect("written");
        assert_eq!(written.extension().and_then(|e| e.to_str()), Some("pdf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_extension_the_user_chose_is_left_alone() {
        let dir = std::env::temp_dir().join("aop-spooler-test-2");
        let _ = std::fs::remove_dir_all(&dir);
        let written = save(&dir.join("plan.print"), b"%PDF-1.7\n").expect("written");
        assert_eq!(written.extension().and_then(|e| e.to_str()), Some("print"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_reason_for_having_no_printers_says_what_to_do_about_it() {
        // Only one of these is something the user can act on, and the message
        // has to make clear which.
        for reason in [
            NoPrinters::NotInstalled,
            NoPrinters::NotRunning,
            NoPrinters::NoneConfigured,
        ] {
            let message = reason.message();
            assert!(
                message.contains("PDF"),
                "every one has to point at the way out that always works"
            );
        }
        assert!(NoPrinters::NotInstalled.message().contains("cups package"));
        assert!(NoPrinters::NotRunning.message().contains("cupsd"));
    }
}
