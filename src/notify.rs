//! Desktop notifications via the session bus.
//!
//! Switching tailnet or toggling the connection from the tray is otherwise near-
//! silent — only the icon changes. A short notification ("Switched to …") gives
//! the action visible feedback. We talk to `org.freedesktop.Notifications`
//! directly over the zbus connection already in our dependency tree (it comes in
//! via `ksni`), so there's no extra crate and no shelling out to `notify-send`;
//! it works with any notification daemon (swaync, mako, GNOME, KDE, …).

use std::collections::HashMap;

use zbus::blocking::Connection;
use zbus::zvariant::Value;

/// Sends desktop notifications, reusing one for the life of the process so rapid
/// actions replace the previous toast instead of stacking up.
pub struct Notifier {
    /// `None` when there's no session bus / daemon — then everything no-ops.
    conn: Option<Connection>,
    /// Id of the last notification, passed as `replaces_id` so consecutive
    /// toasts coalesce.
    last_id: u32,
}

impl Notifier {
    pub fn new() -> Self {
        Notifier {
            conn: Connection::session().ok(),
            last_id: 0,
        }
    }

    /// Shows (or replaces) a notification. Best-effort: any failure is ignored so
    /// feedback never breaks the tray.
    pub fn show(&mut self, summary: &str, body: &str) {
        let Some(conn) = &self.conn else {
            return;
        };
        let hints: HashMap<&str, Value> = HashMap::new();
        let actions: Vec<&str> = Vec::new();
        // Notify(app_name, replaces_id, app_icon, summary, body, actions, hints,
        //        expire_timeout) -> id
        let reply = conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "alavai",
                self.last_id,
                "alavai",
                summary,
                body,
                actions,
                hints,
                -1i32,
            ),
        );
        if let Ok(msg) = reply
            && let Ok(id) = msg.body().deserialize::<u32>()
        {
            self.last_id = id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pops a real toast on the running desktop. Ignored by default (needs a
    /// session bus + notification daemon); run with
    /// `cargo test --lib notify -- --ignored --nocapture` to eyeball it.
    #[test]
    #[ignore]
    fn shows_a_notification() {
        let mut n = Notifier::new();
        assert!(n.conn.is_some(), "no session bus / notification daemon");
        n.show("alavai", "Switched to example.ts.net");
        assert_ne!(n.last_id, 0, "daemon did not return a notification id");
    }
}
