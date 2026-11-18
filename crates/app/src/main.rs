//! Composition root: wires together all contexts, modules and drivers.
//!
//! Usage: `lumen [folder] [file...]`. A folder opens as a project, each file in a tab.
//!
//! `lumen --snapshot <output.png> [paths...]` renders the first screen into a PNG, without
//! a window (see the `snapshot` module for the `EDITOR_SNAPSHOT_*` variables); only built
//! with the `snapshot` feature.
//! `EDITOR_NO_SPLASH=1` disables the splash screen at startup.
//!
//! Environment variables: `EDITOR_GPU_API=auto|metal|dx12|vulkan|gl`, `EDITOR_VSYNC=0`,
//! `RUST_LOG=debug`. Diagnostics: `EDITOR_DEBUG_RESIZE=<n>` resizes the window `n` times,
//! logs p50 / p95 / max frame durations and quits the app.

// On Windows, the release build is a GUI app (no console window).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod autosave;
mod backdrop;
mod caret;
mod commands;
mod config;
mod file_settings;
mod final_newline;
mod floating;
mod frameworks;
mod icon_style;
mod insight;
mod instance;
mod keymaps;
mod language;
mod left_behind;
mod levels;
mod module_manager;
mod modules;
mod over;
mod peers;
mod ports;
mod project_disk;
mod queries;
mod recent;
mod refactor_ports;
mod resources;
mod screen_mark;
mod scrollbars;
mod sessions;
mod settings;
#[cfg(feature = "snapshot")]
mod snapshot;
mod snippets;
mod split_tree;
mod startup;
mod symbols;
mod theme_options;
mod themes;
mod toasts;
mod updating;
mod watchdog;
mod window_menu;
mod windows;
mod workbench;

use std::path::Path;

use anyhow::Context as _;
use platform_api::{Platform, WindowOptions};
use platform_winit::WinitPlatform;
use render_api::{GpuApi, RenderOptions, RendererFactory as _};
use text_layout::{FontFamily, TextSystem};

use crate::themes::Themes;
use crate::workbench::Workbench;

/// Preferred code fonts, in order; the first installed one wins.
const CODE_FONTS: &[&str] =
    &["JetBrains Mono", "SF Mono", "Menlo", "Cascadia Mono", "Consolas", "DejaVu Sans Mono"];

/// The log file (in the config directory), cleared when it exceeds 5 MB.
fn log_file() -> Option<std::fs::File> {
    let dir = config::config_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}.log", kernel::brand::ID));
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > 5 * 1024 * 1024);
    std::fs::OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(path)
        .ok()
}

/// On Windows, mimalloc for every allocation: Windows' own heap returns freed memory poorly
/// and fragments with an editor's many small, short-lived allocations. Elsewhere the
/// system's allocator stays: measured on macOS (a 15,000-file PHP project indexed, release),
/// it settled at ~184 MB (peak ~110 MB footprint) against ~215 MB (~160 MB) with mimalloc,
/// which indexed ~10% faster but keeps free pages per thread. The `system-allocator`
/// feature keeps the system's on Windows too (to compare them there).
/// Either way, the heap's live bytes are counted (`kernel::memory::live_heap`: the Task
/// Manager's "data" next to the process's footprint).
#[cfg(all(windows, not(feature = "system-allocator")))]
#[global_allocator]
static ALLOCATOR: kernel::memory::Counting<mimalloc::MiMalloc> =
    kernel::memory::Counting(mimalloc::MiMalloc);
#[cfg(not(all(windows, not(feature = "system-allocator"))))]
#[global_allocator]
static ALLOCATOR: kernel::memory::Counting<std::alloc::System> =
    kernel::memory::Counting(std::alloc::System);

/// mimalloc's free pages back to the system (after something large was freed).
#[cfg(all(windows, not(feature = "system-allocator")))]
fn release_allocator() {
    // SAFETY: mimalloc's own call; it only returns its free pages.
    #[allow(unsafe_code)]
    unsafe {
        libmimalloc_sys::mi_collect(true);
    }
}

