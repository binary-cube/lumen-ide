//! A page shown by the system's web engine over the window (WKWebView on macOS, WebView2 on
//! Windows), through `wry`: a child view of the window at the bounds the app gives, gone
//! when it's dropped. Hidden, it gives the keyboard back to the window. Two kinds:
//!
//! - **HTML given** (a response's): its scripts don't run (a response's page can't call out
//!   from the app, to the user's own network either), a link followed inside it isn't
//!   (nothing navigates away from what the app showed), it keeps nothing (incognito).
//! - **A trusted app** (a folder on this computer, [`web_app`]): its files served through
//!   the app's own scheme by a worker (`web-app`), its scripts running under a policy that
//!   keeps it off the network, going nowhere but its own files, opening no window (a link
//!   it wants opened is told to the host), downloading nothing. Its messages come through
//!   the engine's IPC to the inbox given (the window asked for a frame after each); the
//!   host's go in as a call to the page's `window.__hostReceive`.
//!
//! Neither has developer tools.

use std::rc::Rc;
use std::sync::Arc;

use kernel::Rect;
use platform_api::{WebEvent, WebInbox, WebSource};

use crate::web_app;

/// What the web engine shows for the app.
pub(crate) struct WryWebView {
    view: wry::WebView,
    /// An app's page (its scripts run: it can be sent messages).
    app: bool,
    /// Its window: asked for a frame when a picture of the page is ready.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    window: Arc<winit::window::Window>,
    /// Where it is (physical pixels, in the window): its holes' places are relative to it.
    area: std::cell::Cell<Rect>,
    /// What floats over it shows through (macOS, Windows).
    holes: Option<crate::web_holes::Holes>,
    /// Where the engine keeps its files (WebView2's user data): kept while the page is,
    /// let go after it (the fields' order).
    _context: wry::WebContext,
}

impl std::fmt::Debug for WryWebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WryWebView").finish_non_exhaustive()
    }
}

fn bounds_of(r: Rect) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::PhysicalPosition::new(r.min_x().round(), r.min_y().round()).into(),
        size: wry::dpi::PhysicalSize::new(
            r.size.width.max(1.0).round(),
            r.size.height.max(1.0).round(),
        )
        .into(),
    }
}

impl WryWebView {
    /// `source` shown over `window` at `bounds` (`None`: no engine, or it couldn't start:
    /// said in the log). `inbox`: where an app's events go.
    pub(crate) fn new(
        window: &Arc<winit::window::Window>,
        source: WebSource<'_>,
        bounds: Rect,
        data: &std::path::Path,
        inbox: WebInbox,
    ) -> Option<Self> {
        let mut context = wry::WebContext::new(Some(data.to_path_buf()));
        let builder = wry::WebViewBuilder::new_with_web_context(&mut context)
            .with_bounds(bounds_of(bounds))
            .with_devtools(false)
            .with_incognito(true);
        let app = matches!(source, WebSource::App { .. });
        let built = match source {
            WebSource::Html(html) => builder
                .with_html(html)
                .with_javascript_disabled()
                // The page given is loaded (`about:blank`, `data:` as the engine puts it); a
                // link clicked in it goes nowhere.
                .with_navigation_handler(|url| {
                    url.starts_with("about:") || url.starts_with("data:")
                })
                .build_as_child(window.as_ref()),
            WebSource::App { root, entry } => {
                app_page(builder, window, root, entry, inbox).build_as_child(window.as_ref())
            }
        };
        match built {
            Ok(view) => {
                #[cfg(target_os = "macos")]
                above_the_app(&view);
                #[cfg(target_os = "macos")]
                let holes = {
                    use wry::WebViewExtMacOS;
                    let page: objc2::rc::Retained<objc2::runtime::AnyObject> =
                        objc2::rc::Retained::into_super(objc2::rc::Retained::into_super(
                            objc2::rc::Retained::into_super(view.webview()),
                        ))
                        .into();
                    crate::web_holes::Holes::new(window, page)
                };
                #[cfg(target_os = "windows")]
                let holes = {
                    use wry::WebViewExtWindows;
                    Some(crate::web_holes::Holes::new(view.hwnd().0 as isize))
                };
                Some(Self {
                    view,
                    app,
                    window: Arc::clone(window),
                    area: std::cell::Cell::new(bounds),
                    holes,
                    _context: context,
                })
            }
            Err(e) => {
                tracing::warn!("the system's web engine didn't start: {e}");
                None
            }
        }
    }
}

