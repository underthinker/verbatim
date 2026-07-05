//! Integration coverage for the real Windows platform seams (Phase 7). Compiled
//! only under `--features win-inject` on Windows; the pure-logic assertions run
//! headless, while the ones that mutate the real clipboard or need a desktop
//! session (a foreground window, a real paste) gate behind `VERBATIM_WIN_E2E=1`
//! so the default suite never touches the developer's clipboard.
#![cfg(all(target_os = "windows", feature = "win-inject"))]

use verbatim_platform::windows::{
    WinClipboardGuard, WinFocusTracker, WinPermissionProbe, WinTextInjector,
};
use verbatim_platform::{
    Capability, ClipboardGuard, FocusTracker, InjectionBackend, PermissionProbe, PermissionState,
    RestoreOutcome, TextInjector,
};

fn e2e_enabled() -> bool {
    std::env::var_os("VERBATIM_WIN_E2E").is_some_and(|v| v == "1")
}

#[test]
fn injector_probe_always_offers_a_last_resort() {
    // UIPI denial is only observable per-target at injection time, so the full
    // chain is always offered; the clipboard-only fallback must always be
    // present so a session is never left with no injection path (E4).
    let backends = WinTextInjector::new().probe();
    assert!(
        backends
            .iter()
            .any(|b| matches!(b, InjectionBackend::ClipboardOnly)),
        "probe must always include ClipboardOnly, got {backends:?}"
    );
}

#[test]
fn injector_probe_tries_real_delivery_before_the_clipboard() {
    // The point of #18 is that text lands *in the app*: direct SendInput typing
    // must be attempted first and the clipboard-only surrender must be last, so
    // we never give up to a manual paste while a real backend could deliver.
    let backends = WinTextInjector::new().probe();
    assert_eq!(
        backends.first(),
        Some(&InjectionBackend::SendInputUnicode),
        "SendInput unicode typing must be the first backend, got {backends:?}"
    );
    assert_eq!(
        backends.last(),
        Some(&InjectionBackend::ClipboardOnly),
        "clipboard-only must be the last resort, got {backends:?}"
    );
}

#[test]
fn permission_probe_answers_every_capability_without_prompting() {
    let probe = WinPermissionProbe::new();
    // Each probe must return a defined state and, critically, must not block or
    // prompt: synthetic input needs no grant on Windows (spike 2).
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
fn focus_tracker_reports_the_foreground_app() {
    if !e2e_enabled() {
        eprintln!("skipping focus E2E; set VERBATIM_WIN_E2E=1 to run");
        return;
    }
    // Headless CI has no foreground window; only assert under E2E where a
    // desktop session exists.
    let focused = WinFocusTracker::new()
        .focused_app()
        .expect("a foreground app should exist in a desktop session");
    assert!(
        !focused.app_id.is_empty(),
        "foreground app id should be non-empty"
    );
}

#[test]
fn clipboard_transient_write_then_restore_preserves_user_content() {
    if !e2e_enabled() {
        eprintln!("skipping clipboard E2E; set VERBATIM_WIN_E2E=1 to run");
        return;
    }
    let guard = WinClipboardGuard::new();

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
