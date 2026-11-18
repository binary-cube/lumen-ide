//! Windows: a power request (`PowerCreateRequest` + `PowerRequestSystemRequired`, and
//! `PowerRequestDisplayRequired` when the screen is kept on too), tied to its handle (not to
//! a thread), its reason shown by `powercfg /requests`.

#![allow(unsafe_code)]

use power::{Keep, PowerError, SleepHold};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Power::{
    POWER_REQUEST_TYPE, PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired,
    PowerRequestSystemRequired, PowerSetRequest,
};
use windows_sys::Win32::System::SystemServices::POWER_REQUEST_CONTEXT_VERSION;
use windows_sys::Win32::System::Threading::{
    POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};

/// A set request (what it's set for): cleared and closed when dropped.
#[derive(Debug)]
struct Request(HANDLE, Vec<POWER_REQUEST_TYPE>);

// SAFETY: the handle is only used to clear and close the request, from whichever thread drops it.
unsafe impl Send for Request {}

impl SleepHold for Request {}

impl Drop for Request {
    fn drop(&mut self) {
        // SAFETY: a request handle `PowerCreateRequest` gave and `PowerSetRequest` set; cleared
        // and closed once.
        unsafe {
            for kind in &self.1 {
                PowerClearRequest(self.0, *kind);
            }
            CloseHandle(self.0);
        }
    }
}

pub(crate) fn hold(reason: &str, keep: Keep) -> Result<Box<dyn SleepHold>, PowerError> {
    let mut text: Vec<u16> = reason.encode_utf16().chain(std::iter::once(0)).collect();
    let context = REASON_CONTEXT {
        Version: POWER_REQUEST_CONTEXT_VERSION,
        Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
        Reason: REASON_CONTEXT_0 { SimpleReasonString: text.as_mut_ptr() },
    };
    // SAFETY: `context` and its string live through the call (Windows copies the reason).
    let handle = unsafe { PowerCreateRequest(&context) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(PowerError::Failed("Windows refused the power request".into()));
    }
    let mut request = Request(handle, Vec::new());
    let kinds: &[POWER_REQUEST_TYPE] = if keep.screen {
        &[PowerRequestSystemRequired, PowerRequestDisplayRequired]
    } else {
        &[PowerRequestSystemRequired]
    };
    for kind in kinds {
        // SAFETY: a valid request handle.
        if unsafe { PowerSetRequest(request.0, *kind) } == 0 {
            return Err(PowerError::Failed("Windows refused to keep the system awake".into()));
        }
        request.1.push(*kind);
    }
    Ok(Box::new(request))
}
