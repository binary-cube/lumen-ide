use super::*;
use crate::format::{Busy, Counter, Io, Level, Owners, Sample, System, ThreadCpu, Trigger};

/// The threads' CPU by module (a prefix counts), who asks for frames a second, what takes
/// the interface's time, the frames: what points at the module behind a busy app.
#[test]
fn a_summary_says_which_module_keeps_the_app_busy() {
    let mut recording = Recording::new(String::new(), 1000, System::default());
    recording.owners = Owners(vec![
        ("indexer".into(), "code.index".into()),
        ("git-".into(), "vcs.git".into()),
        ("main".into(), "app".into()),
    ]);
    for i in 0..2 {
        recording.push(Sample {
            memory_peak: 0,
            gpu_peak: None,
            at_ms: i * 1000,
            span_ms: 1000,
            cpu_us: 700_000,
            memory: 100 + i,
            gpu: Some(5 + i),
            frames: 60,
            frame_us: 60 * 4_000,
            frame_longest_us: 9_000,
            threads: vec![
                ThreadCpu { thread: "indexer".into(), us: 500_000 },
                ThreadCpu { thread: "git-status".into(), us: 100_000 },
                ThreadCpu { thread: "main".into(), us: 100_000 },
            ],
            triggers: vec![Trigger { owner: "caret".into(), cause: "deadline".into(), count: 60 }],
            busy: vec![Busy {
                owner: "log-viewer".into(),
                what: "paint".into(),
                calls: 60,
                us: 50_000,
                longest_us: 3_000,
            }],
            counters: vec![Counter::new("indexer", "files", 10)],
            levels: vec![Level {
                owner: "lsp".into(),
                what: "pending".into(),
                peak: 2 + i,
                last: i,
            }],
            io: vec![Io {
                owner: "http-client".into(),
                sent: 1 << 20,
                received: 3 << 20,
                ..Io::default()
            }],
            disk_read: 1 << 20,
            disk_written: 0,
            spans: vec![crate::format::Span {
                path: vec!["app·frame".into()],
                calls: 60,
                us: 200_000,
            }],
        });
    }
    let s = summary(&recording);
    assert_eq!(s.seconds, 2.0);
    assert!((s.cpu - 70.0).abs() < 1e-9);
    assert_eq!(s.modules[0].0, "code.index");
    assert!((s.modules[0].1 - 50.0).abs() < 1e-9);
    assert_eq!(s.threads[1], ("git-status".into(), Some("vcs.git".into()), 10.0));
    assert_eq!(s.triggers[0], ("caret".into(), "deadline".into(), 60.0));
    assert_eq!(s.busy[0].0, "log-viewer");
    assert!((s.busy[0].2 - 5.0).abs() < 1e-9);
    assert!((s.fps - 60.0).abs() < 1e-9);
    assert!((s.frame_ms - 4.0).abs() < 1e-9);
    assert_eq!((s.memory_peak, s.gpu_peak), (101, Some(6)));
    assert_eq!(s.counters, [("indexer".into(), "files".into(), "count".into(), 10.0, 20)]);
    assert_eq!(s.levels, [("lsp".into(), "pending".into(), 3, 1)]);
    // What stands out: busy with no input (worst first), the indexer's thread, the caret
    // asking 60 frames a second.
    assert!(matches!(s.findings[0], Finding::BusyWhileIdle { .. }));
    assert!(
        s.findings
            .iter()
            .any(|f| matches!(f, Finding::BusyThread { thread, .. } if thread == "indexer"))
    );
    assert!(
        s.findings
            .iter()
            .any(|f| matches!(f, Finding::FrameAsker { owner, .. } if owner == "caret"))
    );
    assert_eq!(s.spans, [(vec!["app·frame".to_owned()], 120, 400_000)]);
    assert_eq!(s.io[0].owner, "http-client");
    assert_eq!((s.io[0].sent, s.io[0].received), (2 << 20, 6 << 20));
    assert_eq!(s.disk_read, 2 << 20);
    assert!(
        s.findings
            .iter()
            .any(|f| matches!(f, Finding::BusyIo { owner, .. } if owner == "http-client"))
    );
    assert_eq!(summary(&Recording::default()), Summary::default());
}
