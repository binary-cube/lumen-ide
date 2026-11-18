//! macOS: an IOKit power assertion that prevents the idle system sleep
//! (`kIOPMAssertPreventUserIdleSystemSleep`) — or, the screen kept on too, its idle sleep
//! (`kIOPMAssertPreventUserIdleDisplaySleep`, which holds the system's as well) — named by
//! its reason.

#![allow(unsafe_code)]

use std::ffi::c_void;

use power::{Keep, PowerError, SleepHold};

type CFStringRef = *const c_void;
type IOPMAssertionID = u32;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
const K_IO_RETURN_SUCCESS: i32 = 0;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        external: u8,
    ) -> CFStringRef;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        kind: CFStringRef,
        level: u32,
        name: CFStringRef,
        id: *mut IOPMAssertionID,
    ) -> i32;
    fn IOPMAssertionRelease(id: IOPMAssertionID) -> i32;
}

/// A Core Foundation string of `text` (released when dropped).
struct CfString(CFStringRef);

impl CfString {
    fn new(text: &str) -> Option<Self> {
        // SAFETY: the bytes are valid UTF-8 for `len` bytes; Core Foundation copies them.
        let s = unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                text.as_ptr(),
                text.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
                0,
            )
        };
        (!s.is_null()).then_some(Self(s))
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        // SAFETY: made by `CFStringCreateWithBytes` (owned, not released elsewhere).
        unsafe { CFRelease(self.0) }
    }
}

/// A held assertion: released when dropped.
#[derive(Debug)]
struct Assertion(IOPMAssertionID);

impl SleepHold for Assertion {}

impl Drop for Assertion {
    fn drop(&mut self) {
        // SAFETY: an id `IOPMAssertionCreateWithName` gave, released once.
        let code = unsafe { IOPMAssertionRelease(self.0) };
        if code != K_IO_RETURN_SUCCESS {
            tracing::warn!("the keep-awake assertion wasn't released (IOKit {code:#x})");
        }
    }
}

pub(crate) fn hold(reason: &str, keep: Keep) -> Result<Box<dyn SleepHold>, PowerError> {
    let failed = |what: &str| PowerError::Failed(format!("IOKit: {what}"));
    let kind =
        if keep.screen { "PreventUserIdleDisplaySleep" } else { "PreventUserIdleSystemSleep" };
    let kind = CfString::new(kind).ok_or_else(|| failed("a string"))?;
    let name = CfString::new(reason).ok_or_else(|| failed("a string"))?;
    let mut id: IOPMAssertionID = 0;
    // SAFETY: both strings live through the call; `id` is a valid place for the result.
    let code =
        unsafe { IOPMAssertionCreateWithName(kind.0, K_IOPM_ASSERTION_LEVEL_ON, name.0, &mut id) };
    if code != K_IO_RETURN_SUCCESS {
        return Err(failed(&format!("the assertion was refused ({code:#x})")));
    }
    Ok(Box::new(Assertion(id)))
}
