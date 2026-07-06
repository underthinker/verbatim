//! Onboarding window property assertions (M2 Phase D verification): unlike the
//! non-activating overlay, the onboarding window is a normal focusable window
//! that shows on first run (UX.md 6).
//!
//! `harness = false` and gated behind `VERBATIM_GUI_E2E=1`, exactly like
//! `overlay_window` - the webview event loop must own the process main thread,
//! and headless CI skips it. Not compiled on Windows for the same webview-DLL
//! reason documented in `overlay_window`.

#[cfg(windows)]
fn main() {
    eprintln!("onboarding_window: skipped on Windows (manual sign-off, see overlay_window)");
}

#[cfg(not(windows))]
fn main() {
    use verbatim_app::onboarding;

    let e2e_enabled = std::env::var_os("VERBATIM_GUI_E2E").is_some_and(|v| v == "1");
    if !e2e_enabled {
        eprintln!(
            "onboarding_window: skipped (set VERBATIM_GUI_E2E=1 with a window server to run)"
        );
        return;
    }

    let app = tauri::Builder::default()
        .setup(|app| {
            let window = onboarding::create_window(app.handle())?;

            // A normal, visible, fixed-size guided window - the contrast to the
            // non-activating overlay pill.
            assert!(window.is_visible()?, "onboarding must show on first run");
            assert!(
                !window.is_resizable()?,
                "onboarding is a fixed-size guided flow"
            );

            app.handle().exit(0);
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("onboarding_window: app build failed: {err}"));

    println!("onboarding_window: window property assertions passed");
    app.run(|_, _| {});
}
