//! Diagnostic logging that can never take the process down.
//!
//! `eprintln!` panics when stderr cannot be written — e.g. the helper was
//! launched by `tauri dev` and that parent has since exited, leaving stderr
//! a broken pipe. A panic on the server thread skipped `process::exit`,
//! so the helper ignored SIGTERM and never idle-exited (the AppKit main
//! thread kept the process alive). Every helper log line goes through
//! `hlog!`, which drops write errors instead.

/// `eprintln!`-shaped logging that ignores write failures.
#[macro_export]
macro_rules! hlog {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::writeln!(::std::io::stderr().lock(), $($arg)*);
    }};
}
