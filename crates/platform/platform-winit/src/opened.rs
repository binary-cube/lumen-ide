//! Files the system asks the app to open (macOS): Finder's "Open With", a file dropped on
//! the Dock's icon, a document double-clicked once the app is its editor. They come as
//! `application:openURLs:` on the application's delegate (winit's, which has none: added
//! here, as the Dock's menu is), before the first window too; their paths go into the loop
//! as an event, for the app (`Application::paths_opened`).

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;
use winit::event_loop::EventLoopProxy;

use crate::UserEvent;

/// Where the paths are sent (the loop's proxy, set before it runs).
static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();

/// Listens for the files the system opens with the app, sending them through `proxy`.
/// Called once the application's delegate exists (the loop built), before it runs.
pub(crate) fn listen(proxy: EventLoopProxy<UserEvent>) {
    let _ = PROXY.set(Mutex::new(proxy));
    if !install() {
        tracing::warn!("the files opened with the app can't be received (no delegate)");
    }
}

/// `application:openURLs:` on the delegate: the URLs' paths sent to the loop.
extern "C-unwind" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    urls: *mut AnyObject,
) {
    // SAFETY: `urls` is the `NSArray<NSURL *> *` AppKit passes; `count`, `objectAtIndex:`,
    // `isFileURL` and `path` are called with their declared types on the main thread.
    #[allow(unsafe_code)]
    let paths: Vec<PathBuf> = unsafe {
        let Some(urls) = urls.as_ref() else { return };
        let count: usize = msg_send![urls, count];
        (0..count)
            .filter_map(|i| {
                let url: *mut AnyObject = msg_send![urls, objectAtIndex: i];
                let url = url.as_ref()?;
                let file: bool = msg_send![url, isFileURL];
                if !file {
                    return None;
                }
                let path: *mut NSString = msg_send![url, path];
                Some(PathBuf::from(path.as_ref()?.to_string()))
            })
            .collect()
    };
    if paths.is_empty() {
        return;
    }
    tracing::info!(paths = paths.len(), "files opened with the app");
    if let Some(proxy) = PROXY.get().and_then(|p| p.lock().ok()) {
        let _ = proxy.send_event(UserEvent::PathsOpened(paths));
    }
}

/// Adds the method to the delegate's class. `false` if there's no delegate yet.
fn install() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else { return false };
    let object: &AnyObject = delegate.as_ref();
    let class: *const AnyClass = object.class();
    // SAFETY: the class is the live delegate's; the function matches the method's type
    // (`-(void)application:(NSApplication *)app openURLs:(NSArray<NSURL *> *)urls`: object,
    // selector, object, object → void), which the encoding says.
    #[allow(unsafe_code)]
    let added = unsafe {
        let imp: objc2::runtime::Imp = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            objc2::runtime::Imp,
        >(open_urls);
        objc2::ffi::class_addMethod(
            class.cast_mut(),
            sel!(application:openURLs:),
            imp,
            c"v@:@@".as_ptr(),
        )
    };
    if !added.as_bool() {
        tracing::warn!("the files opened with the app can't be received (the delegate has it)");
    }
    true
}
