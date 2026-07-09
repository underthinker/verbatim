//! Linux `AccessibilityAnnouncer`: AT-SPI screen-reader detection plus
//! transient overlay announcements delivered as desktop notifications
//! (UX.md 8).
//!
//! The overlay window is click-through and never focused, so its webview
//! `aria-live` region is never in Orca's monitored tree; something OS-level has
//! to carry the state.
//!
//! Detection is exact: `at-spi2-core` owns `org.a11y.Bus` on the session bus
//! and publishes `org.a11y.Status.ScreenReaderEnabled`, which Orca sets while
//! it runs. The flag is watched rather than polled, so turning Orca on or off
//! mid-session is picked up without a restart, and `screen_reader_active` stays
//! a non-blocking atomic read - the overlay driver calls it on every transition
//! from an async task that must not block on D-Bus.
//!
//! Announcement is a compromise, and the reason is worth stating. The
//! equivalent of the macOS `NSAccessibilityPostNotification` is an AT-SPI
//! `Announcement` event, but AT-SPI only routes events whose source resolves to
//! an accessible object registered with the a11y registry. Verbatim exports no
//! accessible tree, so an `Announcement` raised from an unregistered path is
//! dropped by Orca's event manager before it is ever spoken. Registering a
//! minimal AT-SPI application root is the real fix and the upgrade path here;
//! until then this posts the overlay state as a transient desktop notification,
//! which Orca does present. Successive states reuse one notification id, so the
//! banner is replaced in place rather than stacked.
//!
//! Everything D-Bus lives on one owned worker thread. `announce` only pushes a
//! string onto a channel, so it stays safe to call from the main thread the
//! overlay driver dispatches to.
//!
//! CI compiles this on the Linux `real-injection` package job, but the spoken
//! announcement can only be confirmed with Orca actually running - the same
//! manual on-device sign-off the injection and permission seams carry.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use zbus::zvariant::Value;

use crate::traits::AccessibilityAnnouncer;

/// How long a state banner lingers before the notification daemon expires it.
/// Long enough for Orca to speak it, short enough that a missed replacement
/// does not leave a stale state on screen.
const EXPIRE_TIMEOUT_MS: i32 = 2_000;

/// `org.a11y.Status`, published by `at-spi2-core` on the session bus. Absent
/// when no accessibility stack is installed, which reads as "no screen reader".
#[zbus::proxy(
    interface = "org.a11y.Status",
    default_service = "org.a11y.Bus",
    default_path = "/org/a11y/bus"
)]
trait A11yStatus {
    #[zbus(property)]
    fn screen_reader_enabled(&self) -> zbus::Result<bool>;
}

/// The freedesktop notification daemon. Sandboxed builds need
/// `--talk-name=org.freedesktop.Notifications` for this to resolve.
#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, &Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Announcer backed by a single D-Bus worker thread.
pub struct LinuxAnnouncer {
    /// Mirrors `org.a11y.Status.ScreenReaderEnabled`; written by the worker,
    /// read lock-free by the overlay driver on every session transition.
    active: Arc<AtomicBool>,
    /// Unbounded because the overlay produces at most a handful of states per
    /// dictation and `announce` must never block the caller. A closed channel
    /// (worker gone) makes sends fail silently, which is the documented
    /// best-effort contract.
    announcements: UnboundedSender<String>,
}

impl Default for LinuxAnnouncer {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxAnnouncer {
    pub fn new() -> Self {
        let active = Arc::new(AtomicBool::new(false));
        let (announcements, inbox) = unbounded_channel();

        let worker_active = Arc::clone(&active);
        if let Err(err) = std::thread::Builder::new()
            .name("verbatim-a11y".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        tracing::warn!(?err, "a11y worker disabled: no runtime");
                        return;
                    }
                };
                runtime.block_on(serve(worker_active, inbox));
            })
        {
            // A thread that never started leaves `active` false forever, so the
            // driver simply never announces. Dictation is unaffected.
            tracing::warn!(?err, "a11y worker disabled: thread spawn failed");
        }

        Self {
            active,
            announcements,
        }
    }
}

