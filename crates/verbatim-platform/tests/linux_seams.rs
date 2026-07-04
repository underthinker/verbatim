//! Integration coverage for the real Linux platform seams (Phase 6). Compiled
//! only under `--features linux-inject` on Linux; the pure-logic assertions
//! run anywhere, while the ones that need a Wayland session (and may pop a
//! portal consent dialog or mutate the real clipboard) gate behind
//! `VERBATIM_LINUX_E2E=1` so headless CI and the default suite never touch
//! them.
#![cfg(all(target_os = "linux", feature = "linux-inject"))]

use verbatim_platform::linux::{
    LinuxClipboardGuard, LinuxFocusTracker, LinuxPermissionProbe, LinuxTextInjector,
};
use verbatim_platform::{
    Capability, ClipboardGuard, FocusTracker, PermissionProbe, PermissionState, RestoreOutcome,
    TextInjector,
};

fn e2e_enabled() -> bool {
    std::env::var_os("VERBATIM_LINUX_E2E").is_some_and(|v| v == "1")
}

#[test]
fn injector_probe_always_offers_a_last_resort() {
    // Whatever the portal/uinput state, the clipboard-only fallback must be
    // present so a session is never left with no injection path (E4).
    let backends = LinuxTextInjector::new().probe();
    assert!(
        backends
            .iter()
            .any(|b| matches!(b, verbatim_platform::InjectionBackend::ClipboardOnly)),
        "probe must always include ClipboardOnly, got {backends:?}"
    );
}

#[test]
fn permission_probe_answers_every_capability_without_prompting() {
    let probe = LinuxPermissionProbe::new();
    // Each probe must return a defined state and, critically, must not block
    // or prompt (the portal consent dialog only ever appears on first inject).
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
fn focus_tracker_always_reports_a_target() {
    // Wayland offers no cross-client focus query; the tracker must still
    // return a target so injection is never aborted on E7 (the compositor
    // delivers our events to whatever it has focused).
    let focused = LinuxFocusTracker::new()
        .focused_app()
        .expect("the placeholder focus identity must always resolve");
    assert!(!focused.app_id.is_empty());
}

#[test]
fn clipboard_transient_write_then_restore_preserves_user_content() {
    if !e2e_enabled() {
        eprintln!("skipping clipboard E2E; set VERBATIM_LINUX_E2E=1 to run");
        return;
    }
    let guard = LinuxClipboardGuard::new();

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
