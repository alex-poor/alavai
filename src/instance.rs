//! Single-instance guard for the tray daemon.
//!
//! The tray is meant to be the one resident process. Launching alavai again —
//! clicking the app a second time, or a manual run while it autostarted at login
//! — should open a window, not plant a second tray icon. We enforce that with a
//! Unix-domain socket in `$XDG_RUNTIME_DIR`: binding it *is* the lock. The bound
//! listener is parked in a process-lifetime static so the kernel only releases
//! the lock when we exit.

use std::io::ErrorKind;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Holds the bound listener for the life of the process; dropping it would free
/// the lock, so we never do.
static GUARD: OnceLock<UnixListener> = OnceLock::new();

/// Outcome of trying to become the single tray instance.
pub enum Instance {
    /// We hold the lock — go ahead and run the tray.
    Primary,
    /// Another tray already holds the lock.
    AlreadyRunning,
}

/// Path of the lock socket: `$XDG_RUNTIME_DIR/alavai-tray.sock`, falling back to
/// the temp dir when the runtime dir isn't set.
fn sock_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("alavai-tray.sock")
}

/// Tries to acquire the single-instance lock.
///
/// On an unexpected bind error we fall back to [`Instance::Primary`]: better to
/// run an unguarded tray than to silently refuse to start.
pub fn acquire() -> Instance {
    let path = sock_path();
    match UnixListener::bind(&path) {
        Ok(listener) => {
            let _ = GUARD.set(listener);
            Instance::Primary
        }
        Err(e) if e.kind() == ErrorKind::AddrInUse => {
            // A socket file exists. If someone answers, a live tray holds it;
            // otherwise it's stale (a previous instance crashed) — reclaim it.
            if UnixStream::connect(&path).is_ok() {
                Instance::AlreadyRunning
            } else {
                let _ = std::fs::remove_file(&path);
                match UnixListener::bind(&path) {
                    Ok(listener) => {
                        let _ = GUARD.set(listener);
                        Instance::Primary
                    }
                    Err(_) => Instance::AlreadyRunning,
                }
            }
        }
        Err(_) => Instance::Primary,
    }
}

/// Removes the lock socket file so a replacement process (e.g. after a
/// restart-to-update `exec`) can bind cleanly without the stale-socket reclaim
/// dance. Best-effort; a leftover file is handled by [`acquire`] anyway.
pub fn release() {
    let _ = std::fs::remove_file(sock_path());
}