fn main() -> anyhow::Result<()> {
    // The release's version: only this package carries it (the others' builds stay as they
    // are across releases); every crate reads it from here.
    kernel::brand::set_version(env!("CARGO_PKG_VERSION"));
    #[cfg(all(windows, not(feature = "system-allocator")))]
    kernel::memory::set_allocator_release(release_allocator);
    #[cfg(target_os = "macos")]
    without_large_block_cache();
    // Run by an agent as it connects to the MCP server: the header it sends, printed from
    // the keychain (the token is never on its command line); nothing else runs.
    if std::env::args_os().nth(1).is_some_and(|a| a == mcp::HELPER_ARG) {
        let service = format!("{}.{}", kernel::brand::ID, mcp::TOKEN_SERVICE);
        let store = secrets::Keychain { service };
        return match kernel::SecretStore::get(&store, mcp::TOKEN_ID) {
            Some(token) => {
                println!("{}", mcp::helper_output(token.expose()));
                Ok(())
            }
            None => anyhow::bail!("the MCP server has no token yet"),
        };
    }
    // Started by the app as it quit, to put an update in its place (a copy of its program in
    // the temporary folder): nothing else runs.
    if std::env::args_os().nth(1).is_some_and(|a| a == self_update::install::HELPER_ARG) {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        std::process::exit(self_update::install::apply_main(&args));
    }
    let started = startup::begin();
    // The snapshots' language server (made up, on stdin / stdout): nothing else runs.
    #[cfg(feature = "snapshot")]
    if std::env::args_os().nth(1).is_some_and(|a| a == "--fake-language-server") {
        lsp_client::fake::serve_stdio();
        return Ok(());
    }
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,wgpu_core=warn,wgpu_hal=warn,naga=warn".into())
    };
    // Without a console (Windows, or the app launched from Finder / Dock), the log goes to a
    // file: `~/.config/<app id>/<app id>.log` (on Windows, %APPDATA%\<app id>\<app id>.log).
    use std::io::IsTerminal;
    let to_file = cfg!(windows) || !std::io::stdout().is_terminal();
    match log_file() {
        Some(file) if to_file => tracing_subscriber::fmt()
            .with_env_filter(filter())
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init(),
        _ => tracing_subscriber::fmt().with_env_filter(filter()).init(),
    }

    startup::mark("logging ready");

    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let snapshot =
        (args.first().is_some_and(|a| a == "--snapshot") && args.len() >= 2).then(|| {
            args.remove(0);
            args.remove(0)
        });
    // Started again after an update: the session before it, reopened (not a path to open).
    let reopen_session =
        args.iter().position(|a| a == self_update::install::REOPEN_ARG).and_then(|i| {
            let id = args.get(i + 1).and_then(|a| a.to_str()).and_then(kernel::id::parse_uuid);
            args.drain(i..(i + 2).min(args.len()));
            id
        });
    #[cfg(not(feature = "snapshot"))]
    if snapshot.is_some() {
        anyhow::bail!("this build can't render snapshots (built without the `snapshot` feature)");
    }
    // Already running (this executable): the paths go to it, this start ends.
    // `EDITOR_NEW_INSTANCE=1` starts another one anyway.
    if snapshot.is_none() && std::env::var_os("EDITOR_NEW_INSTANCE").is_none() {
        let paths: Vec<std::path::PathBuf> = args.iter().map(std::path::PathBuf::from).collect();
        if instance::claim(&paths) == instance::Start::Handed {
            return Ok(());
        }
    }
    // The app's other instances with these settings: what stands for the whole app is
    // shared with them.
    if snapshot.is_none() {
        peers::join();
        // The programs started from now on noted; what an earlier run left looked at.
        left_behind::start();
    }

    // The GPU (its device, shaders, a window's pipelines) made in the background from now,
    // while the settings, the modules and the window come; the window takes it.
    let preferred_api =
        std::env::var("EDITOR_GPU_API").ok().and_then(|v| GpuApi::parse(&v)).unwrap_or_default();
    // Windows: the window's corners are cut by the app (on the GPU, whatever the system
    // does: Windows 10, a virtual machine), through DX12's DirectComposition.
    // `EDITOR_DX12_HWND=1` goes back to presenting straight to the window (square corners).
    let transparent = cfg!(windows)
        && matches!(preferred_api, GpuApi::Auto | GpuApi::Dx12)
        && std::env::var("EDITOR_DX12_HWND").is_err();
    let render_options = RenderOptions {
        vsync: std::env::var("EDITOR_VSYNC").map_or(true, |v| v != "0"),
        preferred_api,
        transparent,
    };
    if snapshot.is_none() {
        render_wgpu::WgpuRendererFactory.prepare(&render_options);
    }

    // Scanning system fonts takes a while; we do it in parallel with the window and the GPU.
    let locale = i18n::locale();
    let fonts = std::thread::Builder::new().name("font-loader".into()).spawn(move || {
        let mut text = TextSystem::with_locale(&locale);
        let family = text.first_available(CODE_FONTS, FontFamily::Monospace);
        // What the first frame draws with (the interface's weights, the code's), found
        // here while the window opens rather than during that frame (~40 ms).
        let ui = |weight| text_layout::Font::new(FontFamily::SansSerif, 13.0).with_weight(weight);
        text.warm_up(&[
            ui(500),
            ui(text_layout::Font::SEMIBOLD),
            ui(text_layout::Font::BOLD),
            text_layout::Font::new(family.clone(), 13.0),
        ]);
        tracing::info!(font = ?family, elapsed = ?started.elapsed(), "fonts loaded");
        (text, family)
    })?;

    startup::timed("language and keymaps read", || {
        language::load();
        keymaps::load();
    });
    // What shows a command's keys (a toolbar's tooltip) reads them as they are now.
    ui_workbench::shortcuts::set_reader(|id| {
        commands::by_id(id).and_then(commands::shortcut_label)
    });
    // The modules (and the themes they bring) registered on a thread of their own while the
    // main thread builds the event loop (the system's side of starting); taken right after.
    let loading = std::thread::Builder::new().name(MODULE_LOADER.into()).spawn(|| {
        let selection = modules::selection_from_settings();
        let (support, _) = startup::timed("modules registered", || modules::load(&selection));
        // The themes as the modules bring them (the bundled ones, their format).
        let themes = startup::timed("themes read", || Themes::load(&support.themes));
        (support, themes)
    })?;
    let loaded = move || loading.join().map_err(|_| anyhow::anyhow!("loading the modules failed"));

    #[cfg(feature = "snapshot")]
    if let Some(out) = snapshot {
        let (support, themes) = loaded()?;
        let (mut workbench, mut themes, _, _) = assemble(support, themes, &args, true)?;
        let options = snapshot::Options::from_env();
        snapshot::prepare(&options, &mut workbench, &mut themes)?;
        return snapshot::render(Path::new(&out), &options, workbench, &mut themes, fonts);
    }

    let prepare = move || -> anyhow::Result<(WindowOptions, Box<dyn platform_api::Application>)> {
        let (support, themes) = loaded()?;
        let (workbench, themes, syntax, ports) = assemble(support, themes, &args, false)?;
        let options = WindowOptions {
            title: workbench.title(),
            menus: commands::menus(),
            unified_titlebar: Some(40.0),
            transparent,
            // Below this, the panels and the editor no longer fit nicely.
            min_size: kernel::Size::new(900.0, 600.0),
            icon: APP_ICON,
            ..WindowOptions::default()
        };
        let support = workbench.support();
        // The modules' data: read ahead (as chosen) and let go when unused, in the background.
        support.manager.start();
        let shared = app::Shared::new(themes, syntax, fonts, render_options, started);
        startup::mark("event loop built, app made");
        // Only a project given (no files): its tabs from last time, once the window exists.
        let restore = !args.is_empty() && args.iter().all(|a| Path::new(a).is_dir());
        let sessions = sessions::AppSessions::load();
        let app = windows::AppWindows::new(shared, support, ports, workbench, restore, sessions)
            .reopen_at_start(reopen_session);
        Ok((options, Box::new(app)))
    };
    let ran = WinitPlatform.run_prepared(Box::new(move || prepare().map_err(|e| format!("{e:#}"))));
    // The settings still being written behind reach the disk before the process ends.
    storage::settings::flush();
    // A normal exit: nothing left behind to note.
    left_behind::finish();
    ran?;
    Ok(())
}

