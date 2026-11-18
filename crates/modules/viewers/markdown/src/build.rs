//! The layout built off the UI thread: a ~500 KB document takes ~55 ms to lay out again
//! after an edit (~160 ms the first time, release build), too long for a frame. The build
//! runs on a pool of the module's own with a text system of its own (the system's fonts
//! and the app's locale, as the UI's: the same widths), shared by every preview and let go
//! with the last; the layout's caches go with it and come back with the layout. The view
//! keeps drawing the layout it has meanwhile, and the line at its top stays there when the
//! new one is put in place. One build at a time per view: what changed meanwhile is built
//! next, from the latest text.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use text_layout::TextSystem;
use ui_components::Palette;

use crate::layout::{Caches, Layout, Page};

/// The threads building layouts.
pub const LAYOUT_THREAD: &str = "markdown-layout";

/// A document this small is laid out right away when there's nothing to show yet (no
/// empty frame on opening; ~2 ms).
pub const FIRST_INLINE: usize = 16 * 1024;

/// The text system layouts are shaped with on the workers.
pub type LayoutText = Arc<Mutex<TextSystem>>;

/// The layouts' text system: the one alive, or made (the system's fonts listed once per
/// process) if none is.
fn layout_text() -> LayoutText {
    static SHARED: Mutex<Weak<Mutex<TextSystem>>> = Mutex::new(Weak::new());
    let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(text) = shared.upgrade() {
        return text;
    }
    let text = Arc::new(Mutex::new(TextSystem::with_locale(&i18n::locale())));
    *shared = Arc::downgrade(&text);
    text
}

/// The pool the layouts are built on (threads only while there's work).
fn pool() -> kernel::Pool {
    static POOL: std::sync::OnceLock<kernel::Pool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| kernel::Pool::new(LAYOUT_THREAD, 2, kernel::work::IDLE)).clone()
}

/// What a layout is built for: the text's version, the width, the scale, how code wraps,
/// the colors.
#[derive(Clone, PartialEq)]
pub struct Want {
    pub version: u64,
    pub width: f32,
    pub scale: f32,
    pub wrap: bool,
    pub palette: Palette,
}

/// A layout built, with the caches it took and the text system it was shaped with.
pub struct Built {
    pub layout: Layout,
    pub caches: Caches,
    pub text: Option<LayoutText>,
}

/// A build on its way: what it's for, and where it arrives.
pub struct Building {
    pub want: Want,
    pub rx: Receiver<Built>,
}

/// What a build reads: the text, the folder its images are relative to, the folds turned.
pub struct Input {
    pub source: String,
    pub dir: PathBuf,
    pub toggled: HashSet<usize>,
}

/// `input` laid out for `want` on the pool, with `caches` (given back with the layout);
/// `wake` once it's there. A build that panics gives nothing back (the channel closes).
pub fn start(
    want: Want,
    input: Input,
    mut caches: Caches,
    text: Option<LayoutText>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Building {
    let (tx, rx) = std::sync::mpsc::channel();
    let job = want.clone();
    pool().spawn(kernel::Priority::Interactive, None, move || {
        let text = text.unwrap_or_else(layout_text);
        let layout = {
            let mut system = text.lock().unwrap_or_else(PoisonError::into_inner);
            let page = Page {
                source: &input.source,
                version: job.version,
                width: job.width,
                scale: job.scale,
                dir: &input.dir,
                toggled: &input.toggled,
            };
            Layout::build(page, &mut caches, &mut system, &job.palette)
        };
        let _ = tx.send(Built { layout, caches, text: Some(text) });
        if let Some(wake) = wake {
            wake();
        }
    });
    Building { want, rx }
}
