//! What the web engine costs, measured with the real engine (plan 99): the host and every
//! process it started (GPU, renderers, utilities), their CPU time and their physical
//! footprint (what the system's monitor counts, not the shared library counted again in
//! each), in the states a user goes through:
//!
//! 1. the host started, no page;
//! 2. a page that doesn't change (Chromium draws only when something does);
//! 3. the page hidden (its tab behind another);
//! 4. draw.io open and still (with `EDITOR_DRAWIO=<the draw.io package's folder>`);
//! 5. every page closed: the host ends by itself, its memory given back.
//!
//! Each state has a budget checked here; the numbers are printed as a table (they go in the
//! plan). Ignored unless asked: it needs the engine bundled —
//! `EDITOR_WEB_HOST=<…>/web-host.app/Contents/MacOS/web-host cargo test --test consumption -- --ignored --nocapture`.

// A measuring test: a step that fails ends it, saying which.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use web_engine::protocol::{Event, Size, Source};
use web_engine::{Connection, Launch, PageEvent, PageSpec, Sink, WebPage};

/// How long each state is measured.
const WINDOW: Duration = Duration::from_secs(5);
/// How long the host lives on with no page here (the real one: 5 minutes).
const IDLE_SECS: &str = "12";

/// The budgets: CPU as a share of one core over the window, memory in MB (measured on an
/// M2 Max: ~1 % with no page or a hidden one, ~1 % a still page, ~2 % draw.io still;
/// Chromium's own processes keep timers of their own).
const IDLE_CPU: f64 = 0.015;
const STILL_PAGE_CPU: f64 = 0.03;
const HIDDEN_CPU: f64 = 0.015;
const STILL_PAGE_MB: f64 = 450.0;
const DRAWIO_MB: f64 = 500.0;

/// What the host's tree used over a window.
struct Use {
    cpu: f64,
    mb: f64,
    processes: u32,
}

/// Measures the tree of `host` over `window`, `during` run every 16 ms meanwhile.
fn measure(
    trees: &mut process_stats::ProcessTrees,
    host: u32,
    window: Duration,
    during: &dyn Fn(),
) -> Use {
    let before = trees.sample(&[host]).remove(&host).expect("the host runs");
    let since = Instant::now();
    while since.elapsed() < window {
        during();
        std::thread::sleep(Duration::from_millis(16));
    }
    let after = trees.sample(&[host]).remove(&host).expect("the host runs");
    let used = after.cpu.saturating_sub(before.cpu).as_secs_f64() / since.elapsed().as_secs_f64();
    Use { cpu: used, mb: after.memory as f64 / 1e6, processes: after.processes }
}

fn row(name: &str, used: &Use) {
    println!(
        "| {name:<34} | {:>6.2} % | {:>7.0} MB | {:>2} |",
        used.cpu * 100.0,
        used.mb,
        used.processes
    );
}

/// The frames a page gets, given back at once (as the app copies them).
fn sink() -> Sink {
    Arc::new(|event| {
        if let PageEvent::Event(Event::Crashed { .. }) = event {
            panic!("the engine crashed");
        }
    })
}

#[test]
#[ignore = "needs the engine bundled (EDITOR_WEB_HOST)"]
fn what_the_engine_costs() {
    let Ok(program) = std::env::var("EDITOR_WEB_HOST") else { return };
    // SAFETY: set before this test's threads read the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("EDITOR_WEB_HOST_IDLE_SECS", IDLE_SECS)
    };
    let dir = std::env::temp_dir().join(format!("wh-cost-{}", std::process::id()));
    let channel = web_host_core::channel::Channel {
        name: format!("web-host-cost-{}", std::process::id()),
        dir,
    };
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
    let host = connection.host();
    let mut trees = process_stats::ProcessTrees::default();
    println!("\n| state | CPU (one core) | memory | processes |\n|---|---|---|---|");

    std::thread::sleep(Duration::from_secs(2));
    let idle = measure(&mut trees, host, WINDOW, &|| {});
    row("1. no page", &idle);

    let spec = PageSpec {
        source: Source::Html("<body style='margin:0'><h1>still</h1><p>text</p></body>".into()),
        size: Size { width: 1200, height: 800, scale: 2.0 },
        hidden: false,
        background: [255; 4],
        pixels: None,
    };
    let page = WebPage::open(Arc::clone(&connection), spec, sink());
    std::thread::sleep(Duration::from_secs(3));
    let still = measure(&mut trees, host, WINDOW, &|| {});
    row("2. still page", &still);

    page.set_hidden(true);
    std::thread::sleep(Duration::from_millis(500));
    let hidden = measure(&mut trees, host, WINDOW, &|| {});
    row("3. page hidden", &hidden);
    drop(page);

    let drawio = std::env::var("EDITOR_DRAWIO").ok().map(|root| {
        let spec = PageSpec {
            source: Source::App { root: root.into(), entry: "index.html?embed=1&ui=min".into() },
            size: Size { width: 1600, height: 1000, scale: 2.0 },
            hidden: false,
            background: [255; 4],
            pixels: None,
        };
        let page = WebPage::open(Arc::clone(&connection), spec, sink());
        std::thread::sleep(Duration::from_secs(6));
        let used = measure(&mut trees, host, WINDOW, &|| {});
        row("4. draw.io open, still", &used);
        drop(page);
        used
    });

    // Every page closed: the host ends after its idle time, everything given back.
    let closed = Instant::now();
    while trees.sample(&[host]).contains_key(&host) {
        assert!(closed.elapsed() < Duration::from_secs(30), "the host ended by itself");
        std::thread::sleep(Duration::from_millis(200));
    }
    println!(
        "| 5. no page for {IDLE_SECS} s: the host ended | after {:.1} s | 0 MB | 0 |",
        closed.elapsed().as_secs_f64()
    );

    assert!(idle.cpu < IDLE_CPU, "no page: {:.2} % of a core", idle.cpu * 100.0);
    assert!(still.cpu < STILL_PAGE_CPU, "a still page: {:.2} % of a core", still.cpu * 100.0);
    assert!(hidden.cpu < HIDDEN_CPU, "a hidden page: {:.2} % of a core", hidden.cpu * 100.0);
    assert!(still.mb < STILL_PAGE_MB, "a still page: {:.0} MB", still.mb);
    if let Some(drawio) = drawio {
        assert!(drawio.mb < DRAWIO_MB, "draw.io: {:.0} MB", drawio.mb);
    }
}
