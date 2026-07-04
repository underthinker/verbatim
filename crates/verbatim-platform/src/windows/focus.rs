//! `FocusTracker` for Windows: the foreground window's owning executable is
//! the per-OS stable app identifier, plus its window title (E7, per-app
//! profiles later).

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};
use windows::core::PWSTR;

use crate::errors::FocusError;
use crate::traits::FocusTracker;
use crate::types::FocusedApp;

/// Windows [`FocusTracker`]. Stateless; every call re-queries the foreground
/// window, so it is trivially `Send + Sync`.
#[derive(Default)]
pub struct WinFocusTracker;

impl WinFocusTracker {
    pub fn new() -> Self {
        Self
    }
}

impl FocusTracker for WinFocusTracker {
    fn focused_app(&self) -> Result<FocusedApp, FocusError> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return Err(FocusError::Unknown);
        }
        let app_id = executable_name(hwnd).ok_or(FocusError::Unknown)?;
        Ok(FocusedApp {
            app_id,
            window_title: window_title(hwnd),
        })
    }
}

/// The file name of the executable owning `hwnd` (e.g. `notepad.exe`).
fn executable_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    if unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) } == 0 || pid == 0 {
        return None;
    }
    let process: HANDLE =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    // The full image path varies per install; the file name is the stable id.
    path.rsplit(['\\', '/'])
        .next()
        .map(|name| name.to_ascii_lowercase())
}

fn window_title(hwnd: HWND) -> Option<String> {
    let mut buffer = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if len <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..len as usize]))
}
