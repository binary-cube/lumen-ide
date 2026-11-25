//! The web engine's process (plan 99): Chromium (CEF) drawing the app's web pages off
//! screen, one for every app of the user.
//!
//! Started by an app (`--channel-dir <dir> --channel-name <name>`): it takes the channel
//! (another host holding it: this one ends at once), starts CEF, and serves the apps
//! (`web_host_core::Server`) until it has had no page for its idle time. Its main thread
//! sleeps until an app asks something or CEF's own work is due ([`pump`]): a host with
//! nothing to draw costs nothing. On Windows and Linux the same program runs CEF's
//! subprocesses; on macOS they are the helper apps (`helper.rs`).

use std::process::ExitCode;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use web_host::mac_app;
use web_host::options::Options;
use web_host::{app, engine, pump, scheme};
use web_host_core::channel::Channel;
use web_host_core::{Lifetime, Server, State};

/// The longest the main thread sleeps with nothing asked and no page (a wake lost would
/// cost this).
const LONGEST_SLEEP: Duration = Duration::from_secs(1);

/// With a page shown, CEF's work runs at least this often while something happens (an
/// app's command, a frame drawn: for [`pump::ACTIVE_FOR`] after it), as CEF's own external
/// pumps do — what it doesn't schedule itself (a page loading) would otherwise wait.
const PAGE_PUMP: Duration = Duration::from_millis(33);

/// …and this often when nothing happens (a still page: a few wakes a second, ~0 CPU).
const STILL_PUMP: Duration = Duration::from_millis(250);

/// How long the browsers have to close before CEF ends.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// How often CEF's work runs while the browsers close.
const CLOSE_PUMP: Duration = Duration::from_millis(10);

fn main() -> ExitCode {
    let started = Instant::now();
    // What it is, for its packaging (nothing of CEF loaded).
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("{}", engine::manifest());
        return ExitCode::SUCCESS;
    }
    #[cfg(target_os = "macos")]
    let Some(_framework) = mac_app::load_framework(false) else {
        return ExitCode::FAILURE;
    };
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    // Only loaded, then ended (`--warm`, after an install): the system checks the engine's
    // files now, in the background, not at the first page that needs it.
    if std::env::args().nth(1).as_deref() == Some("--warm") {
        return ExitCode::SUCCESS;
    }
    let args = cef::args::Args::new();
    let mut app = app::build();
    // A subprocess of CEF's (Windows, Linux): it runs and ends here.
    let code =
        cef::execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    if code >= 0 {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    let Some(options) = Options::parse(std::env::args().skip(1)) else {
        eprintln!("usage: web-host --channel-dir <dir> --channel-name <name> --profile <dir>");
        return ExitCode::from(2);
    };
    // Another host runs: this one ends (its log left to it).
    let claim = match options.channel.claim() {
        Ok(Some(claim)) => claim,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("the channel can't be taken: {e}");
            return ExitCode::FAILURE;
        }
    };
    logging(&options.channel);
    // Its version held as in use while it runs (released as it ends).
    let _held = options.hold.as_deref().and_then(hold);
    // `EDITOR_WEB_HOST_IDLE_SECS`: how long it lives on with no page (tests end it soon).
    let lifetime = std::env::var("EDITOR_WEB_HOST_IDLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or_else(Lifetime::default, |secs| Lifetime { idle: Duration::from_secs(secs) });
    let mut server = match Server::start(claim, lifetime) {
        Ok(server) => server,
        Err(e) => {
            tracing::error!(error = %e, "the server didn't start");
            return ExitCode::FAILURE;
        }
    };
    let pump = pump::Pump::new(server.waker());
    #[cfg(target_os = "macos")]
    mac_app::make_application();
    if let Some(adapter) = options.adapter {
        app::draw_on(adapter);
    }
    if !app::initialize(&args, &mut app, &options.profile) {
        tracing::error!("the engine didn't start");
        return ExitCode::FAILURE;
    }
    scheme::register();
    tracing::info!(engine = %engine::version(), elapsed = ?started.elapsed(), "the web host runs");
    let mut engine = engine::CefEngine::new(server.acks().clone(), options.channel.dir.clone());
    let mut worked = Instant::now();
    loop {
        // CEF's work when it asks; with a page shown, also at least every `PAGE_PUMP` while
        // something happens, every `STILL_PUMP` when not (hidden pages and none: only when
        // asked).
        let floor = match (engine.shown_pages() > 0, pump.active()) {
            (false, _) => None,
            (true, true) => Some(PAGE_PUMP),
            (true, false) => Some(STILL_PUMP),
        };
        let wait = match floor {
            Some(floor) => pump.until_due().min(floor.saturating_sub(worked.elapsed())),
            None => pump.until_due().min(LONGEST_SLEEP),
        };
        // A page's accessibility tree held back: woken when it's due.
        let wait = match engine.tell_access() {
            Some(due) => wait.min(due.saturating_duration_since(Instant::now())),
            None => wait,
        };
        if server.poll(&mut engine, wait) == State::Ended {
            break;
        }
        // A page whose app caught up after frames were dropped: painted again.
        engine.catch_up();
        if pump.take_due() || floor.is_some_and(|floor| worked.elapsed() >= floor) {
            cef::do_message_loop_work();
            worked = Instant::now();
        }
    }
    tracing::info!("no page for a while: the web host ends");
    engine.close_all();
    // The browsers' closing runs on CEF's loop: given its time before CEF ends.
    let closing = Instant::now();
    while engine.open_browsers() > 0 && closing.elapsed() < CLOSE_WAIT {
        cef::do_message_loop_work();
        std::thread::sleep(CLOSE_PUMP);
    }
    cef::shutdown();
    ExitCode::SUCCESS
}

/// `path` created if needed and held locked (`None`: it can't be; the host runs anyway).
fn hold(path: &std::path::Path) -> Option<std::fs::File> {
    let file =
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
    match file.try_lock() {
        Ok(()) => Some(file),
        // Another host of the same version holds it: as good.
        Err(_) => None,
    }
}

/// The host's log: a file beside its channel (nobody reads its standard streams), its
/// level from `EDITOR_LOG` (warnings by default). The previous run's is kept beside it
/// (`.previous.log`): a host that ended badly is started again by its app, and its log
/// must not go with that.
fn logging(channel: &Channel) {
    let _ = std::fs::create_dir_all(&channel.dir);
    let path = channel.dir.join(format!("{}.log", channel.name));
    let _ = std::fs::rename(&path, channel.dir.join(format!("{}.previous.log", channel.name)));
    let Ok(file) = std::fs::File::create(path) else { return };
    let filter = tracing_subscriber::EnvFilter::try_from_env("EDITOR_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_env_filter(filter)
        .init();
}
