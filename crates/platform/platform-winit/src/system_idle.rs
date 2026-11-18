//! How long the computer has gone without any input, in any program.

use std::time::Duration;

/// How long since the last key, click or mouse move anywhere; `None`: this system doesn't say.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub(crate) fn system_idle() -> Option<Duration> {
    /// `kCGEventSourceStateCombinedSessionState`.
    const COMBINED_SESSION: i32 = 0;
    /// `kCGAnyInputEventType`.
    const ANY_INPUT: u32 = u32::MAX;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event: u32) -> f64;
    }
    // SAFETY: takes two plain values, reads the session's state, keeps nothing.
    let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION, ANY_INPUT) };
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

/// How long since the last key, click or mouse move anywhere; `None`: this system doesn't say.
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) fn system_idle() -> Option<Duration> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `info` is valid for the call and its size is set, as the call asks.
    if unsafe { GetLastInputInfo(&raw mut info) } == 0 {
        return None;
    }
    // SAFETY: no arguments, no state kept. The counters wrap together: the difference holds.
    let now = unsafe { GetTickCount() };
    Some(Duration::from_millis(u64::from(now.wrapping_sub(info.dwTime))))
}

/// How long since the last key, click or mouse move anywhere; `None`: this system doesn't say.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn system_idle() -> Option<Duration> {
    None
}