/// The page's layer kept above the app's own image (macOS): the window's Metal layer is a
/// sublayer of the same view, and one made again (a window given its images back) is added
/// on top of everything — the page would be covered, showing only what the app drew under
/// it. A higher `zPosition` keeps it above whatever order the layers come in.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn above_the_app(view: &wry::WebView) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use wry::WebViewExtMacOS;
    let page = view.webview();
    // SAFETY: `-[NSView setWantsLayer:]`, `-[NSView layer]` (an optional `CALayer`) and
    // `-[CALayer setZPosition:]` (a `CGFloat`) are called with their declared types, on the
    // main thread the page was just made on.
    unsafe {
        let _: () = objc2::msg_send![&*page, setWantsLayer: true];
        let layer: Option<Retained<AnyObject>> = objc2::msg_send![&*page, layer];
        if let Some(layer) = layer {
            let _: () = objc2::msg_send![&*layer, setZPosition: 1.0_f64];
        }
    }
}

/// `builder` made into an app's page: `entry` of `root`, served by the app's scheme.
fn app_page<'a>(
    builder: wry::WebViewBuilder<'a>,
    window: &Arc<winit::window::Window>,
    root: &std::path::Path,
    entry: &str,
    inbox: WebInbox,
) -> wry::WebViewBuilder<'a> {
    let inbox = Rc::new(inbox);
    let root: Arc<std::path::Path> = Arc::from(root);
    let builder = builder
        .with_url(web_app::entry_url(entry))
        .with_clipboard(true)
        .with_asynchronous_custom_protocol(web_app::SCHEME.to_owned(), move |_, request, reply| {
            // Read on a worker: an app asks for hundreds of files as it starts.
            let root = Arc::clone(&root);
            let method = request.method().as_str().to_owned();
            let host = request.uri().host().map(str::to_owned);
            let path = request.uri().path().to_owned();
            kernel::work::run("web-app", move || {
                let served = web_app::serve(&root, &method, host.as_deref(), &path);
                reply.respond(response_of(served));
            });
        })
        .with_navigation_handler(|url| web_app::allows_navigation(&url))
        .with_new_window_req_handler({
            let inbox = Rc::clone(&inbox);
            let window = Arc::clone(window);
            move |url, _| {
                inbox(WebEvent::OpenLink(url));
                window.request_redraw();
                wry::NewWindowResponse::Deny
            }
        })
        .with_download_started_handler(|url, _| {
            tracing::debug!("an app's download refused: {url}");
            false
        })
        .with_ipc_handler({
            let window = Arc::clone(window);
            move |request: wry::http::Request<String>| {
                let from = request.uri().to_string();
                if !web_app::allows_navigation(&from) {
                    tracing::warn!("a message from outside the app dropped: {from}");
                    return;
                }
                let message = request.into_body();
                if message.len() > platform_api::MAX_WEB_MESSAGE {
                    tracing::warn!("an app's message too long, dropped: {} bytes", message.len());
                    return;
                }
                inbox(WebEvent::Message(message));
                window.request_redraw();
            }
        });
    // WebView2 serves the scheme as `https://local-app.localhost/`: a secure context, as
    // WebKit's own scheme is.
    #[cfg(target_os = "windows")]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_https_scheme(true)
    };
    builder
}

/// What the engine is answered with for a request to an app's scheme.
fn response_of(served: web_app::Served) -> wry::http::Response<Vec<u8>> {
    let mut response = wry::http::Response::builder()
        .status(served.status)
        .header(wry::http::header::CONTENT_TYPE, served.mime)
        .header("X-Content-Type-Options", "nosniff");
    if served.policy {
        response = response.header("Content-Security-Policy", web_app::CSP);
    }
    response.body(served.body).unwrap_or_else(|e| {
        tracing::warn!("an app's answer wasn't made: {e}");
        let mut empty = wry::http::Response::new(Vec::new());
        *empty.status_mut() = wry::http::StatusCode::INTERNAL_SERVER_ERROR;
        empty
    })
}

impl platform_api::WebView for WryWebView {
    fn set_bounds(&self, bounds: Rect) {
        if let Err(e) = self.view.set_bounds(bounds_of(bounds)) {
            tracing::debug!("the page wasn't moved: {e}");
        }
        self.area.set(bounds);
        if let Some(holes) = &self.holes {
            holes.moved(bounds);
        }
    }

    fn set_holes(&self, holes: &[(Rect, f32)]) {
        if let Some(h) = &self.holes {
            h.set(holes, self.area.get());
        }
    }

    fn set_visible(&self, visible: bool) {
        if let Err(e) = self.view.set_visible(visible) {
            tracing::debug!("the page wasn't shown or hidden: {e}");
        }
        if !visible {
            self.release_focus();
        }
    }

    fn release_focus(&self) {
        if let Err(e) = self.view.focus_parent() {
            tracing::debug!("the keyboard wasn't given back: {e}");
        }
    }

    fn load_html(&self, html: &str) {
        if self.app {
            tracing::warn!("an app's page keeps its own; no HTML loaded in its place");
            return;
        }
        if let Err(e) = self.view.load_html(html) {
            tracing::warn!("the page wasn't loaded: {e}");
        }
    }

