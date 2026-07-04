//! Real Windows [`HotkeyManager`]: `RegisterHotKey` on a dedicated
//! message-loop thread (windows doc stub, plan phase 7).
//!
//! `RegisterHotKey` only reports presses (`WM_HOTKEY`), so the release edge -
//! which the hold/push-to-talk semantics in core need - is synthesized by
//! polling `GetAsyncKeyState` on the chord's key until it goes up. Unlike the
//! macOS backend nothing here needs the process main thread: the hotkey is
//! bound to the thread that registers it, so the backend owns one background
//! thread for registration, message pumping, and release polling, and tears
//! it down with `WM_QUIT` on unregister/drop.
//!
//! A bare-modifier push-to-talk trigger (the macOS `modifier_tap` equivalent,
//! via a `WH_KEYBOARD_LL` hook) is deferred until a chord proves insufficient
//! on real hardware; chords cover the M1 walking skeleton.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PostThreadMessageW, WM_HOTKEY, WM_QUIT,
};

use crate::errors::HotkeyError;
use crate::traits::{HotkeyCallback, HotkeyManager};
use crate::types::{HotkeyBinding, HotkeyEvent};

/// Our single hotkey's thread-local id (`RegisterHotKey` ids are per-thread).
const HOTKEY_ID: i32 = 1;

/// How often the message-loop thread samples `GetAsyncKeyState` while waiting
/// for the chord's key to be released.
const RELEASE_POLL: Duration = Duration::from_millis(15);

/// A [`HotkeyManager`] backed by `RegisterHotKey`. `Send + Sync`: all OS state
/// lives on the owned message-loop thread.
#[derive(Default)]
pub struct WinHotkeyBackend {
    registered: Option<Registered>,
}

struct Registered {
    thread_id: u32,
    thread: JoinHandle<()>,
}

impl WinHotkeyBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HotkeyManager for WinHotkeyBackend {
    fn register(
        &mut self,
        binding: &HotkeyBinding,
        on_event: HotkeyCallback,
    ) -> Result<(), HotkeyError> {
        if self.registered.is_some() {
            return Err(HotkeyError::AlreadyRegistered);
        }
        let chord = parse_chord(&binding.chord)?;
        let chord_text = binding.chord.clone();

        // The registration outcome is decided on the message-loop thread
        // (RegisterHotKey binds to the calling thread) and reported back here
        // so a taken chord fails register() synchronously.
        let (outcome_tx, outcome_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("verbatim-hotkey".to_owned())
            .spawn(move || message_loop(chord, chord_text, on_event, &outcome_tx))
            .map_err(|err| HotkeyError::Backend(err.to_string()))?;

        match outcome_rx.recv() {
            Ok(Ok(thread_id)) => {
                self.registered = Some(Registered { thread_id, thread });
                Ok(())
            }
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => {
                let _ = thread.join();
                Err(HotkeyError::Backend(
                    "hotkey thread died before reporting".to_owned(),
                ))
            }
        }
    }

    fn unregister(&mut self) {
        if let Some(reg) = self.registered.take() {
            // SAFETY: plain FFI; posting to a dead thread just fails.
            let _ = unsafe { PostThreadMessageW(reg.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = reg.thread.join();
        }
    }
}

impl Drop for WinHotkeyBackend {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// A parsed chord: `RegisterHotKey` modifiers plus the virtual key.
#[derive(Clone, Copy)]
struct Chord {
    modifiers: HOT_KEY_MODIFIERS,
    vk: u32,
}

/// Register, pump `WM_HOTKEY`, synthesize release edges, unregister on quit.
fn message_loop(
    chord: Chord,
    chord_text: String,
    on_event: HotkeyCallback,
    outcome: &mpsc::Sender<Result<u32, HotkeyError>>,
) {
    // MOD_NOREPEAT keeps auto-repeat from spamming Pressed edges while held.
    let registration =
        unsafe { RegisterHotKey(None, HOTKEY_ID, chord.modifiers | MOD_NOREPEAT, chord.vk) };
    if let Err(err) = registration {
        let _ = outcome.send(Err(classify(err.to_string(), &chord_text)));
        return;
    }
    let _ = outcome.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut msg = MSG::default();
    // GetMessageW returns 0 on WM_QUIT and -1 on error; both end the loop.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        if msg.message != WM_HOTKEY || msg.wParam.0 != HOTKEY_ID as usize {
            continue;
        }
        on_event(HotkeyEvent::Pressed);
        // RegisterHotKey never reports the release; sample the chord's key
        // until it goes up so hold/push-to-talk semantics get their edge.
        while key_down(chord.vk) {
            thread::sleep(RELEASE_POLL);
        }
        on_event(HotkeyEvent::Released);
    }

    let _ = unsafe { UnregisterHotKey(None, HOTKEY_ID) };
}

fn key_down(vk: u32) -> bool {
    // High bit set means the key is currently down.
    (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0
}

/// Parse `Ctrl+Alt+Space`-style chords into `RegisterHotKey` arguments. The
/// textual form matches the macOS/Linux backends so `VERBATIM_HOTKEY` is
/// portable.
fn parse_chord(chord: &str) -> Result<Chord, HotkeyError> {
    let mut modifiers = HOT_KEY_MODIFIERS(0);
    let mut vk = None;
    for token in chord.split('+') {
        let token = token.trim();
        match token.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => modifiers |= MOD_CONTROL,
            "ALT" | "OPTION" => modifiers |= MOD_ALT,
            "SHIFT" => modifiers |= MOD_SHIFT,
            "SUPER" | "WIN" | "META" | "CMD" | "COMMAND" => modifiers |= MOD_WIN,
            key => {
                if vk.replace(parse_key(key, chord)?).is_some() {
                    return Err(HotkeyError::ChordUnavailable(chord.to_owned()));
                }
            }
        }
    }
    let Some(vk) = vk else {
        // A bare-modifier binding needs the deferred WH_KEYBOARD_LL backend.
        return Err(HotkeyError::ChordUnavailable(chord.to_owned()));
    };
    Ok(Chord { modifiers, vk })
}

/// Map one non-modifier token to a virtual-key code.
fn parse_key(key: &str, chord: &str) -> Result<u32, HotkeyError> {
    // Letters and digits use their ASCII code as the virtual key.
    if key.len() == 1 {
        let ch = key.as_bytes()[0];
        if ch.is_ascii_alphanumeric() {
            return Ok(u32::from(ch.to_ascii_uppercase()));
        }
    }
    if let Some(number) = key.strip_prefix('F')
        && let Ok(n) = number.parse::<u32>()
        && (1..=24).contains(&n)
    {
        return Ok(0x70 + (n - 1)); // VK_F1..VK_F24
    }
    match key {
        "SPACE" => Ok(0x20),            // VK_SPACE
        "TAB" => Ok(0x09),              // VK_TAB
        "ESC" | "ESCAPE" => Ok(0x1B),   // VK_ESCAPE
        "ENTER" | "RETURN" => Ok(0x0D), // VK_RETURN
        _ => Err(HotkeyError::ChordUnavailable(chord.to_owned())),
    }
}

/// Map the OS error onto the typed error. A "taken" chord is distinct from a
/// generic backend failure so callers can guide the user.
fn classify(message: String, chord: &str) -> HotkeyError {
    let lower = message.to_lowercase();
    if lower.contains("already registered") || lower.contains("1409") {
        HotkeyError::ChordUnavailable(chord.to_owned())
    } else {
        HotkeyError::Backend(message)
    }
}
