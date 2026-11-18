//! A project's runs: one per project shared by whoever asks, a run begun and ended, the
//! gutter's marks following the latest run of each request while its line is unchanged.

use super::*;

const FILE: &str = "GET https://e.com/a\n\n###\nPOST https://e.com/b\n";

fn run(runs: &Runs, file: &Path, line: u32) -> RunId {
    let range = http_requests::parse(FILE)
        .requests
        .into_iter()
        .find(|r| r.line == line)
        .map(|r| r.line_range)
        .unwrap();
    runs.begin(Run {
        id: kernel::id::new_uuid(),
        number: 0,
        file: file.to_path_buf(),
        label: "x".into(),
        method: "GET".into(),
        hash: text_hash(&FILE[range.clone()]),
        line: range,
        line_number: line,
        state: RunState::Running,
        started: Instant::now(),
        cancel: Cancel::new(),
    })
}

#[test]
fn a_project_s_runs_are_shared_and_a_lone_file_has_its_own() {
    let project = ProjectId::new();
    let one = Runs::of(Some(project), None);
    let two = Runs::of(Some(project), Some(Path::new("/p")));
    assert!(Arc::ptr_eq(&one, &two));
    assert_eq!(one.root().as_deref(), Some(Path::new("/p")), "the root told later");
    assert!(Runs::held(project).is_some());
    let lone = Runs::of(None, None);
    assert!(!Arc::ptr_eq(&lone, &Runs::of(None, None)), "a file alone: its own");
    drop((one, two));
    // Another test may hold every project's runs a moment (the Task Manager's look).
    let started = Instant::now();
    while Runs::held(project).is_some() && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(Runs::held(project).is_none(), "gone with its last holder");
}

#[test]
fn the_marks_follow_each_request_s_latest_run() {
    let runs = Runs::of(Some(ProjectId::new()), None);
    let file = Path::new("/p/api.http");
    let states = |text: &str| {
        let parsed = http_requests::parse(text);
        runs.marks(file, &parsed, text).iter().map(|m| m.state).collect::<Vec<_>>()
    };
    assert_eq!(states(FILE), [MarkState::Runnable, MarkState::Runnable]);
    let first = run(&runs, file, 0);
    assert_eq!(states(FILE), [MarkState::Running, MarkState::Runnable]);
    assert!(runs.running(file));
    assert_eq!(runs.take_reveal(), Some(first), "the run just begun, to show");
    assert_eq!(runs.take_reveal(), None, "once");
    runs.end(first, RunState::Failed("no".into()));
    assert_eq!(states(FILE), [MarkState::Failed, MarkState::Runnable]);
    // Its request line edited: ready again.
    let edited = FILE.replacen("/a", "/a2", 1);
    assert_eq!(states(&edited), [MarkState::Runnable, MarkState::Runnable]);
    assert_eq!(runs.runs()[0].number, 1);
}

#[test]
fn stop_cancels_a_file_s_runs_and_the_environment_is_per_file() {
    let runs = Runs::of(Some(ProjectId::new()), None);
    let file = Path::new("/p/a.http");
    let id = run(&runs, file, 0);
    assert!(runs.stop(file));
    assert!(runs.run(id).unwrap().cancel.is_cancelled());
    assert!(!runs.stop(Path::new("/p/other.http")));
    assert_eq!(runs.environment(file), None);
    runs.choose_environment(file, Some("dev".into()));
    assert_eq!(runs.environment(file).as_deref(), Some("dev"));
    assert_eq!(runs.environment(Path::new("/p/other.http")), None);
}

#[test]
fn each_request_s_latest_state_is_read_at_once() {
    let runs = Runs::of(Some(ProjectId::new()), None);
    let file = Path::new("/p/api.http");
    let labels = ["x".to_owned(), "y".to_owned()];
    assert!(runs.states(file, &labels).iter().all(Option::is_none), "none run yet");
    let first = run(&runs, file, 0);
    runs.end(first, RunState::Failed("no".into()));
    run(&runs, file, 0);
    let states = runs.states(file, &labels);
    assert!(matches!(states[0], Some(RunState::Running)), "the newest of `x`");
    assert!(states[1].is_none());
    assert!(runs.states(Path::new("/p/other.http"), &labels)[0].is_none(), "another file's");
}

#[test]
fn past_the_budget_the_oldest_saved_bodies_are_left_in_their_files() {
    let done = |bytes: usize, saved: bool| {
        RunState::Done(Arc::new(Outcome {
            request: http_requests::vars::ResolvedRequest {
                method: "GET".into(),
                url: "https://e.com".into(),
                headers: Vec::new(),
                body: Vec::new(),
                secret: false,
                secrets: Vec::new(),
            },
            response: http_requests::transport::Incoming {
                status: 200,
                reason: "OK".into(),
                version: "HTTP/1.1".into(),
                headers: Vec::new(),
                body: http_requests::transport::IncomingBody::Memory(vec![b'x'; bytes]),
                url: "https://e.com".into(),
                hops: Vec::new(),
                timings: http_requests::transport::Timings::default(),
            },
            saved: saved.then(|| PathBuf::from("/p/saved.json")),
            kept: None,
            json: None,
            script: None,
        }))
    };
    let runs = Runs::of(Some(ProjectId::new()), None);
    let file = Path::new("/p/api.http");
    let ids: Vec<RunId> = (0..3).map(|_| run(&runs, file, 0)).collect();
    let mut all = runs.state();
    all.runs[0].state = done(40, false);
    all.runs[1].state = done(40, true);
    all.runs[2].state = done(40, true);
    lighten(&mut all.runs, 80);
    let in_memory: Vec<bool> = all
        .runs
        .iter()
        .map(|r| match &r.state {
            RunState::Done(o) => {
                matches!(o.response.body, http_requests::transport::IncomingBody::Memory(_))
            }
            _ => false,
        })
        .collect();
    assert_eq!(in_memory, [true, false, true], "not saved: kept; the oldest saved: let go");
    let RunState::Done(o) = &all.runs[1].state else { panic!() };
    assert!(matches!(
        &o.response.body,
        http_requests::transport::IncomingBody::File { path, len: 40 } if path == Path::new("/p/saved.json")
    ));
    drop(all);
    assert_eq!(ids.len(), 3);
}

