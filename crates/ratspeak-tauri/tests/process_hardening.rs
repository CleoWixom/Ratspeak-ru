//! The app hardens its process first: no core dumps, on Linux no same-user
//! memory reads, and a panic never reports its message. Runs itself as a
//! child for the panic, so the hook it installs never touches another test.

const CHILD: &str = "RATSPEAK_HARDENING_CHILD";
/// The child reports a failed check by exit code: the hook under test hides
/// panic messages, its own included.
#[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
const CORE_DUMPS_ON: i32 = 3;
#[cfg(target_os = "linux")]
const DUMPABLE: i32 = 4;

fn main() {
    if std::env::var_os(CHILD).is_some() {
        ratspeak_tauri::process::harden();
        #[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
        {
            let mut limit = libc::rlimit {
                rlim_cur: 1,
                rlim_max: 1,
            };
            // SAFETY: reads this process's own limit.
            unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) };
            if (limit.rlim_cur, limit.rlim_max) != (0, 0) {
                std::process::exit(CORE_DUMPS_ON);
            }
            #[cfg(target_os = "linux")]
            // SAFETY: reads this process's own flag.
            if unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } != 0 {
                std::process::exit(DUMPABLE);
            }
        }
        panic!("passcode 314159 must never be printed");
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&child.stderr);
    #[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
    assert_ne!(
        child.status.code(),
        Some(CORE_DUMPS_ON),
        "core dumps stay on"
    );
    #[cfg(target_os = "linux")]
    assert_ne!(
        child.status.code(),
        Some(DUMPABLE),
        "the process stays dumpable"
    );
    assert_eq!(
        child.status.code(),
        Some(101),
        "the child must have panicked: {stderr}"
    );
    assert!(
        !stderr.contains("314159"),
        "a panic printed its message: {stderr}"
    );
}