impl AccessibilityAnnouncer for LinuxAnnouncer {
    fn screen_reader_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn announce(&self, message: &str) {
        if self.announcements.send(message.to_owned()).is_err() {
            tracing::warn!("a11y announce skipped: worker thread is gone");
        }
    }
}

/// Own the session bus for the process lifetime: mirror the screen-reader flag
/// into `active` and drain queued announcements onto the notification daemon.
async fn serve(active: Arc<AtomicBool>, mut inbox: tokio::sync::mpsc::UnboundedReceiver<String>) {
    let connection = match zbus::Connection::session().await {
        Ok(connection) => connection,
        Err(err) => {
            // No session bus means no Orca and no notification daemon; drop the
            // worker rather than retry into a socket that will not appear.
            tracing::warn!(?err, "a11y worker disabled: no session bus");
            return;
        }
    };

    let status = match A11yStatusProxy::new(&connection).await {
        Ok(status) => status,
        Err(err) => {
            tracing::warn!(?err, "a11y worker disabled: no org.a11y.Status proxy");
            return;
        }
    };

    // Seed from the current value, then follow it. A read failure here is the
    // normal "no accessibility stack installed" case, not an error.
    active.store(
        status.screen_reader_enabled().await.unwrap_or(false),
        Ordering::Relaxed,
    );
    // The change stream is built from a PropertiesChanged match, so it also
    // delivers once `at-spi2-core` appears on a bus that had no a11y stack when
    // the worker started.
    let mut changes = status.receive_screen_reader_enabled_changed().await;

    // Built lazily on the first announcement: a session with no screen reader
    // never talks to the notification daemon at all.
    let mut notifications: Option<NotificationsProxy> = None;
    // 0 asks the daemon for a fresh id; every later post replaces that one.
    let mut banner: u32 = 0;

    loop {
        tokio::select! {
            Some(change) = changes.next() => {
                match change.get().await {
                    Ok(enabled) => active.store(enabled, Ordering::Relaxed),
                    Err(err) => tracing::warn!(?err, "a11y screen-reader flag read failed"),
                }
            }
            message = inbox.recv() => {
                let Some(message) = message else {
                    // Announcer dropped: the app is shutting down.
                    break;
                };
                let proxy = match &notifications {
                    Some(proxy) => proxy,
                    None => match NotificationsProxy::new(&connection).await {
                        Ok(proxy) => notifications.insert(proxy),
                        Err(err) => {
                            tracing::warn!(?err, "a11y announce failed: no notification daemon");
                            continue;
                        }
                    },
                };
                banner = post(proxy, banner, &message).await.unwrap_or(banner);
            }
            else => break,
        }
    }
}

/// Post one state banner, replacing the previous one. Returns the daemon's
/// notification id so the next state reuses it.
async fn post(proxy: &NotificationsProxy<'_>, banner: u32, message: &str) -> Option<u32> {
    // `transient` keeps the banner out of the notification tray - overlay state
    // is ephemeral and must not accumulate history. Normal urgency: screen
    // readers speak it, but it never interrupts a critical alert.
    let transient = Value::from(true);
    let urgency = Value::from(1u8);
    let hints = HashMap::from([("transient", &transient), ("urgency", &urgency)]);

    // The message goes in the summary, not the body: notification daemons and
    // Orca both lead with the summary, and the overlay states are one phrase.
    match proxy
        .notify(
            "Verbatim",
            banner,
            "",
            message,
            "",
            &[],
            hints,
            EXPIRE_TIMEOUT_MS,
        )
        .await
    {
        Ok(id) => Some(id),
        Err(err) => {
            tracing::warn!(?err, "a11y announce failed");
            None
        }
    }
}
