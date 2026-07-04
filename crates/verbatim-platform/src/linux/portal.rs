//! RemoteDesktop portal session: the consent-ful keyboard-injection channel
//! on GNOME and KDE (spike 1).
//!
//! The portal grants a session after an explicit user dialog; we request
//! `PersistMode::ExplicitlyRevoked` and persist the returned `restore_token`
//! so subsequent daemon runs reconnect silently (spike 1 `restore_token`
//! persistence requirement).
//!
//! The portal stack is async (zbus); this module owns a current-thread tokio
//! runtime and exposes a small blocking API to the sync platform traits.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use ashpd::WindowIdentifier;
use ashpd::desktop::remote_desktop::{
    DeviceType, KeyState, NotifyKeyboardKeycodeOptions, RemoteDesktop, SelectDevicesOptions,
    StartOptions,
};
use ashpd::desktop::{CreateSessionOptions, PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use tokio::runtime::Runtime;

use crate::errors::InjectError;
use crate::types::InjectionBackend;

/// Linux evdev keycode for left Ctrl (`KEY_LEFTCTRL`).
const KEY_LEFTCTRL: i32 = 29;
/// Linux evdev keycode for V (`KEY_V`).
const KEY_V: i32 = 47;

/// Small gap between synthesized key transitions so the compositor delivers
/// them in order.
const KEY_GAP: Duration = Duration::from_millis(5);

fn backend_error(reason: impl ToString) -> InjectError {
    InjectError::Backend {
        backend: InjectionBackend::LibeiPortal,
        reason: reason.to_string(),
    }
}

/// Where the `restore_token` is persisted across daemon runs.
fn token_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(base.join("verbatim/remote-desktop.token"))
}

fn load_token() -> Option<String> {
    let token = fs::read_to_string(token_path()?).ok()?;
    let token = token.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

fn store_token(token: &str) {
    let Some(path) = token_path() else { return };
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(?err, "cannot create state dir for portal restore token");
        return;
    }
    if let Err(err) = fs::write(&path, token) {
        tracing::warn!(?err, "cannot persist portal restore token");
    }
}

fn clear_token() {
    if let Some(path) = token_path() {
        let _ = fs::remove_file(path);
    }
}

/// A started RemoteDesktop portal session with keyboard access.
pub struct PortalSession {
    runtime: Runtime,
    proxy: RemoteDesktop,
    session: Session<RemoteDesktop>,
}

impl PortalSession {
    /// Connect, negotiate keyboard access (silently when a valid
    /// `restore_token` exists, otherwise via the portal consent dialog), and
    /// persist the new token.
    pub fn connect() -> Result<Self, InjectError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(backend_error)?;
        let (proxy, session) = runtime.block_on(Self::open())?;
        Ok(Self {
            runtime,
            proxy,
            session,
        })
    }

    async fn open() -> Result<(RemoteDesktop, Session<RemoteDesktop>), InjectError> {
        let proxy = RemoteDesktop::new().await.map_err(backend_error)?;
        let session = proxy
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(backend_error)?;

        let restore_token = load_token();
        let had_token = restore_token.is_some();
        proxy
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(BitFlags::from(DeviceType::Keyboard))
                    .set_persist_mode(PersistMode::ExplicitlyRevoked)
                    .set_restore_token(restore_token.as_deref()),
            )
            .await
            .map_err(backend_error)?
            .response()
            .map_err(backend_error)?;

        let devices = proxy
            .start(&session, None::<&WindowIdentifier>, StartOptions::default())
            .await
            .map_err(|err| {
                // A stale/revoked token can fail the whole start; drop it so
                // the next attempt asks the user again instead of looping.
                if had_token {
                    clear_token();
                }
                backend_error(err)
            })?
            .response()
            .map_err(backend_error)?;

        if !devices.devices().contains(DeviceType::Keyboard) {
            return Err(backend_error("portal session granted without keyboard"));
        }
        match devices.restore_token() {
            Some(token) => store_token(token),
            None => tracing::warn!("portal did not return a restore token; consent will re-prompt"),
        }
        Ok((proxy, session))
    }

    /// Synthesize Ctrl-V through the portal keyboard.
    pub fn send_paste_chord(&self) -> Result<(), InjectError> {
        self.runtime.block_on(async {
            self.key(KEY_LEFTCTRL, KeyState::Pressed).await?;
            tokio::time::sleep(KEY_GAP).await;
            self.key(KEY_V, KeyState::Pressed).await?;
            tokio::time::sleep(KEY_GAP).await;
            self.key(KEY_V, KeyState::Released).await?;
            tokio::time::sleep(KEY_GAP).await;
            self.key(KEY_LEFTCTRL, KeyState::Released).await
        })
    }

    async fn key(&self, keycode: i32, state: KeyState) -> Result<(), InjectError> {
        self.proxy
            .notify_keyboard_keycode(
                &self.session,
                keycode,
                state,
                NotifyKeyboardKeycodeOptions::default(),
            )
            .await
            .map_err(backend_error)
    }
}

impl Drop for PortalSession {
    fn drop(&mut self) {
        let close = self.session.close();
        if let Err(err) = self.runtime.block_on(close) {
            tracing::debug!(?err, "portal session close failed");
        }
    }
}

/// Cheap availability probe: is a D-Bus session (and thus possibly a desktop
/// portal) reachable at all? The real consent check happens on connect.
pub fn portal_plausible() -> bool {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        || std::env::var_os("XDG_RUNTIME_DIR").is_some()
}
