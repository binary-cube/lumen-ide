use super::*;
use std::sync::mpsc;
use tracing_subscriber::layer::SubscriberExt;

fn lines(rx: &mpsc::Receiver<Msg>) -> Vec<Value> {
    rx.try_iter()
        .filter_map(|m| match m {
            Msg::Line { text, .. } => Some(serde_json::from_str(&text).unwrap()),
            Msg::Flush(_) | Msg::Dir(_) => None,
        })
        .collect()
}

/// An event: its level, target, thread, run, message, typed fields (a duration in ms) and
/// the spans it's in, the outermost first; a warning is urgent.
#[test]
fn an_event_becomes_its_line_with_its_spans() {
    let (tx, rx) = mpsc::sync_channel(16);
    let subscriber = tracing_subscriber::registry().with(JsonLayer::new(tx, "run1".into()));
    tracing::subscriber::with_default(subscriber, || {
        let project = tracing::info_span!("project", name = "shop");
        let _in = project.enter();
        let source = tracing::info_span!("source", id = 7);
        let _in_source = source.enter();
        tracing::info!(count = 3, elapsed = ?std::time::Duration::from_millis(12), "opened");
        tracing::warn!("careful");
    });
    let mut all = rx.try_iter().collect::<Vec<_>>().into_iter();
    let Some(Msg::Line { text, urgent, .. }) = all.next() else { panic!("a line") };
    assert!(!urgent);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["level"], "INFO");
    assert_eq!(v["msg"], "opened");
    assert_eq!(v["run"], "run1");
    assert!(v["target"].as_str().unwrap().contains("layer"));
    assert_eq!(v["fields"]["count"], 3);
    assert_eq!(v["fields"]["elapsed_ms"], 12.0);
    assert_eq!(v["span"][0]["name"], "project");
    assert_eq!(v["span"][0]["fields"]["name"], "shop");
    assert_eq!(v["span"][1]["fields"]["id"], 7);
    assert!(v["thread"].is_string());
    let Some(Msg::Line { urgent, .. }) = all.next() else { panic!("the warning") };
    assert!(urgent, "a warning is written at once");
}

/// The writer behind (the channel full): lines dropped and counted, never waited for; the
/// next line sent says how many.
#[test]
fn a_full_channel_drops_and_counts_never_waits() {
    let (tx, rx) = mpsc::sync_channel(2);
    let layer = JsonLayer::new(tx, "r".into());
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        for i in 0..5 {
            tracing::info!(i, "one");
        }
    });
    assert_eq!(lines(&rx).len(), 2, "the first two");
    // Room again: the note first, then the line.
    let (tx, rx) = mpsc::sync_channel(8);
    let layer = JsonLayer::new(tx, "r".into());
    layer.dropped.store(3, Ordering::Relaxed);
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || tracing::info!("after"));
    let got = lines(&rx);
    assert_eq!(got[0]["fields"]["dropped"], 3);
    assert_eq!(got[1]["msg"], "after");
}

/// What a `debug!` costs when the level is `info` (plan 89c: debug stays compiled in
/// release only if it's next to nothing). Run by hand: `--ignored --nocapture`.
#[test]
#[ignore = "a measurement"]
fn a_debug_line_below_the_level_costs_next_to_nothing() {
    let (tx, _rx) = mpsc::sync_channel(16);
    let filter = tracing_subscriber::EnvFilter::new("info");
    let subscriber =
        tracing_subscriber::registry().with(filter).with(JsonLayer::new(tx, "r".into()));
    tracing::subscriber::with_default(subscriber, || {
        const N: u32 = 10_000_000;
        let started = std::time::Instant::now();
        for i in 0..N {
            tracing::debug!(i, path = "a/b.php", "a line not written");
        }
        let each = started.elapsed() / N;
        println!("a debug line below the level: {each:?} each ({N} in {:?})", started.elapsed());
        assert!(each < std::time::Duration::from_nanos(50), "{each:?}");
    });
}
