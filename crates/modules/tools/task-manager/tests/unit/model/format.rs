use super::*;
use crate::collector::Line;
use workbench_model::resources::APP;

#[test]
fn rates_read_well() {
    assert_eq!(rate(0.0), "0 B/s");
    assert_eq!(rate(2048.0), "2 KB/s");
    assert_eq!(percent(3.44), i18n::decimal("3.4%".into()));
    assert_eq!(duration(Duration::from_secs(125)), "2 min 5 s");
}

/// A part's numbers are written once: a dash where it measures nothing, its CPU only when
/// threads were counted for it.
#[test]
fn a_parts_numbers_are_written_once() {
    let mut line = Line::new("Index", "app", APP, None);
    line.cpu = 12.5;
    line.net = Some(2048.0);
    let texts = PartTexts::of(&line);
    assert_eq!(texts.cpu, None, "no thread counted: nothing measured");
    assert_eq!((texts.memory, texts.net.as_deref()), (None, Some("2 KB/s")));
    line.threads = 2;
    line.memory = Some(1024 * 1024);
    let texts = PartTexts::of(&line);
    assert_eq!(texts.cpu.as_deref(), Some(i18n::decimal("12.5%".into()).as_str()));
    assert_eq!(texts.memory, Some(i18n::bytes(1024 * 1024)));
}

/// A part measured by the programs it started (a language server): its processes' CPU is
/// its own, written like a part's with threads.
#[test]
fn a_part_with_programs_shows_their_cpu() {
    let mut line = Line::new("phpactor", "lsp", "language-servers", None);
    line.cpu = 0.1;
    line.processes = vec![crate::collector::ProcessRow {
        pid: 9127,
        name: Some("php".into()),
        cpu: 0.1,
        memory: 6 * 1024 * 1024,
        disk: 0.0,
        ..Default::default()
    }];
    let texts = PartTexts::of(&line);
    assert_eq!(texts.cpu.as_deref(), Some(i18n::decimal("0.1%".into()).as_str()));
    assert_eq!(texts.threads, None);
}

fn operation(subject: &str, kind: ActivityKind, done: u64, total: Option<u64>) -> Activity {
    Activity {
        id: kernel::id::new_uuid(),
        project: None,
        kind,
        subject: subject.into(),
        detail: String::new(),
        text: None,
        unit: ActivityUnit::Bytes,
        done,
        total,
        started: Instant::now() - Duration::from_secs(2),
        ended: None,
        stoppable: false,
    }
}

#[test]
fn an_operation_says_how_far_and_how_it_ended() {
    let now = Instant::now();
    let mut a = operation("big.zip", ActivityKind::Download, 1024 * 1024, Some(4 * 1024 * 1024));
    a.started = now - Duration::from_secs(1);
    let running = status(&a, now);
    assert!(running.starts_with("25%"), "{running}");
    assert!(running.contains("MB/s") || running.contains("KB/s"), "its speed: {running}");
    let mb = |n: u64| i18n::bytes(n * 1024 * 1024);
    assert_eq!(progress(&a), format!("{} / {}", mb(1), mb(4)), "in the language's numbers");
    a.ended = Some((now, Outcome::Done));
    assert_eq!(status(&a, now), tr!("tasks.op_done", time = "1 s"));
    a.ended = Some((now, Outcome::Stopped));
    assert_eq!(status(&a, now), t("tasks.op_stopped"));
    assert_eq!(kind_icon(ActivityKind::Upload), "upload");
}

/// A program's row says what it is whatever it runs: the name it was started by beside its
/// program's when they differ, its id, how long it has run, its tree's programs.
#[test]
fn a_program_s_row_says_what_it_is() {
    let row = |invoked: Option<&str>, name: Option<&str>, secs: u64, processes: u32| {
        ProcessTexts::of(&crate::collector::ProcessRow {
            pid: 71540,
            name: name.map(Into::into),
            invoked: invoked.map(Into::into),
            running: Duration::from_secs(secs),
            processes,
            ..Default::default()
        })
        .name
    };
    let tree = |n| i18n::tr!("tasks.process_tree", count = n);
    assert_eq!(
        row(Some("claude"), Some("2.1.325"), 5 * 3600 + 12 * 60, 4),
        format!("claude (2.1.325) · 71540 · 5 h 12 min · {}", tree(4))
    );
    assert_eq!(row(Some("zsh"), Some("zsh"), 42, 1), "zsh · 71540 · 42 s", "the same name once");
    assert_eq!(row(None, Some("git"), 0, 1), "git · 71540");
    assert_eq!(row(Some("tool"), None, 600, 1), "tool · 71540 · 10 min");
    assert_eq!(row(None, None, 0, 1), "71540");
    assert_eq!(running(Duration::from_secs(52 * 3600 + 180)), "52 h 3 min");
}

/// A terminal's program says the project it works for, after what it is.
#[test]
fn a_program_s_row_says_its_project() {
    let texts = ProcessTexts::of(&crate::collector::ProcessRow {
        pid: 76008,
        name: Some("2.1.327".into()),
        invoked: Some("claude".into()),
        project: Some("api.acme.test".into()),
        ..Default::default()
    });
    assert_eq!(texts.name, "claude (2.1.327) · api.acme.test · 76008");
}
