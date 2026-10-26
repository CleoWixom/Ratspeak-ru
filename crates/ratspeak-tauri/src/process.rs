//! What the app holds stays in it: no core dumps, and no panic report that
//! quotes its message. The process stays dumpable on Linux: the desktop
//! portals, which every file dialog goes through, read `/proc/<pid>/root`.

/// Call first in `run()`, before any thread, secret or WebView exists: the
/// WebView's processes inherit the core-dump limit.
pub fn harden() {
    #[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
    {
        let none = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: a plain syscall on this process; a failure leaves the default.
        unsafe {
            libc::setrlimit(libc::RLIMIT_CORE, &none);
        }
    }
    std::panic::set_hook(Box::new(|info| {
        // Never the message: it can quote whatever was at hand, a secret included.
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()));
        tracing::error!(
            location = location.as_deref().unwrap_or("unknown"),
            "Ratspeak panicked"
        );
    }));
}
