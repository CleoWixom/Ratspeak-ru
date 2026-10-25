//! One owner per profile. Storage init takes an exclusive lock on
//! `<data root>/.ratspeak-host.lock` and the process keeps it until exit, so a
//! second app, host or CLI can never reconcile or open the same profile
//! concurrently.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rns_identity::persistence::try_lock_exclusive;

// In the root, not `.ratspeak/`: Windows storage init may republish
// `.ratspeak` by renaming it, which an open handle inside would block. Every
// app that opens these profiles locks this same file.
const LOCK_FILE: &str = ".ratspeak-host.lock";

/// Another process owns the profile.
#[derive(Debug, thiserror::Error)]
#[error("another process already owns this profile")]
pub struct ProfileInUse;

#[derive(Default)]
struct Held {
    paths: HashSet<PathBuf>,
    // Profiles on a filesystem that cannot lock, opened without exclusion.
    unenforced: HashSet<PathBuf>,
    // Closing a handle releases its lock; keep them for the process lifetime.
    files: Vec<File>,
}

fn held() -> &'static Mutex<Held> {
    static HELD: OnceLock<Mutex<Held>> = OnceLock::new();
    HELD.get_or_init(Mutex::default)
}

/// Acquire the owner lock for an existing data root: `ResourceBusy` carrying
/// [`ProfileInUse`] while another process owns it. Re-entrant within the
/// owning process, because a second handle in the same process would contend
/// with the first. A filesystem that cannot lock is accepted; [`unenforced`]
/// then reports it.
pub(crate) fn acquire_owner_lock(data_root: &Path) -> io::Result<()> {
    let path = data_root.canonicalize()?.join(LOCK_FILE);
    let mut held = held().lock().unwrap_or_else(|e| e.into_inner());
    if held.paths.contains(&path) || held.unenforced.contains(&path) {
        return Ok(());
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(&path)?;
    if let Err(error) = try_lock_exclusive(&file) {
        return match classify(&error) {
            LockFailure::Busy => Err(io::Error::new(io::ErrorKind::ResourceBusy, ProfileInUse)),
            LockFailure::Unsupported => {
                tracing::warn!(
                    reason = "profile_lock_unsupported",
                    "profile filesystem cannot lock; another owner is not excluded"
                );
                held.unenforced.insert(path);
                Ok(())
            }
            LockFailure::Failed => Err(error),
        };
    }
    held.paths.insert(path);
    held.files.push(file);
    Ok(())
}

/// Whether this profile's filesystem could not take the owner lock, so
/// another app may open the profile at the same time.
pub fn unenforced(data_root: &Path) -> bool {
    let Ok(root) = data_root.canonicalize() else {
        return false;
    };
    let held = held().lock().unwrap_or_else(|e| e.into_inner());
    held.unenforced.contains(&root.join(LOCK_FILE))
}

#[derive(Debug, PartialEq, Eq)]
enum LockFailure {
    Busy,
    Unsupported,
    Failed,
}

fn classify(error: &io::Error) -> LockFailure {
    // Network and FUSE filesystems without lock support. ENOTSUP is distinct
    // from EOPNOTSUPP on Apple platforms, where flock reports it.
    #[cfg(unix)]
    const UNSUPPORTED: &[i32] = &[libc::ENOLCK, libc::EOPNOTSUPP, libc::ENOTSUP, libc::ENOSYS];
    // ERROR_INVALID_FUNCTION and ERROR_NOT_SUPPORTED.
    #[cfg(windows)]
    const UNSUPPORTED: &[i32] = &[1, 50];
    if error.kind() == io::ErrorKind::WouldBlock {
        LockFailure::Busy
    } else if error
        .raw_os_error()
        .is_some_and(|code| UNSUPPORTED.contains(&code))
    {
        LockFailure::Unsupported
    } else {
        LockFailure::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_namespace_owner_lock_is_reentrant_and_held_for_the_process() {
        let root = tempfile::tempdir().unwrap();
        acquire_owner_lock(root.path()).unwrap();
        acquire_owner_lock(root.path()).unwrap();

        // An independent handle stands in for another process.
        let other = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.path().join(LOCK_FILE))
            .unwrap();
        let error = try_lock_exclusive(&other).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(!unenforced(root.path()));
    }

    #[test]
    fn storage_namespace_lock_failures_warn_refuse_or_fail() {
        #[cfg(unix)]
        let (unsupported, failed) = (
            [libc::ENOLCK, libc::EOPNOTSUPP, libc::ENOTSUP, libc::ENOSYS],
            [libc::EACCES, libc::EIO, libc::EBADF],
        );
        // ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED; ERROR_ACCESS_DENIED,
        // ERROR_INVALID_HANDLE, ERROR_SHARING_VIOLATION.
        #[cfg(windows)]
        let (unsupported, failed) = ([1, 50], [5, 6, 32]);
        for code in unsupported {
            let error = io::Error::from_raw_os_error(code);
            assert_eq!(classify(&error), LockFailure::Unsupported, "{error}");
        }
        for code in failed {
            let error = io::Error::from_raw_os_error(code);
            assert_eq!(classify(&error), LockFailure::Failed, "{error}");
        }
        let held = io::Error::new(io::ErrorKind::WouldBlock, "file is locked by another owner");
        assert_eq!(classify(&held), LockFailure::Busy);
        // Android's flock reports contention as the raw errno.
        #[cfg(unix)]
        assert_eq!(
            classify(&io::Error::from_raw_os_error(libc::EWOULDBLOCK)),
            LockFailure::Busy
        );
    }
}
