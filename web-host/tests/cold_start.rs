//! How long the web engine takes to start cold (plan 99, step 4), measured with the real
//! engine: a host started anew each time (as for the first page after a while with none),
//! with a profile used before (as every start but the very first) and with a new one.
//! For each, from the app's side:
//!
//! - `joined`: the host started and greeted the app;
//! - `HTML`: a response's page opened → its content loaded → its first frame since;
//! - `draw.io` (with `EDITOR_DRAWIO`): the same for draw.io's page.
//!
//! The medians are printed as a table (they go in the plan). Ignored unless asked: it needs
//! the engine bundled — `EDITOR_WEB_HOST=<…>/web-host.app/Contents/MacOS/web-host cargo
//! test --test cold_start -- --ignored --nocapture`.

// A measuring test: a step that fails ends it, saying which.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use web_engine::protocol::{Event, Size, Source};
use web_engine::{Connection, Launch, PageEvent, PageSpec, WebPage};

/// Starts measured, each time a host made anew.
const RUNS: usize = 5;
/// The longest a step may take before the run is called broken.
const STEP_WAIT: Duration = Duration::from_secs(30);

/// What a page told the test: loaded, or a frame (given back at once).
enum Seen {
    Loaded,
    Frame,
}

/// `source` opened on `connection`: how long until it loaded, and until its first frame
/// after that.
fn first_frame(connection: &Arc<Connection>, source: Source) -> (Duration, Duration) {
    let (tell, seen): (_, Receiver<Seen>) = channel();
    let sink: web_engine::Sink = Arc::new(move |event| {
        let said = match event {
            PageEvent::Frame(_) => Seen::Frame,
            PageEvent::Event(Event::Loaded { .. }) => Seen::Loaded,
            PageEvent::Event(Event::Crashed { .. }) => panic!("the engine crashed"),
            PageEvent::Event(_) => return,
        };
        let _ = tell.send(said);
    });
    let spec = PageSpec {
        source,
        size: Size { width: 1600, height: 1000, scale: 2.0 },
        hidden: false,
        background: [255; 4],
        pixels: None,
    };
    let opened = Instant::now();
    let page = WebPage::open(Arc::clone(connection), spec, sink);
    let mut loaded = None;
    loop {
        match seen.recv_timeout(STEP_WAIT).expect("the page answered in time") {
            Seen::Loaded => loaded = loaded.or(Some(opened.elapsed())),
            Seen::Frame if loaded.is_some() => break,
            Seen::Frame => {}
        }
    }
    let framed = opened.elapsed();
    drop(page);
    (loaded.unwrap_or(framed), framed)
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

fn ms(d: Duration) -> String {
    format!("{} ms", d.as_millis())
}

#[test]
#[ignore = "needs the engine bundled (EDITOR_WEB_HOST)"]
fn how_long_the_engine_takes_to_start() {
    let Ok(program) = std::env::var("EDITOR_WEB_HOST") else { return };
    // The host ends soon with no page: each run starts one anew.
    // SAFETY: set before this test's threads read the environment.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("EDITOR_WEB_HOST_IDLE_SECS", "2")
    };
    let drawio = std::env::var("EDITOR_DRAWIO").ok();
    let root = std::env::temp_dir().join(format!("wh-cold-{}", std::process::id()));
    println!(
        "\n| start | joined (slowest) | HTML loaded | HTML frame | draw.io loaded | draw.io frame |"
    );
    println!("|---|---|---|---|---|---|");
    for (label, fresh) in [("profile used before", false), ("new profile", true)] {
        let mut rows: [Vec<Duration>; 5] = Default::default();
        for run in 0..RUNS {
            let channel = web_host_core::channel::Channel {
                name: format!("web-host-cold-{}-{run}-{fresh}", std::process::id()),
                dir: root.clone(),
            };
            let profile = match fresh {
                true => root.join(format!("profile-{run}")),
                false => root.join("profile"),
            };
            let launch = Launch {
                program: program.clone().into(),
                channel: channel.clone(),
                profile,
                hold: None,
                adapter: None,
            };
            let started = Instant::now();
            let connection =
                Arc::new(Connection::establish(&channel, None, || Some(launch)).expect("joined"));
            rows[0].push(started.elapsed());
            let html = Source::Html("<body><h1>A response</h1><p>text</p></body>".into());
            let (loaded, framed) = first_frame(&connection, html);
            rows[1].push(loaded);
            rows[2].push(framed);
            if let Some(drawio) = &drawio {
                let source = Source::App {
                    root: drawio.into(),
                    entry: "index.html?embed=1&ui=kennedy".into(),
                };
                let (loaded, framed) = first_frame(&connection, source);
                rows[3].push(loaded);
                rows[4].push(framed);
            }
            drop(connection);
            // The host gone before the next (its channel and its processes).
            std::thread::sleep(Duration::from_millis(4500));
        }
        let cell = |values: &Vec<Duration>| {
            if values.is_empty() { "—".to_owned() } else { ms(median(values.clone())) }
        };
        let slowest = rows[0].iter().max().copied().map_or_else(String::new, ms);
        println!(
            "| {label} | {} ({slowest}) | {} | {} | {} | {} |",
            cell(&rows[0]),
            cell(&rows[1]),
            cell(&rows[2]),
            cell(&rows[3]),
            cell(&rows[4])
        );
    }
    let _ = std::fs::remove_dir_all(root);
}
