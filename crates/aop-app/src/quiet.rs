//! Starting a program without a console window appearing.
//!
//! A graphical application on Windows that starts a console subsystem program
//! gets a console window for it, briefly or otherwise. `powershell`, `reg.exe`
//! and the hardware probes are all console programs, so listing printers,
//! reading a stored token or signing in each flashed a black window at
//! whoever was using the application. Nothing was wrong; it simply looked
//! like something was.
//!
//! Every `Command` in this application goes through here. It is a no-op
//! everywhere but Windows, so there is one thing to remember rather than a
//! rule to apply on one platform and forget on the others.

use std::process::Command;

/// Not attached to a console, and not given one.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Outlives the process that started it. Used by the updater, which starts an
/// installer that then replaces this very program.
#[cfg(windows)]
pub const DETACHED_PROCESS: u32 = 0x0000_0008;

pub trait Quiet {
    /// Run without showing a console window.
    fn quiet(&mut self) -> &mut Self;
}

impl Quiet for Command {
    #[cfg(windows)]
    fn quiet(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn quiet(&mut self) -> &mut Self {
        self
    }
}
