//! Overlay window property assertions (M2 Phase B verification): the overlay
//! must be non-activating and stay unfocused even when shown - the spike-1
//! focus-steal regression, automated where a window server is available.
//!
//! `harness = false`: the webview event loop must own the process main
//! thread (macOS refuses window creation elsewhere), which the default test
//! harness does not allow. Gated behind `VERBATIM_GUI_E2E=1` like the
//! platform seam tests, so headless CI and default local runs skip it.
//!
//! Not compiled on Windows: referencing tauri from this standalone test
//! binary retains webview DLL imports the CI runner cannot resolve, so the
//! executable dies at load (`STATUS_ENTRYPOINT_NOT_FOUND`) before the env
//! gate even runs. The Windows non-activating check (WS_EX_NOACTIVATE) is a
//! manual sign-off alongside the KDE Plasma 6 focus-steal check.

#[cfg(windows)]
fn main() {
    eprintln!("overlay_window: skipped on Windows (manual sign-off, see module docs)");
}

#[cfg(not(windows))]
fn main() {
    use std::time::Duration;

    use verbatim_app::overlay;

    let e2e_enabled = std::env::var_os("VERBATIM_GUI_E2E").is_some_and(|v| v == "1");
    if !e2e_enabled {
        eprintln!("overlay_window: skipped (set VERBATIM_GUI_E2E=1 with a window server to run)");
        return;
    }

    let app = tauri::Builder::default()
        .setup(|app| {
            let window = overlay::create_window(app.handle())?;

            assert!(
                !window.is_visible()?,
                "overlay must be created hidden so ARMING can show it cheaply"
            );
            assert!(
                window.is_always_on_top()?,
                "overlay must sit above all windows (UX.md 7)"
            );
            assert!(
                !window.is_focused()?,
                "overlay must never hold focus while hidden"
            );

            // The focus-steal regression: showing the overlay must not
            // activate it (spike 1 broke Handy on KDE this way).
            window.show()?;
            std::thread::sleep(Duration::from_millis(300));
            assert!(window.is_visible()?, "overlay must be showable");
            assert!(
                !window.is_focused()?,
                "overlay must not take focus when shown (spike-1 regression)"
            );

            app.handle().exit(0);
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|err| panic!("overlay_window: app build failed: {err}"));

    // `exit(0)` was requested at the end of setup; the loop unwinds here.
    // Reaching `run` at all means every assertion above passed.
    println!("overlay_window: window property assertions passed");
    app.run(|_, _| {});
}
