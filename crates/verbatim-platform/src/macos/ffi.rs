//! Thin, honest wrappers over the macOS C/ObjC entry points the seams need but
//! that no Rust binding crate covers cleanly. Everything here is a single
//! system call with a documented return contract; the enums the callers map to
//! live in `crate::types`.

use objc2::msg_send;
use objc2::runtime::AnyClass;
use objc2_foundation::NSString;

// `Boolean` is an unsigned char; model it as `u8` and normalize to `bool` at
// the boundary rather than assuming C `bool` ABI.
unsafe extern "C" {
    /// `AXIsProcessTrusted` (ApplicationServices): is this process trusted for
    /// the Accessibility API, without prompting.
    fn AXIsProcessTrusted() -> u8;

    /// `IsSecureEventInputEnabled` (Carbon/HIToolbox): some other app has
    /// secure keyboard entry on (password field). We must not inject (E5).
    fn IsSecureEventInputEnabled() -> u8;

    /// `IOHIDCheckAccess` (IOKit): TCC state for HID event access. We pass
    /// `kIOHIDRequestTypeListenEvent` (1). Returns `kIOHIDAccessType`:
    /// 0 = granted, 1 = denied, 2 = unknown/undetermined.
    fn IOHIDCheckAccess(request_type: u32) -> u32;
}

const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;

/// Whether the process holds the Accessibility (AX) trust needed to post
/// synthetic events into other apps.
pub fn ax_trusted() -> bool {
    // SAFETY: nullary C call with no arguments and a `Boolean` return.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Whether secure event input is active anywhere in the system.
pub fn secure_input_enabled() -> bool {
    // SAFETY: nullary C call with no arguments and a `Boolean` return.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// Raw `kIOHIDAccessType` for listening to HID events (0/1/2 as documented).
pub fn input_monitoring_access() -> u32 {
    // SAFETY: single `u32` in, `u32` out; the request-type constant is valid.
    unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) }
}

/// Raw `AVAuthorizationStatus` for audio capture: 0 = not determined,
/// 1 = restricted, 2 = denied, 3 = authorized. Returns -1 if the AVFoundation
/// class is somehow unavailable.
pub fn microphone_authorization() -> isize {
    // `AVMediaTypeAudio` is the string constant "soun"; using its value keeps
    // us off an AVFoundation binding crate for a single class method call.
    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        return -1;
    };
    let media = NSString::from_str("soun");
    // SAFETY: `authorizationStatusForMediaType:` is a class method taking an
    // NSString and returning NSInteger; both types match.
    unsafe { msg_send![class, authorizationStatusForMediaType: &*media] }
}
