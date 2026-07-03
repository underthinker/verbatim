//! Link the macOS system frameworks the `mac-inject` seams call into via raw
//! FFI (AXIsProcessTrusted, IsSecureEventInputEnabled, IOHIDCheckAccess,
//! AVCaptureDevice authorization). AppKit and CoreGraphics are linked by their
//! respective crates; these are the ones we reach for with `extern "C"` /
//! dynamic messaging and must link ourselves. Only emitted for macOS builds
//! that actually enable the feature, so nothing else pays for it.

fn main() {
    let is_macos = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos");
    let mac_inject = std::env::var_os("CARGO_FEATURE_MAC_INJECT").is_some();

    if is_macos && mac_inject {
        for framework in ["ApplicationServices", "Carbon", "IOKit", "AVFoundation"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
