//! Renames the helper processes the webview starts, so a process list names
//! the product rather than the toolkit.
//!
//! The window is ours, but the renderer is not: WebKitGTK forks
//! `/usr/lib/webkit2gtk-4.1/WebKitWebProcess`, and that is the entry a planner
//! sees in `btop` sitting on a couple of hundred megabytes with no clue which
//! application it belongs to. There is no setting for this. WebKitGTK offers
//! no override for the helper's name, and a process can only be renamed from
//! the inside, so the only way in is to be loaded into it.
//!
//! This library is put on `LD_PRELOAD` before the webview starts. Every child
//! it forks loads this object, and the constructor below runs before that
//! child's own `main`.
//!
//! Linux only, and deliberately so: this is a cosmetic nicety, not a feature
//! anything depends on. If the library is missing or the platform is not
//! Linux, the processes simply keep their own names.

#![cfg(target_os = "linux")]

use std::ffi::c_char;
use std::ffi::c_int;

/// What a process list should say.
const MASK: &[u8] = b"Alterion Open Project\0";

/// The kernel's `comm` field is `TASK_COMM_LEN` bytes including the
/// terminator, so fifteen characters survive and the rest is dropped. There is
/// no way around it: it is a fixed size array in the task struct.
const SHORT: &[u8] = b"Alterion Open P\0";

const PR_SET_NAME: c_int = 15;

unsafe extern "C" {
    fn prctl(option: c_int, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> c_int;
}

/// Runs before the host program's `main`.
///
/// glibc hands every `.init_array` entry the real `argc`, `argv` and `envp`,
/// which is what makes rewriting the command line possible at all.
///
/// # Safety
///
/// Called by the dynamic loader with the arguments glibc guarantees. The
/// rewrite stays inside the block the kernel already allocated for the command
/// line and never makes it longer, so nothing outside that block is touched.
unsafe extern "C" fn rename_me(argc: c_int, argv: *mut *mut c_char, _envp: *mut *mut c_char) {
    unsafe {
        // The short name, for anything reading /proc/<pid>/comm.
        prctl(PR_SET_NAME, SHORT.as_ptr() as usize, 0, 0, 0);

        // The command line, for anything reading /proc/<pid>/cmdline.
        if argc < 1 || argv.is_null() {
            return;
        }

        // How much room the original command line occupies. Writing past it
        // would run into the environment, so it is a hard ceiling.
        let first = *argv;
        if first.is_null() {
            return;
        }
        let last = *argv.offset((argc - 1) as isize);
        if last.is_null() {
            return;
        }
        let end = last.add(strlen(last)) as usize;
        let start = first as usize;
        if end <= start {
            return;
        }
        let room = end - start;

        // Collect the arguments before anything is overwritten, since the
        // rewrite below moves them.
        let mut kept: Vec<Vec<u8>> = Vec::new();
        for index in 1..argc {
            let arg = *argv.offset(index as isize);
            if arg.is_null() {
                continue;
            }
            let len = strlen(arg);
            let mut copy = vec![0u8; len + 1];
            std::ptr::copy_nonoverlapping(arg as *const u8, copy.as_mut_ptr(), len);
            kept.push(copy);
        }

        // Lay the new command line down from the start of the old one: the
        // mask, then each remaining argument, each terminated. Packing them
        // up against each other keeps the arguments contiguous, so a process
        // list shows "Alterion Open Project 4 20" rather than the mask
        // followed by a long gap where the old path used to be.
        let block = std::slice::from_raw_parts_mut(first as *mut u8, room);
        block.fill(0);

        let mut at = 0usize;
        let mask = &MASK[..MASK.len() - 1];
        let take = mask.len().min(room.saturating_sub(1));
        block[..take].copy_from_slice(&mask[..take]);
        at += take + 1;

        // The arguments have moved, so the pointers the host will read have to
        // move with them, or it would parse whatever used to be there.
        for (index, arg) in kept.iter().enumerate() {
            if at + arg.len() > room {
                break;
            }
            block[at..at + arg.len()].copy_from_slice(arg);
            *argv.offset((index + 1) as isize) = first.add(at);
            at += arg.len();
        }
    }
}

/// Length of a C string, without pulling in a dependency for one loop.
unsafe fn strlen(mut p: *const c_char) -> usize {
    unsafe {
        let mut n = 0;
        while *p != 0 {
            p = p.add(1);
            n += 1;
        }
        n
    }
}

/// Put the constructor where the loader looks for one.
#[used]
#[unsafe(link_section = ".init_array")]
static INIT: unsafe extern "C" fn(c_int, *mut *mut c_char, *mut *mut c_char) = rename_me;
