//! macOS: the frame's `IOSurface` as a Mach port, sent to the app's service (looked up from
//! its greeting) with the page and the frame's number; the app takes the surface from it.

#![allow(unsafe_code)]

use cef::AcceleratedPaintInfo;
use web_engine_protocol::{Hello, PageId, Texture};
use web_host_core::Undelivered;

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    /// A send right to the surface (the caller owns it).
    fn IOSurfaceCreateMachPort(surface: *mut std::ffi::c_void) -> u32;
}

pub struct Transfer {
    /// The app's service (`None`: it gave none, or it can't be reached: no frames).
    service: Option<mach_ports::Outbox>,
}

impl Transfer {
    pub fn to(hello: &Hello) -> Self {
        let service = hello.mach_service.as_deref().and_then(|name| {
            mach_ports::Outbox::look_up(name)
                .inspect_err(
                    |e| tracing::warn!(error = %e, "the app's frame service can't be reached"),
                )
                .ok()
        });
        Self { service }
    }

    pub fn hand_over(
        &self,
        info: &AcceleratedPaintInfo,
        page: PageId,
        number: u64,
    ) -> Option<(Texture, Option<Undelivered>)> {
        let service = self.service.as_ref()?;
        if info.shared_texture_io_surface.is_null() {
            return None;
        }
        // SAFETY: CEF's surface, valid for the paint callback; the right made is ours.
        let port = unsafe { IOSurfaceCreateMachPort(info.shared_texture_io_surface) };
        if port == 0 {
            return None;
        }
        let right = mach_ports::Right::adopt(port);
        match service.send(right, page.0, number) {
            // The right went to the app's port: it's the app's whatever becomes of the message.
            Ok(()) => Some((Texture::MachPort, None)),
            Err(e) => {
                tracing::debug!(error = %e, "a frame's surface wasn't sent");
                None
            }
        }
    }
}
