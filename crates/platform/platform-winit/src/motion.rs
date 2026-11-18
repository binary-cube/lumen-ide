//! What the system says of motion and power: "reduce motion" (macOS' accessibility
//! setting, Windows' "Animation effects" off) and saving power (macOS' Low Power Mode,
//! Windows' battery saver). Each is one quick system call; callers ask now and then, not on
//! every frame.

/// The user asked the system for less motion.
pub fn reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        workspace.accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::BOOL;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut animate: BOOL = 1;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL through the pointer, which
        // points at a live local of that type.
        #[allow(unsafe_code)]
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut animate).cast(), 0)
        };
        ok != 0 && animate == 0
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// The system is saving power (the battery saver / Low Power Mode is on).
pub fn power_saving() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_foundation::NSProcessInfo::processInfo().isLowPowerModeEnabled()
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        // SAFETY: a plain C struct, all zeros is a valid value.
        #[allow(unsafe_code)]
        let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        // SAFETY: the pointer is to a live, writable SYSTEM_POWER_STATUS.
        #[allow(unsafe_code)]
        let ok = unsafe { GetSystemPowerStatus(&raw mut status) };
        // `SystemStatusFlag` 1: the battery saver is on.
        ok != 0 && status.SystemStatusFlag == 1
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}
