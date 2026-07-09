//! Integration coverage for the real macOS platform seams (Phase 4). Compiled
//! only under `--features mac-inject` on macOS; the pure-logic assertions run
//! anywhere with a login session, while the ones that mutate the real
//! pasteboard or need a window server gate behind `VERBATIM_MAC_E2E=1` so the
//! default suite never touches the developer's clipboard.
#![cfg(all(target_os = "macos", feature = "mac-inject"))]

use verbatim_platform::macos::MacTextInjector;
use verbatim_platform::macos::{MacClipboardGuard, MacFocusTracker, MacPermissionProbe};
use verbatim_platform::{
    Capability, ClipboardGuard, FocusTracker, PermissionProbe, PermissionState, RestoreOutcome,
    TextInjector,
};

fn e2e_enabled() -> bool {
    std::env::var_os("VERBATIM_MAC_E2E").is_some_and(|v| v == "1")
}

/// The real general pasteboard is process-wide shared state, and the test
/// harness runs tests in parallel threads. Every test that writes it takes this
/// lock, or they clobber each other's fixtures.
static PASTEBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_pasteboard() -> std::sync::MutexGuard<'static, ()> {
    match PASTEBOARD.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[test]
fn injector_probe_always_offers_a_last_resort() {
    // Whatever the AX trust state, the clipboard-only fallback must be present
    // so a session is never left with no injection path (E4).
    let backends = MacTextInjector::new().probe();
    assert!(
        backends
            .iter()
            .any(|b| matches!(b, verbatim_platform::InjectionBackend::ClipboardOnly)),
        "probe must always include ClipboardOnly, got {backends:?}"
    );
}

#[test]
fn injector_probe_keeps_the_clipboard_surrender_last() {
    // The point of #18 is that text lands *in the app*: when AX is trusted the
    // paste/typing backends precede the clipboard-only surrender, and even
    // without trust the clipboard is the single, last fallback - never tried
    // ahead of a backend that could actually deliver.
    let backends = MacTextInjector::new().probe();
    assert_eq!(
        backends.last(),
        Some(&verbatim_platform::InjectionBackend::ClipboardOnly),
        "clipboard-only must be the last resort, got {backends:?}"
    );
    let clipboard_positions = backends
        .iter()
        .filter(|b| matches!(b, verbatim_platform::InjectionBackend::ClipboardOnly))
        .count();
    assert_eq!(
        clipboard_positions, 1,
        "clipboard-only must appear exactly once, got {backends:?}"
    );
}

#[test]
fn permission_probe_answers_every_capability_without_prompting() {
    let probe = MacPermissionProbe::new();
    // Each probe must return a defined state and, critically, must not block or
    // prompt. We only assert the FFI path yields a valid enum.
    for capability in [
        Capability::Microphone,
        Capability::TextInjection,
        Capability::InputMonitoring,
    ] {
        let state = probe.probe(capability);
        assert!(matches!(
            state,
            PermissionState::Granted
                | PermissionState::Denied
                | PermissionState::Undetermined
                | PermissionState::NotNeeded
        ));
    }
}

#[test]
fn clipboard_transient_write_then_restore_preserves_user_content() {
    if !e2e_enabled() {
        eprintln!("skipping clipboard E2E; set VERBATIM_MAC_E2E=1 to run");
        return;
    }
    let _serialized = lock_pasteboard();
    let guard = MacClipboardGuard::new();

    // Seed a known "user" clipboard, snapshot it, stage dictated text, then
    // restore. With no intervening write the original must come back verbatim.
    guard.set_persistent_text("user's own text").unwrap();
    let snapshot = guard.snapshot().unwrap();
    assert_eq!(snapshot.text.as_deref(), Some("user's own text"));

    guard.set_transient_text("dictated words").unwrap();
    assert_eq!(
        guard.snapshot().unwrap().text.as_deref(),
        Some("dictated words")
    );

    assert_eq!(
        guard.restore_if_unchanged(snapshot).unwrap(),
        RestoreOutcome::Restored
    );
    assert_eq!(
        guard.snapshot().unwrap().text.as_deref(),
        Some("user's own text")
    );
}

#[test]
fn focus_tracker_reports_the_frontmost_app() {
    if !e2e_enabled() {
        eprintln!("skipping focus E2E; set VERBATIM_MAC_E2E=1 to run");
        return;
    }
    let focused = MacFocusTracker::new()
        .focused_app()
        .expect("a frontmost app should exist in a GUI session");
    assert!(
        !focused.app_id.is_empty(),
        "frontmost app id should be non-empty"
    );
}

/// The paste backend restores the clipboard from a detached thread, so a second
/// dictation can arrive while our own transient text is still on the pasteboard.
/// `holds_our_transient` is what stops that dictation from snapshotting our text
/// as "the user's clipboard" and later handing it back to them as if it were.
#[test]
fn transient_ownership_is_recognized_until_someone_else_writes() {
    if !e2e_enabled() {
        eprintln!("skipping: set VERBATIM_MAC_E2E=1 to exercise the real pasteboard");
        return;
    }
    let _serialized = lock_pasteboard();
    let guard = MacClipboardGuard::new();

    // Nothing staged yet: the pasteboard is the user's, whatever it holds.
    guard.set_persistent_text("user content").unwrap();
    assert!(
        !guard.holds_our_transient(),
        "a clipboard we have not staged into is not ours"
    );

    let snapshot = guard.snapshot().unwrap();
    guard.set_transient_text("dictated text").unwrap();
    assert!(
        guard.holds_our_transient(),
        "our own transient write must be recognized as ours"
    );

    // The user copying something must hand ownership straight back.
    guard.set_persistent_text("user copied this").unwrap();
    assert!(
        !guard.holds_our_transient(),
        "a write after ours means the pasteboard is the user's again"
    );

    // And the delayed restore must then yield to them rather than clobber it.
    assert_eq!(
        guard.restore_if_unchanged(snapshot).unwrap(),
        RestoreOutcome::UserModified
    );
    assert_eq!(
        guard.snapshot().unwrap().text.as_deref(),
        Some("user copied this")
    );
}
