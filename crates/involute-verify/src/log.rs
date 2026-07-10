//! Progress logging for the CLI and library.
//!
//! Messages go to stderr and are flushed immediately so a wrapping service can
//! stream them. Toggle with [`set_verbose`].

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static VERBOSE: AtomicBool = AtomicBool::new(true);

/// Enable or disable progress messages on stderr.
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
}

/// Whether progress logging is currently enabled.
pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Print a progress line to stderr (no-op when quiet).
pub fn progress(msg: impl AsRef<str>) {
    if !verbose() {
        return;
    }
    let _ = writeln!(io::stderr(), "{}", msg.as_ref());
    let _ = io::stderr().flush();
}

/// Print a stepped progress line, e.g. `[3/7] …`.
pub fn step(n: u32, total: u32, msg: impl AsRef<str>) {
    progress(format!("[{n}/{total}] {}", msg.as_ref()));
}
