//! CEF's application: its command line (what makes Chromium lighter), its browser process
//! handler (CEF's work scheduled on our main loop), the app scheme registered in every
//! process, and its start.

use std::path::Path;

use cef::rc::Rc as _;
use cef::*;

use crate::pump::Pump;

/// Chromium's switches for pages of our own, few and trusted: less of everything it would
/// run for a browser (each one measured in plan 99, 99a).
const SWITCHES: &[&str] = &[
    // No window of its own, no dialog, no first-run work.
    "no-startup-window",
    "noerrdialogs",
    "no-first-run",
    "no-default-browser-check",
    // No keychain prompt on macOS (cookies kept in our profile only).
    "use-mock-keychain",
    // Nothing of a browser's own: extensions, sync, translation, components updated in the
    // background, background networking, spelling, crash reports.
    "disable-extensions",
    "disable-sync",
    "disable-component-update",
    "disable-background-networking",
    "disable-default-apps",
    "disable-breakpad",
    "disable-domain-reliability",
    "disable-client-side-phishing-detection",
    "disable-spell-checking",
    // Our pages are muted (none plays sound).
    "mute-audio",
];

/// Switches with a value.
const VALUES: &[(&str, &str)] = &[
    // Our pages share their renderers (draw.io, a response's HTML): fewer processes.
    ("renderer-process-limit", "2"),
    (
        "disable-features",
        "Translate,MediaRouter,OptimizationHints,AutofillServerCommunication,\
         InterestFeedContentSuggestions,CalculateNativeWinOcclusion,BackForwardCache",
    ),
];

/// The GPU Chromium draws on (the app's window's: a shared texture opens only on the GPU it
/// was made on), set once before CEF starts.
static ADAPTER: std::sync::OnceLock<web_engine_protocol::Adapter> = std::sync::OnceLock::new();

/// Chromium told to draw on `adapter` (before [`initialize`]).
pub fn draw_on(adapter: web_engine_protocol::Adapter) {
    let _ = ADAPTER.set(adapter);
}

/// The GPU it was told to draw on.
pub fn adapter() -> Option<web_engine_protocol::Adapter> {
    ADAPTER.get().copied()
}

#[derive(Debug, Clone)]
pub struct HostApp;

wrap_app! {
    pub struct AppBuilder {
        app: HostApp,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefStringUtf16>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(command_line) = command_line else { return };
            for switch in SWITCHES {
                command_line.append_switch(Some(&(*switch).into()));
            }
            for (switch, value) in VALUES {
                command_line.append_switch_with_value(Some(&(*switch).into()), Some(&(*value).into()));
            }
            // The GPU process is started with the browser's switches: told here, it opens
            // its device on the app's GPU (Windows: `<high>,<low>` of the adapter's LUID).
            if let Some(adapter) = ADAPTER.get() {
                command_line.append_switch_with_value(
                    Some(&"use-adapter-luid".into()),
                    Some(&adapter.switch_value().as_str().into()),
                );
            }
        }

        fn on_register_custom_schemes(&self, registrar: Option<&mut SchemeRegistrar>) {
            crate::scheme::declare(registrar);
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(BrowserHandlerBuilder::new(BrowserHandler))
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(PagesHandlerBuilder::new(PagesHandler))
        }
    }
}

/// A renderer's side: what our pages get from their host (`window.ipc`).
#[derive(Debug, Clone)]
pub struct PagesHandler;

wrap_render_process_handler! {
    pub struct PagesHandlerBuilder {
        handler: PagesHandler,
    }

    impl RenderProcessHandler {
        fn on_context_created(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            context: Option<&mut V8Context>,
        ) {
            crate::ipc::define(frame, context);
        }
    }
}

/// CEF's application (the same in every process).
pub fn build() -> cef::App {
    AppBuilder::new(HostApp)
}

#[derive(Debug, Clone)]
pub struct BrowserHandler;

wrap_browser_process_handler! {
    pub struct BrowserHandlerBuilder {
        handler: BrowserHandler,
    }

    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            Pump::schedule(delay_ms);
        }
    }
}

/// CEF started: windowless, its message loop ours (the [`Pump`] made before, which CEF's
/// schedule tells), its files in `profile` (the pages' storage: draw.io's own, kept between
/// runs). `false`: it didn't start.
pub fn initialize(args: &cef::args::Args, app: &mut cef::App, profile: &Path) -> bool {
    let profile: CefString = profile.to_string_lossy().as_ref().into();
    let settings = Settings {
        windowless_rendering_enabled: 1,
        external_message_pump: 1,
        // Where the system allows it (`sandbox.rs`).
        no_sandbox: i32::from(!crate::sandbox::wanted()),
        root_cache_path: profile.clone(),
        cache_path: profile,
        persist_session_cookies: 0,
        log_severity: LogSeverity::ERROR,
        // Nothing reachable from outside the process.
        remote_debugging_port: 0,
        ..Default::default()
    };
    cef::initialize(Some(args.as_main_args()), Some(&settings), Some(app), std::ptr::null_mut())
        == 1
}
