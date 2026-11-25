//! macOS: CEF's framework loaded (it's beside the host in its bundle, not linked), and the
//! application object CEF needs (`NSApplication` answering `CrAppControlProtocol`): no
//! window, no Dock icon, no menu — the host is never seen.

use std::cell::Cell;

use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

/// The framework, loaded for as long as this lives (`helper`: the helper's place in the
/// bundle, three folders deeper).
pub fn load_framework(helper: bool) -> Option<cef::library_loader::LibraryLoader> {
    let exe = std::env::current_exe().ok()?;
    let loader = cef::library_loader::LibraryLoader::new(&exe, helper);
    loader.load().then_some(loader)
}

/// The application's state CEF asks about.
#[derive(Debug, Default)]
pub struct HostApplicationIvars {
    handling_send_event: Cell<bool>,
}

define_class! {
    #[unsafe(super(NSApplication))]
    #[thread_kind = MainThreadOnly]
    #[ivars = HostApplicationIvars]
    pub struct HostApplication;

    unsafe impl NSObjectProtocol for HostApplication {}

    unsafe impl CrAppProtocol for HostApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn is_handling_send_event(&self) -> Bool {
            Bool::new(self.ivars().handling_send_event.get())
        }
    }

    unsafe impl CrAppControlProtocol for HostApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_handling_send_event(&self, handling: Bool) {
            self.ivars().handling_send_event.set(handling.as_bool());
        }
    }

    unsafe impl CefAppProtocol for HostApplication {}
}

/// The application made before CEF starts, never shown.
pub fn make_application() {
    // Only on the main thread (where AppKit's application lives).
    if MainThreadMarker::new().is_none() {
        return;
    }
    // SAFETY: `sharedApplication` on our `NSApplication` subclass, on the main thread,
    // before anything made the app's application: it becomes that one.
    #[allow(unsafe_code)]
    let app: Retained<NSApplication> =
        unsafe { msg_send![HostApplication::class(), sharedApplication] };
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
}
