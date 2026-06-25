//! Launch-on-login via the XDG autostart spec.
//!
//! Enabling drops a `.desktop` file in `~/.config/autostart/`; the desktop
//! environment then starts the tray daemon at session login. Disabling removes
//! it. This is the per-user toggle an in-app preference (feature I3) or the
//! `alavai autostart` CLI flips — distinct from the system-wide
//! `dist/alavai-tray.desktop` a packager may install into `/etc/xdg/autostart`.
//!
//! No external crates: XDG paths are resolved directly to keep the dependency
//! tree small.

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

/// Basename of the autostart entry we own. Matches the packaged tray entry so
/// the two don't both fire if a distro also installs the system-wide copy.
const ENTRY: &str = "alavai-tray.desktop";

/// `$XDG_CONFIG_HOME/autostart`, falling back to `~/.config/autostart`.
fn autostart_dir() -> Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .ok_or_else(|| anyhow!("neither XDG_CONFIG_HOME nor HOME is set"))?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("autostart"))
}

/// Full path to our autostart entry.
fn entry_path() -> Result<PathBuf> {
    Ok(autostart_dir()?.join(ENTRY))
}

/// Does a system-wide autostart entry exist (packaged into `/etc/xdg/autostart`
/// or another `$XDG_CONFIG_DIRS` dir)? If so, deleting the user entry wouldn't
/// disable it — per XDG, a same-named user file wins, so we mask with
/// `Hidden=true` instead, and treat "no user entry" as enabled.
fn system_entry_exists() -> bool {
    let dirs = env::var_os("XDG_CONFIG_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/etc/xdg".into());
    env::split_paths(&dirs).any(|d| d.join("autostart").join(ENTRY).is_file())
}

/// The command the entry should launch. Prefer the absolute path of the running
/// binary so it works for non-`PATH` and dev builds; fall back to bare `alavai`.
fn exec_command() -> String {
    match env::current_exe() {
        Ok(p) => format!("{} tray", p.display()),
        Err(_) => "alavai tray".into(),
    }
}

/// Is launch-on-login currently enabled?
///
/// True only when the entry exists and isn't explicitly disabled via the
/// `Hidden=true` / `X-GNOME-Autostart-enabled=false` keys some tools write
/// instead of deleting the file.
pub fn is_enabled() -> Result<bool> {
    let path = entry_path()?;
    let Ok(contents) = fs::read_to_string(&path) else {
        // No user entry: enabled only if the package supplies a system-wide one.
        return Ok(system_entry_exists());
    };
    let disabled = contents.lines().any(|line| {
        let l = line.trim().replace(' ', "").to_ascii_lowercase();
        l == "hidden=true" || l == "x-gnome-autostart-enabled=false"
    });
    Ok(!disabled)
}

/// Enable launch-on-login by writing the autostart entry (idempotent).
pub fn enable() -> Result<()> {
    let dir = autostart_dir()?;
    fs::create_dir_all(&dir)
        .with_context(|| format!("creating autostart directory {}", dir.display()))?;
    let path = dir.join(ENTRY);
    let contents = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=alavai (tray)\n\
         Comment=alavai system-tray daemon — one-click tailnet switching\n\
         Exec={}\n\
         Icon=alavai\n\
         Terminal=false\n\
         Categories=Network;\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_command()
    );
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Disable launch-on-login (idempotent).
///
/// Normally we just remove our user entry. But if the package installed a
/// system-wide entry, a removal wouldn't stop it firing — so we write a
/// `Hidden=true` user entry, which XDG honours as a mask over the system copy.
pub fn disable() -> Result<()> {
    let path = entry_path()?;
    if system_entry_exists() {
        let dir = autostart_dir()?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("creating autostart directory {}", dir.display()))?;
        let masked = "[Desktop Entry]\n\
             Type=Application\n\
             Name=alavai (tray)\n\
             Hidden=true\n";
        return fs::write(&path, masked).with_context(|| format!("writing {}", path.display()));
    }
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}