/// The thread the modules are registered on at start-up (it ends once done).
pub(crate) const MODULE_LOADER: &str = "module-loader";

/// The first window's workbench, from the modules loaded: the command line's paths opened
/// in it (`snapshot`: rendering a snapshot, the user's projects forgotten). With the themes,
/// the grammars and the ports the windows share.
fn assemble(
    support: modules::LanguageSupport,
    themes: Themes,
    args: &[std::ffi::OsString],
    snapshot: bool,
) -> anyhow::Result<(Workbench, Themes, std::sync::Arc<syntax_ts::SyntaxConfig>, workbench::Ports)>
{
    let syntax = std::sync::Arc::clone(&support.syntax);
    let support = std::sync::Arc::new(support);
    // Code outside the documents (a Markdown preview's) is highlighted with the grammars.
    snippets::Snippets::register(std::sync::Arc::clone(&support));
    // The environment (e.g. the installed PHP's version) looked at in the background, before
    // anything opens: opening a project never waits for it; the windows take the levels
    // found once it's done (`levels::warmed`).
    let warm = std::sync::Arc::clone(&support);
    let _ = std::thread::Builder::new().name("level-detect".into()).spawn(move || {
        for intel in &warm.intelligence {
            intel.warm();
        }
        levels::warmed();
    });
    let ports = workbench::Ports::local();
    let mut workbench = startup::timed("workbench made", || Workbench::new(support, ports.clone()));
    if snapshot {
        workbench.forget_projects();
        #[cfg(feature = "snapshot")]
        snapshot::before_open();
    }
    for path in args {
        let path = Path::new(path);
        workbench.open_path(path).with_context(|| format!("can't open {}", path.display()))?;
    }
    startup::mark("command-line paths opened");
    Ok((workbench, themes, syntax, ports))
}

/// The app's icon for the windows (Linux) and the Dock outside the bundle (macOS), made by
/// `cargo xtask icon`. Windows has it among the executable's resources (`build.rs`).
#[cfg(not(windows))]
const APP_ICON: Option<&[u8]> = Some(kernel::brand::ICON_PNG);
#[cfg(windows)]
const APP_ICON: Option<&[u8]> = None;

/// macOS keeps freed large blocks (a decoded image, a purged cache) in a cache that still
/// counts as the app's memory; with `MallocLargeCache=0` they go back to the system at
/// once. The allocator reads it only at startup: the bundle sets it (`LSEnvironment` in
/// `Info.plist`); started otherwise, the process restarts itself once with it (`exec`:
/// same pid, before anything else ran).
#[cfg(target_os = "macos")]
fn without_large_block_cache() {
    use std::os::unix::process::CommandExt;
    const VAR: &str = "MallocLargeCache";
    if std::env::var_os(VAR).is_some() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        // Returns only on failure: then the app simply runs with the cache.
        let _ =
            std::process::Command::new(exe).args(std::env::args_os().skip(1)).env(VAR, "0").exec();
    }
}

#[cfg(test)]
#[path = "../tests/unit/startup_memory.rs"]
mod startup_memory;