    fn send(&self, message: &str) {
        if !self.app {
            return;
        }
        if let Err(e) = self.view.evaluate_script(&web_app::receive_script(message)) {
            tracing::warn!("a message wasn't given to the app's page: {e}");
        }
    }

    #[cfg(target_os = "macos")]
    fn snapshot(&self, done: Box<dyn FnOnce(Option<platform_api::WebPicture>)>) -> bool {
        snapshot::take(&self.view, Arc::clone(&self.window), done)
    }
}

/// A page's picture (macOS): WebKit's snapshot of what it shows, drawn into RGBA pixels of
/// the page's size on screen.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod snapshot {
    use std::cell::Cell;
    use std::sync::Arc;

    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use platform_api::WebPicture;
    use wry::WebViewExtMacOS;

    /// The largest picture taken (pixels a side): a page past it isn't pictured.
    const MAX_SIDE: f64 = 16_384.0;

    type Done = Box<dyn FnOnce(Option<WebPicture>)>;

    /// Asks WebKit for the page's picture; `done` gets it on the main thread.
    pub(super) fn take(
        view: &wry::WebView,
        window: Arc<winit::window::Window>,
        done: Done,
    ) -> bool {
        let page = view.webview();
        let scale = window.scale_factor();
        // SAFETY: `-[NSView bounds]` returns an `NSRect`, called on the main thread.
        let bounds: NSRect = unsafe { objc2::msg_send![&*page, bounds] };
        let width = (bounds.size.width * scale).round();
        let height = (bounds.size.height * scale).round();
        if !(1.0..=MAX_SIDE).contains(&width) || !(1.0..=MAX_SIDE).contains(&height) {
            return false;
        }
        let (width, height) = (width as u32, height as u32);
        // Called once (WebKit's promise); a `Cell` lets the `Fn` block hand `done` out.
        let done = Cell::new(Some(done));
        let handler = RcBlock::new(move |image: *mut NSImage, _error: *mut AnyObject| {
            let Some(done) = done.take() else { return };
            // SAFETY: WebKit passes a valid (autoreleased) image, or nil on an error; it is
            // only read during this call, on the main thread.
            let image = unsafe { image.as_ref() };
            done(image.and_then(|image| pixels_of(image, width, height)));
            window.request_redraw();
        });
        // SAFETY: `-[WKWebView takeSnapshotWithConfiguration:completionHandler:]` with a nil
        // configuration (its bounds, at the screen's scale) and a block of the declared type
        // `void (^)(NSImage *, NSError *)`; WebKit copies the block and calls it once on the
        // main thread.
        unsafe {
            let none: *const AnyObject = std::ptr::null();
            let _: () = objc2::msg_send![
                &*page,
                takeSnapshotWithConfiguration: none,
                completionHandler: &*handler
            ];
        }
        true
    }

    /// `image` drawn into `width` × `height` RGBA pixels (straight alpha, rows top down).
    fn pixels_of(image: &NSImage, width: u32, height: u32) -> Option<WebPicture> {
        let (w, h) = (width as isize, height as isize);
        // SAFETY: no planes given: the rep allocates its own buffer of `h` rows of `w * 4`
        // bytes, read below with exactly that length.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                w,
                h,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                w * 4,
                32,
            )
        }?;
        let class = objc2::class!(NSGraphicsContext);
        // SAFETY: `+[NSGraphicsContext graphicsContextWithBitmapImageRep:]` (an optional
        // context drawing into `rep`), `+saveGraphicsState`, `+setCurrentContext:`,
        // `+restoreGraphicsState` and `-[NSImage drawInRect:]` with their declared types, on
        // the main thread; the context is current only for the draw.
        let drawn = unsafe {
            let context: Option<Retained<AnyObject>> =
                objc2::msg_send![class, graphicsContextWithBitmapImageRep: &*rep];
            match context {
                Some(context) => {
                    let _: () = objc2::msg_send![class, saveGraphicsState];
                    let _: () = objc2::msg_send![class, setCurrentContext: &*context];
                    let all = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w as f64, h as f64));
                    let _: () = objc2::msg_send![image, drawInRect: all];
                    let _: () = objc2::msg_send![&*context, flushGraphics];
                    let _: () = objc2::msg_send![class, restoreGraphicsState];
                    true
                }
                None => false,
            }
        };
        let data = rep.bitmapData();
        if !drawn || data.is_null() {
            return None;
        }
        let len = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        // SAFETY: `data` is the rep's own buffer of `len` bytes (asked above), alive while
        // `rep` is.
        let mut rgba = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
        // The rep's colors are premultiplied by alpha; a page is mostly opaque.
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = u16::from(p[3]);
            if a != 0 && a != 255 {
                for c in &mut p[..3] {
                    *c = ((u16::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Some(WebPicture { width, height, rgba })
    }
}