/// Bodies with no file to be left in (`# @no-log`, small), past the budget: their JSON read
/// is let go (oldest first), the body kept; the memory told counts the JSON too.
#[test]
fn past_the_budget_unsaved_bodies_let_their_json_go() {
    let done = |bytes: usize| {
        RunState::Done(Arc::new(Outcome {
            request: http_requests::vars::ResolvedRequest {
                method: "GET".into(),
                url: "https://e.com".into(),
                headers: Vec::new(),
                body: Vec::new(),
                secret: false,
                secrets: Vec::new(),
            },
            response: http_requests::transport::Incoming {
                status: 200,
                reason: "OK".into(),
                version: "HTTP/1.1".into(),
                headers: Vec::new(),
                body: http_requests::transport::IncomingBody::Memory(vec![b'1'; bytes]),
                url: "https://e.com".into(),
                hops: Vec::new(),
                timings: http_requests::transport::Timings::default(),
            },
            saved: None,
            kept: None,
            json: Some(Arc::new(serde_json::json!(1))),
            script: None,
        }))
    };
    let runs = Runs::of(Some(ProjectId::new()), None);
    let file = Path::new("/p/api.http");
    for _ in 0..2 {
        run(&runs, file, 0);
    }
    let mut all = runs.state();
    all.runs[0].state = done(10);
    all.runs[1].state = done(10);
    drop(all);
    assert_eq!(runs.memory(), 100, "bodies and their JSON (five times)");
    let mut all = runs.state();
    lighten(&mut all.runs, 70);
    let json: Vec<bool> = all
        .runs
        .iter()
        .map(|r| matches!(&r.state, RunState::Done(o) if o.json.is_some()))
        .collect();
    assert_eq!(json, [false, true], "the oldest's JSON let go, its body kept");
    drop(all);
    assert_eq!(runs.memory(), 60);
}

#[test]
fn a_run_is_deleted_and_the_finished_ones_cleared() {
    let runs = Runs::of(Some(ProjectId::new()), None);
    let file = Path::new("/p/api.http");
    let (a, b, c) = (run(&runs, file, 0), run(&runs, file, 0), run(&runs, file, 0));
    runs.end(a, RunState::Failed("x".into()));
    runs.end(b, RunState::Failed("x".into()));
    let generation = runs.generation();
    assert!(runs.remove(a));
    assert!(!runs.remove(a), "gone already");
    assert!(runs.generation() > generation, "told");
    assert_eq!(runs.clear_finished(), 1, "b; c still runs");
    assert_eq!(runs.runs().len(), 1);
    assert_eq!(runs.runs()[0].id, c);
}

#[test]
fn a_streamed_response_keeps_its_last_part() {
    let runs = Runs::of(Some(ProjectId::new()), None);
    let id = run(&runs, Path::new("/p/a.http"), 0);
    let piece = vec![b'a'; 64 * 1024];
    for _ in 0..40 {
        runs.append_live(id, &piece);
    }
    runs.append_live(id, b"end");
    let live = runs.live(id).unwrap();
    assert_eq!(live.len(), LIVE_KEPT);
    assert!(live.ends_with("aend"));
}

/// What comes while a run goes is counted apart: its view reads it, while the bars, the
/// editors and Services (following the runs' generation) aren't made to redo their work.
#[test]
fn what_comes_while_a_run_goes_moves_only_its_own_count() {
    let runs = Runs::of(Some(ProjectId::new()), None);
    let id = run(&runs, Path::new("/p/a.http"), 0);
    let (generation, live) = (runs.generation(), runs.live_generation());
    runs.append_live(id, b"data: one\n\n");
    assert_eq!(runs.generation(), generation, "the runs as they were");
    assert!(runs.live_generation() > live, "its view told");
}

#[test]
fn a_not_logged_body_with_its_private_copy_can_be_left_in_it() {
    let outcome = |kept: Option<&str>| Outcome {
        request: http_requests::vars::ResolvedRequest {
            method: "GET".into(),
            url: "https://e.com".into(),
            headers: Vec::new(),
            body: Vec::new(),
            secret: false,
            secrets: Vec::new(),
        },
        response: http_requests::transport::Incoming {
            status: 200,
            reason: "OK".into(),
            version: "HTTP/1.1".into(),
            headers: Vec::new(),
            body: http_requests::transport::IncomingBody::Memory(vec![b'x'; 10]),
            url: "https://e.com".into(),
            hops: Vec::new(),
            timings: http_requests::transport::Timings::default(),
        },
        saved: None,
        kept: kept.map(PathBuf::from),
        json: None,
        script: None,
    };
    let light = outcome(Some("/p/bodies/x.body")).lightened().expect("its copy");
    assert!(matches!(
        &light.response.body,
        http_requests::transport::IncomingBody::File { path, len: 10 } if path == Path::new("/p/bodies/x.body")
    ));
    assert!(light.saved.is_none(), "still not in the history");
    assert!(outcome(None).lightened().is_none(), "no copy: it stays");
}
