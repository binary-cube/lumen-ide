use super::*;

#[cfg(unix)]
use std::sync::Mutex;

/// A program's name only: its path, arguments and control characters never kept.
#[test]
fn only_the_programs_name_is_kept() {
    assert_eq!(program_name(Path::new("/usr/local/bin/phpactor")), "phpactor");
    assert_eq!(program_name(Path::new("C:\\Program Files\\Git\\bin\\git.exe")), "git");
    assert_eq!(program_name(Path::new("-zsh")), "zsh");
    assert_eq!(program_name(Path::new("node\u{1b}[31m")), "node[31m");
    assert_eq!(program_name(Path::new("")), "?");
    assert_eq!(program_name(Path::new(&"x".repeat(500))).chars().count(), 64);
}

/// The owner says the kind: servers and helpers stopped, the user's work asked about.
#[test]
fn the_kind_follows_the_owner() {
    assert_eq!(Kind::of_owner("lsp"), Kind::Server);
    assert_eq!(Kind::of_owner("driver"), Kind::Server);
    assert_eq!(Kind::of_owner("acp"), Kind::Agent);
    assert_eq!(Kind::of_owner("git"), Kind::Git);
    assert_eq!(Kind::of_owner("quality"), Kind::Other);
    for kind in [Kind::Terminal, Kind::Agent, Kind::Task] {
        assert!(kind.is_users());
        assert_eq!(Kind::from_id(kind.id()), kind);
    }
    for kind in [Kind::Server, Kind::Git, Kind::Other] {
        assert!(!kind.is_users());
        assert_eq!(Kind::from_id(kind.id()), kind);
    }
    assert_eq!(Kind::from_id("made-up"), Kind::Other);
}

/// What the recorder heard.
#[cfg(unix)]
#[derive(Default)]
struct Heard(Mutex<Vec<(u32, Option<Launch>)>>);

#[cfg(unix)]
impl Recorder for Heard {
    fn started(&self, launch: Launch) {
        self.0.lock().unwrap().push((launch.pid, Some(launch)));
    }
    fn ended(&self, pid: u32) {
        self.0.lock().unwrap().push((pid, None));
    }
}

/// A program started is told at once and its end when it's dropped; one run only once it
/// has run long enough; a short one never.
#[cfg(unix)]
#[test]
fn long_lived_programs_are_told_and_short_ones_not() {
    let heard = Arc::new(Heard::default());
    set_recorder(Arc::clone(&heard) as Arc<dyn Recorder>);
    let of = |pid: u32| -> Vec<Option<Launch>> {
        heard.0.lock().unwrap().iter().filter(|(p, _)| *p == pid).map(|(_, l)| l.clone()).collect()
    };
    let dir = std::env::temp_dir();
    let started = crate::start(&crate::Run::new("sleep", ["30"]).owner("lsp").dir(&dir)).unwrap();
    let pid = started.id();
    let told = of(pid);
    assert_eq!(told.len(), 1);
    let launch = told[0].clone().unwrap();
    assert_eq!((launch.kind, launch.program.as_str()), (Kind::Server, "sleep"));
    assert_eq!(launch.project.as_deref(), Some(dir.as_path()));
    assert!(launch.told_end);
    drop(started);
    assert_eq!(of(pid).last(), Some(&None), "its end told");

    // A short run: never told (nothing heard of a pid it had).
    let before = heard.0.lock().unwrap().len();
    crate::run(&crate::Run::shell("true").owner("git")).unwrap();
    let after: Vec<_> = heard.0.lock().unwrap()[before..]
        .iter()
        .filter_map(|(_, l)| l.as_ref().map(|l| l.kind))
        .collect();
    assert!(!after.contains(&Kind::Git));

    // A long one: told after a while, its end told when it ends.
    let before = heard.0.lock().unwrap().len();
    crate::run(&crate::Run::new("sleep", ["2.5"]).owner("git")).unwrap();
    let told: Vec<_> = heard.0.lock().unwrap()[before..].to_vec();
    let (pid, launch) = told
        .iter()
        .find(|(_, l)| l.as_ref().is_some_and(|l| l.kind == Kind::Git))
        .cloned()
        .expect("a long git run is told");
    assert_eq!(launch.unwrap().program, "sleep");
    assert!(told.iter().any(|(p, l)| *p == pid && l.is_none()), "its end told");
}
