use super::*;

fn sample(at_ms: u64) -> Sample {
    Sample {
        memory_peak: 0,
        gpu_peak: None,
        at_ms,
        span_ms: 100,
        cpu_us: 50_000,
        memory: 1_000 + at_ms,
        gpu: Some(10),
        frames: 2,
        frame_us: 8_000,
        frame_longest_us: 5_000,
        threads: vec![ThreadCpu { thread: "main".into(), us: 40_000 }],
        triggers: vec![Trigger { owner: "log-viewer".into(), cause: "poll".into(), count: 3 }],
        busy: vec![Busy {
            owner: "log-viewer".into(),
            what: "paint".into(),
            calls: 2,
            us: 3_000,
            longest_us: 2_000,
        }],
        counters: vec![
            Counter::new("indexer", "files", 4),
            Counter { unit: "bytes".into(), ..Counter::new("http-client", "sent", 512) },
        ],
        levels: vec![Level { owner: "lsp".into(), what: "pending".into(), peak: 3, last: 1 }],
        io: vec![Io { owner: "http-client".into(), sent: 100, received: 900, read: 0, written: 0 }],
        disk_read: 10,
        disk_written: 20,
        spans: vec![Span {
            path: vec![step("app", "frame"), step("log-viewer", "paint")],
            calls: 2,
            us: 900,
        }],
    }
}

/// Every key the format may hold, and only those: a new field fails this until it's added
/// here on purpose (and checked against `.claude/rules/privacy.md`).
const ALLOWED: &[&str] = &[
    "version",
    "started",
    "duration_ms",
    "interval_ms",
    "system",
    "owners",
    "samples",
    "os",
    "os_version",
    "arch",
    "cores",
    "memory",
    "gpu",
    "gpu_driver",
    "session",
    "scale",
    "app_version",
    "build",
    "at_ms",
    "span_ms",
    "cpu_us",
    "frames",
    "frame_us",
    "frame_longest_us",
    "threads",
    "triggers",
    "busy",
    "thread",
    "us",
    "owner",
    "cause",
    "count",
    "what",
    "calls",
    "longest_us",
    "counters",
    "levels",
    "peak",
    "last",
    "unit",
    "io",
    "sent",
    "received",
    "read",
    "written",
    "disk_read",
    "disk_written",
    "spans",
    "path",
    "memory_peak",
    "gpu_peak",
];

fn keys(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                keys(v, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| keys(v, out)),
        _ => {}
    }
}

#[test]
fn a_recording_holds_only_the_fields_allowed() {
    let mut recording = Recording::new("2026-11-20T10:00:00Z".into(), 100, System::default());
    recording.owners = Owners(vec![("indexer".into(), "code.index".into())]);
    recording.push(sample(0));
    let value: serde_json::Value = serde_json::from_str(&recording.to_json()).unwrap();
    let mut found = Vec::new();
    keys(&value, &mut found);
    for key in found {
        assert!(ALLOWED.contains(&key.as_str()), "a field not allowed: {key}");
    }
}

/// Read back as written; a newer format refused.
#[test]
fn a_recording_reads_back_and_a_newer_one_is_refused() {
    let mut recording = Recording::new("2026-11-20T10:00:00Z".into(), 100, System::default());
    recording.push(sample(0));
    recording.push(sample(100));
    let back = Recording::from_json(&recording.to_json()).unwrap();
    assert_eq!(back, recording);
    assert_eq!(back.duration_ms, 200);
    let newer = recording.to_json().replace("\"version\":1", "\"version\":9");
    assert!(Recording::from_json(&newer).is_err());
}

/// Past the budget, the oldest samples merge by twos: the totals stay, the count drops.
#[test]
fn a_long_recording_stays_small() {
    let mut recording = Recording::new(String::new(), 100, System::default());
    for i in 0..(MAX_SAMPLES as u64 + 1) {
        recording.push(sample(i * 100));
    }
    assert!(recording.samples.len() <= MAX_SAMPLES);
    let cpu: u64 = recording.samples.iter().map(|s| s.cpu_us).sum();
    assert_eq!(cpu, 50_000 * (MAX_SAMPLES as u64 + 1), "nothing lost");
    let triggers: u64 = recording.samples.iter().flat_map(|s| &s.triggers).map(|t| t.count).sum();
    assert_eq!(triggers, 3 * (MAX_SAMPLES as u64 + 1));
    let spans: u64 = recording.samples.iter().map(|s| s.span_ms).sum();
    // The highest memory seen stays, however merged (the first samples' were the highest).
    let peak = recording.samples.iter().map(|s| s.memory_peak.max(s.memory)).max().unwrap();
    assert!(peak >= 1_000 + 100 * MAX_SAMPLES as u64, "{peak}");
    assert_eq!(spans, 100 * (MAX_SAMPLES as u64 + 1), "the time covered whole");
}

/// A name kept is the code's, cleaned: no slash-free path tricks, no control characters,
/// no other script, cut.
#[test]
fn names_are_cleaned() {
    assert_eq!(name("http-run"), "http-run");
    assert_eq!(name("tokio-runtime-worker\n"), "tokio-runtime-worker");
    assert_eq!(name("caf\u{e9} \u{1b}[31m"), "caf 31m");
    assert_eq!(name(&"x".repeat(100)).len(), 48);
}
