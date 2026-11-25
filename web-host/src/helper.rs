//! macOS: CEF's subprocesses (a page's renderer, the GPU process…) as the helper apps of
//! the host's bundle. Each enters Chromium's sandbox first (a renderer reads a response's
//! HTML: it can touch nothing of the user's then), loads the framework from its place and
//! runs what CEF asks.

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    let args = cef::args::Args::new();
    // Before anything else, as CEF's own helpers do.
    let _sandbox = match web_host::sandbox::wanted() {
        true => match sandbox::Sandbox::enter(&args) {
            Some(entered) => Some(entered),
            None => return std::process::ExitCode::FAILURE,
        },
        false => None,
    };
    let Some(_framework) = web_host::mac_app::load_framework(true) else {
        return std::process::ExitCode::FAILURE;
    };
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let mut app = web_host::app::build();
    let code =
        cef::execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    std::process::ExitCode::from(u8::try_from(code.max(0)).unwrap_or(1))
}

/// Elsewhere the host itself runs the subprocesses.
#[cfg(not(target_os = "macos"))]
fn main() {}

/// macOS: Chromium's sandbox for a helper (`libcef_sandbox.dylib`, in the framework beside
/// it), held while the helper runs.
#[cfg(target_os = "macos")]
mod sandbox {
    #![allow(unsafe_code)]

    use std::ffi::c_void;

    /// The library, from a helper's program (`<host>.app/Contents/Frameworks/<helper>.app/
    /// Contents/MacOS/<helper>`).
    const LIBRARY: &str =
        "../../../Chromium Embedded Framework.framework/Libraries/libcef_sandbox.dylib";

    pub struct Sandbox {
        library: libloading::Library,
        context: *mut c_void,
    }

    impl Sandbox {
        /// The sandbox entered for this process (`None`: it couldn't be: the helper ends).
        pub fn enter(args: &cef::args::Args) -> Option<Self> {
            let exe = std::env::current_exe().ok()?;
            let path = exe.parent()?.join(LIBRARY).canonicalize().ok()?;
            // SAFETY: CEF's own sandbox library, loaded once, its documented entry point
            // called with the process's arguments.
            unsafe {
                let library = libloading::Library::new(path).ok()?;
                let main = args.as_main_args();
                let initialize = library
                    .get::<unsafe extern "C" fn(i32, *mut *mut std::ffi::c_char) -> *mut c_void>(
                        b"cef_sandbox_initialize",
                    )
                    .ok()?;
                let context = initialize(main.argc, main.argv);
                (!context.is_null()).then_some(Self { library, context })
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            // SAFETY: the context `cef_sandbox_initialize` gave, destroyed once.
            unsafe {
                if let Ok(destroy) =
                    self.library.get::<unsafe extern "C" fn(*mut c_void)>(b"cef_sandbox_destroy")
                {
                    destroy(self.context);
                }
            }
        }
    }
}
