//! The whole way with the real engine (macOS): the app starts `web-host`, opens a page, asks
//! for frames, gets each one's surface through its Mach service, and gives it back.
//!
//! Compiled on every system (a test of all of them goes here); the engine's own only on
//! macOS. Ignored unless asked: it needs the engine built and bundled. Run it with
//! `EDITOR_WEB_HOST=<…>/web-host.app/Contents/MacOS/web-host cargo test -p web-engine -- --ignored`.

#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::{Mutex, mpsc};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use web_engine_protocol::{Event, Size, Source};
#[cfg(target_os = "macos")]
use web_host_core::channel::Channel;

#[cfg(target_os = "macos")]
use crate::{Connection, Launch, PageEvent, PageSpec, WebPage};

#[cfg(target_os = "macos")]
#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceLookupFromMachPort(port: u32) -> *mut std::ffi::c_void;
    fn IOSurfaceGetWidth(surface: *mut std::ffi::c_void) -> usize;
    fn IOSurfaceGetHeight(surface: *mut std::ffi::c_void) -> usize;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *mut std::ffi::c_void);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs the engine bundled (EDITOR_WEB_HOST)"]
fn a_page_drawn_by_the_real_engine_reaches_the_app() {
    let Ok(program) = std::env::var("EDITOR_WEB_HOST") else { return };
    // SAFETY: set before any thread of this test reads the environment.
    unsafe { std::env::set_var("EDITOR_WEB_HOST_IDLE_SECS", "3") };
    let dir = std::env::temp_dir().join(format!("we-e2e-{}", std::process::id()));
    let channel = Channel { name: format!("web-host-e2e-{}", std::process::id()), dir };
    let started = Instant::now();
    let profile = channel.dir.join("profile");
    let launch = Launch {
        program: program.into(),
        channel: channel.clone(),
        profile,
        hold: None,
        adapter: None,
    };
    let connection =
        Arc::new(Connection::establish(&channel, None, || Some(launch)).expect("joined"));
    let joined = started.elapsed();

    let (tx, frames) = mpsc::channel::<(u32, u32, usize, usize)>();
    let tx = Mutex::new(tx);
    let sink: crate::Sink = Arc::new(move |event| {
        if let PageEvent::Frame(frame) = event {
            let crate::Native::Surface(right) = &frame.native else {
                panic!("a GPU texture asked, pixels came");
            };
            // SAFETY: the surface's port, ours for the frame; the surface released after.
            let (w, h) = unsafe {
                let surface = IOSurfaceLookupFromMachPort(right.port());
                assert!(!surface.is_null(), "the surface is there");
                let size = (IOSurfaceGetWidth(surface), IOSurfaceGetHeight(surface));
                CFRelease(surface);
                size
            };
            let _ = tx.lock().unwrap().send((frame.width, frame.height, w, h));
        } else if let PageEvent::Event(Event::Crashed { .. }) = event {
            panic!("the engine crashed");
        }
    });
    let spec = PageSpec {
        source: Source::Html("<body style='background:#c33;margin:0'>hello</body>".into()),
        size: Size { width: 400, height: 200, scale: 2.0 },
        hidden: false,
        background: [255; 4],
        pixels: None,
    };
    let page = WebPage::open(Arc::clone(&connection), spec, sink);
    let opened = Instant::now();
    let frame = loop {
        page.begin_frame();
        if let Ok(frame) = frames.recv_timeout(Duration::from_millis(16)) {
            break frame;
        }
        assert!(opened.elapsed() < Duration::from_secs(20), "no frame came");
    };
    eprintln!(
        "joined in {joined:?}, first frame {:?} after the page opened: {frame:?}",
        opened.elapsed()
    );
    assert_eq!((frame.0, frame.1), (400, 200), "the page's pixels");
    assert_eq!((frame.2, frame.3), (400, 200), "the surface's");
    drop(page);
}
