//! `verbatim inject-selftest`: the repeatable E2E injection check behind the
//! M1 acceptance criterion (issue #18). Focus a text field in any foreign app,
//! run the command, and confirm the sentinel string lands - the one honest
//! signal that the platform backend chain delivers real keystrokes, not stubs.
//!
//! We cannot read the target app's contents back (that would need AX-read or
//! screenshot OCR, a far larger permission surface), so verification is
//! human-in-the-loop by construction: we drive the inject half deterministically
//! and print an honest receipt; the operator confirms the visual result.

use std::process::ExitCode;

/// A sentinel exercising ASCII, digits, and multi-byte UTF-8 so a truncating or
/// encoding-broken backend shows itself.
const SENTINEL: &str = "Verbatim inject self-test OK 1234 \u{4f60}\u{597d}";

pub fn run(text: Option<String>) -> ExitCode {
    let text = text.unwrap_or_else(|| SENTINEL.to_owned());
    run_impl(&text)
}

#[cfg(all(feature = "real-injection", any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn run_impl(text: &str) -> ExitCode {
    use std::thread;
    use std::time::Duration;

    use verbatim_platform::{FocusTracker, InjectionStrategy, TextInjector};

    /// Seconds to let the operator focus a target text field before we inject.
    const FOCUS_COUNTDOWN: u64 = 5;

    #[cfg(target_os = "macos")]
    let (injector, focus): (Box<dyn TextInjector>, Box<dyn FocusTracker>) = (
        Box::new(verbatim_platform::macos::MacTextInjector::new()),
        Box::new(verbatim_platform::macos::MacFocusTracker::new()),
    );
    #[cfg(target_os = "linux")]
    let (injector, focus): (Box<dyn TextInjector>, Box<dyn FocusTracker>) = (
        Box::new(verbatim_platform::linux::LinuxTextInjector::new()),
        Box::new(verbatim_platform::linux::LinuxFocusTracker::new()),
    );
    #[cfg(target_os = "windows")]
    let (injector, focus): (Box<dyn TextInjector>, Box<dyn FocusTracker>) = (
        Box::new(verbatim_platform::windows::WinTextInjector::new()),
        Box::new(verbatim_platform::windows::WinFocusTracker::new()),
    );

    println!("verbatim inject-selftest");
    println!("probed backends (fallback order): {:?}", injector.probe());
    println!(
        "\nFocus a text field in the target app now. Injecting in {FOCUS_COUNTDOWN}s..."
    );
    for remaining in (1..=FOCUS_COUNTDOWN).rev() {
        println!("  {remaining}...");
        thread::sleep(Duration::from_secs(1));
    }

    let target = match focus.focused_app() {
        Ok(app) => app,
        Err(err) => {
            eprintln!("could not read focused app: {err}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "target app: {}{}",
        target.app_id,
        target
            .window_title
            .as_deref()
            .map(|t| format!(" (\"{t}\")"))
            .unwrap_or_default()
    );

    match injector.inject(text, &target, InjectionStrategy::Auto) {
        Ok(receipt) => {
            println!(
                "\nreceipt: backend={:?} verified={}",
                receipt.backend, receipt.verified
            );
            println!(
                "\nExpected sentinel in the target app:\n  {text}\n\nConfirm it landed to tick the platform box in issue #18."
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("\ninjection failed: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(all(feature = "real-injection", any(target_os = "macos", target_os = "linux", target_os = "windows"))))]
fn run_impl(_text: &str) -> ExitCode {
    eprintln!(
        "inject-selftest needs a real backend: rebuild with `--features real-injection` \
         (add `win-inject` on Windows) on macOS, Linux, or Windows."
    );
    ExitCode::FAILURE
}
