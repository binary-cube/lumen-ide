//! Windows: the frame's D3D11 shared texture handle duplicated into the app's process (its
//! process told in its greeting, which the host's server checked is the pipe's client: a
//! greeting naming another process never reaches here); the app opens it on its own device
//! and closes it once copied. A frame's message that never reaches the app (its app let
//! go, a queue full) closes its copy from here, in the app's process: no handle is left
//! there that nobody knows.

#![allow(unsafe_code)]

use std::sync::Arc;

use cef::AcceleratedPaintInfo;
use web_engine_protocol::{Hello, PageId, Texture};
use web_host_core::Undelivered;
use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE};

/// The app's process, opened to duplicate handles into it (and close them there); closed
/// once the last frame that may need it is done with.
struct AppProcess(HANDLE);

// SAFETY: a process handle is a kernel object's handle, usable from any thread; it's only
// closed once, as the last holder drops it.
unsafe impl Send for AppProcess {}
// SAFETY: as above: the handle is only read.
unsafe impl Sync for AppProcess {}

impl Drop for AppProcess {
    fn drop(&mut self) {
        // SAFETY: our handle to the app's process, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

pub struct Transfer {
    /// The app's process (`None`: it can't be opened: no frames).
    app: Option<Arc<AppProcess>>,
}

impl Transfer {
    pub fn to(hello: &Hello) -> Self {
        // SAFETY: opening the app's process by its id, only to duplicate handles into it (and
        // close the copies it never got).
        let app = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, hello.pid) };
        if app.is_null() {
            tracing::warn!("the app's process can't be opened for its frames");
            return Self { app: None };
        }
        Self { app: Some(Arc::new(AppProcess(app))) }
    }

    pub fn hand_over(
        &self,
        info: &AcceleratedPaintInfo,
        _page: PageId,
        _number: u64,
    ) -> Option<(Texture, Option<Undelivered>)> {
        let app = self.app.as_ref()?;
        if info.shared_texture_handle.is_null() {
            return None;
        }
        let mut theirs: HANDLE = std::ptr::null_mut();
        // SAFETY: CEF's handle, valid for the paint callback, duplicated into the app's
        // process (the app closes its copy).
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                info.shared_texture_handle,
                app.0,
                &raw mut theirs,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            return None;
        }
        // The handle's value in the app's process (a number there, not one of ours).
        let value = theirs as usize;
        let app = Arc::clone(app);
        let undelivered = Undelivered::new(move || close_in(&app, value));
        Some((Texture::Windows { handle: value as u64 }, Some(undelivered)))
    }
}

/// The copy `value` closed in the app's process (its message never reached the app: nobody
/// there knows it).
fn close_in(app: &AppProcess, value: usize) {
    // SAFETY: `value` is the handle duplicated into the app's process for a frame whose
    // message was never written to it, so the app never saw (nor closed) it; closing it as the
    // source of a duplication with no target only closes it there.
    let ok = unsafe {
        DuplicateHandle(
            app.0,
            value as HANDLE,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            DUPLICATE_CLOSE_SOURCE,
        )
    };
    if ok == 0 {
        tracing::debug!("a frame's handle the app never got wasn't closed");
    }
}
