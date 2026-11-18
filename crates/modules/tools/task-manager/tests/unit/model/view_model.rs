use super::*;
use crate::collector::ProcessRow;
use std::time::Duration;
use workbench_model::resources::{APP, INDEXING, MODULES, PROGRAMS, SERVICES, TERMINALS};
use workbench_model::{ActivityKind, ActivityUnit, Outcome};

fn line(name: &str, owner: &str, group: &'static str, cpu: f32, memory: u64) -> Line {
    let mut line = Line::new(name, owner, group, None);
    line.cpu = cpu;
    line.memory = Some(memory);
    line.threads = 1;
    line
}

fn of(mut l: Line, id: ProjectId) -> Line {
    l = Line { project: Some(id), ..l };
    l.key = crate::collector::key(&l.owner, &l.name, l.project);
    l
}

fn frame() -> Arc<Frame> {
    let mut git = line("Git", "vcs.git", PROGRAMS, 2.0, 50);
    git.processes = vec![ProcessRow {
        pid: 4242,
        name: Some("git-remote-https".into()),
        cpu: 1.0,
        memory: 10,
        disk: 0.0,
        ..Default::default()
    }];
    Arc::new(Frame {
        lines: vec![
            line("Interface", "app", APP, 40.0, 5),
            line("Index: shop", "indexing", INDEXING, 12.0, 30),
            line("Terminal", "terminal", TERMINALS, 0.5, 0),
            git,
            line("Remote: shop", "remote.projects", SERVICES, 1.0, 80),
            line("PHP", "lang.php", MODULES, 0.0, 0),
        ],
        ..Frame::default()
    })
}

fn build(frame: &Arc<Frame>, state: &ViewState) -> ViewModel {
    ViewModel::build(Arc::clone(frame), state, (1, 1), Instant::now())
}

fn names(model: &ViewModel) -> Vec<String> {
    model.shown_lines().map(|l| l.name.clone()).collect()
}

fn groups_of(model: &ViewModel) -> Vec<GroupKey> {
    model
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Group { key, .. } => Some(*key),
            Item::Part { .. } | Item::Process { .. } | Item::Operation(_) => None,
        })
        .collect()
}

/// By kind: the app's core, indexing, terminals, external programs, connections, modules
/// (folded), each with its totals written.
#[test]
fn the_parts_are_grouped_by_kind() {
    let frame = frame();
    let model = build(&frame, &ViewState::default());
    assert_eq!(
        groups_of(&model),
        [APP, INDEXING, TERMINALS, PROGRAMS, SERVICES, MODULES].map(GroupKey::Kind)
    );
    assert!(!model.keys.contains(&"lang.php/PHP".to_owned()), "the modules start folded");
    let Item::Group { cpu_text, memory_text, count, .. } = &model.items[0] else { panic!() };
    assert_eq!(
        (cpu_text.as_str(), memory_text.as_deref(), *count),
        (i18n::decimal("40.0%".into()).as_str(), Some(i18n::bytes(5).as_str()), 1)
    );
    // As one list: no groups, every part (the modules too), sorted.
    let list = build(&frame, &ViewState { grouping: Grouping::List, ..ViewState::default() });
    assert!(groups_of(&list).is_empty());
    assert_eq!(list.keys.len(), 6);
    assert_eq!(list.keys[0], "app/Interface", "the busiest first");
    // By project asked on this project's view: by kind.
    let project = build(&frame, &ViewState { grouping: Grouping::Project, ..ViewState::default() });
    assert_eq!(groups_of(&project), groups_of(&model));
}

/// Every open project: by project — a group each (by identity: two named alike stay
/// apart), then what they share, then the modules; by kind, every project's parts in their
/// kinds.
#[test]
fn every_project_is_grouped_by_its_identity() {
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let frame = Arc::new(Frame {
        all: true,
        projects: vec![
            workbench_model::OpenProject { id: a, name: "shop".into(), root: "/a/shop".into() },
            workbench_model::OpenProject { id: b, name: "shop".into(), root: "/b/shop".into() },
        ],
        lines: vec![
            of(line("Index: shop", "indexing", INDEXING, 5.0, 10), a),
            of(line("Index: shop", "indexing", INDEXING, 1.0, 10), b),
            line("Interface", "app", APP, 40.0, 5),
            line("PHP", "lang.php", MODULES, 0.0, 0),
        ],
        ..Frame::default()
    });
    let state = ViewState { grouping: Grouping::Project, ..ViewState::default() };
    let model = build(&frame, &state);
    assert_eq!(
        groups_of(&model),
        [GroupKey::Project(a), GroupKey::Project(b), GroupKey::Shared, GroupKey::Kind(MODULES)]
    );
    assert_eq!(model.keys.len(), 3, "two projects' indexes and the interface: {:?}", model.keys);
    assert_ne!(model.keys[0], model.keys[1], "the same name, two projects");
    assert_eq!(model.keys[2], "app/Interface", "what they share, after them");
    let by_kind = build(&frame, &ViewState::default());
    assert_eq!(groups_of(&by_kind), [APP, INDEXING, MODULES].map(GroupKey::Kind));
    assert_eq!(by_kind.keys.iter().filter(|k| k.starts_with("indexing/")).count(), 2);
}

/// Sorted by a column, the largest first (names A to Z); parts alike keep the look's order
/// (nothing jumps between looks).
#[test]
fn the_sorting_is_stable() {
    let mut lines: Vec<Line> = (0..6).map(|i| line(&format!("p{i}"), "m", APP, 1.0, 7)).collect();
    lines[3].cpu = 9.0;
    let frame = Arc::new(Frame { lines, ..Frame::default() });
    let model = build(&frame, &ViewState::default());
    assert_eq!(names(&model), ["p3", "p0", "p1", "p2", "p4", "p5"]);
    let by_memory = ViewState { sort: (Column::Memory, true), ..ViewState::default() };
    assert_eq!(names(&build(&frame, &by_memory)), ["p0", "p1", "p2", "p3", "p4", "p5"]);
    let by_name = ViewState { sort: (Column::Name, false), ..ViewState::default() };
    assert_eq!(names(&build(&frame, &by_name))[0], "p0");
}

/// The filter finds a part by its name, owner or detail, a program's id or name; filtering
/// opens every group.
#[test]
fn the_filter_finds_names_owners_details_and_programs() {
    let mut frame = (*frame()).clone();
    frame.lines[4].detail = "3 transfers logged".into();
    let frame = Arc::new(frame);
    let found = |text: &str| {
        let state = ViewState { filter: text.into(), ..ViewState::default() };
        names(&build(&frame, &state))
    };
    assert_eq!(found("interface"), ["Interface"]);
    assert_eq!(found("lang.php"), ["PHP"], "by its owner (its group folded, opened)");
    assert_eq!(found("transfers"), ["Remote: shop"], "by its detail");
    assert_eq!(found("4242"), ["Git"], "by a program's id");
    assert_eq!(found("remote-https"), ["Git"], "by a program's name");
    let state = ViewState { filter: "php".into(), ..ViewState::default() };
    assert!(build(&frame, &state).keys.contains(&"lang.php/PHP".to_owned()));
}

/// A selection is known by its part's key: it survives the looks; a part gone keeps its
/// last values, said gone.
#[test]
fn a_selection_survives_the_looks_and_a_part_gone_is_said() {
    let first = frame();
    let key = "remote.projects/Remote: shop";
    let selected = selection(&first, key, None).unwrap();
    assert!(!selected.gone && selected.line.memory == Some(80));
    let mut next = (*first).clone();
    next.lines.reverse();
    next.lines.iter_mut().for_each(|l| l.memory = Some(1));
    let again = selection(&next, key, Some(&selected.line)).unwrap();
    assert_eq!((again.gone, again.line.memory), (false, Some(1)), "the same part, now");
    let gone = Frame { lines: vec![line("Interface", "app", APP, 1.0, 1)], ..Frame::default() };
    let left = selection(&gone, key, Some(&again.line)).unwrap();
    assert!(left.gone, "gone: said so");
    assert_eq!(left.line.memory, Some(1), "with its last values");
    assert!(selection(&gone, "nobody", None).is_none());
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

/// The operations above the parts: the running ones first, then the last ended (at most
/// ten), their texts written; the filter finds them.
#[test]
fn operations_are_listed_above_the_parts() {
    let mut remote = line("Remote: shop", "remote.projects", SERVICES, 1.0, 80);
    let running = operation("index.php", ActivityKind::Upload, 512, Some(1024));
    let mut ended: Vec<Activity> = (0..15)
        .map(|i| {
            let mut a = operation(&format!("old-{i}.css"), ActivityKind::Download, 10, Some(10));
            a.ended = Some((Instant::now() - Duration::from_secs(20 - i), Outcome::Done));
            a
        })
        .collect();
    let mut failed = operation("broken.js", ActivityKind::Upload, 0, Some(5));
    failed.ended = Some((Instant::now(), Outcome::Failed("timeout".into())));
    ended.push(failed);
    remote.activities = std::iter::once(running).chain(ended).collect();
    let frame = Arc::new(Frame { lines: vec![remote], ..Frame::default() });
    let model = build(&frame, &ViewState::default());
    let shown: Vec<&OperationRow> = model.operations().collect();
    assert_eq!(shown.len(), 1 + ENDED_SHOWN, "the running one and the last ten ended");
    assert_eq!(shown[0].activity.subject, "index.php", "the running first");
    assert_eq!(shown[1].activity.subject, "broken.js", "then the newest ended");
    assert!(shown[0].status.starts_with("50%"), "written: {}", shown[0].status);
    assert_eq!(shown[0].kind, i18n::t("tasks.op_upload"));
    assert_eq!(shown[0].owner, "remote.projects/Remote: shop");
    assert!(matches!(
        model.items[0],
        Item::Group { key: GroupKey::Operations, running: Some(1), .. }
    ));
    let state = ViewState { filter: "broken".into(), ..ViewState::default() };
    assert_eq!(build(&frame, &state).operations().count(), 1);
}

/// `n` parts in every group, a few with programs and operations (a busy app).
fn many(n: usize) -> Arc<Frame> {
    let groups = [APP, INDEXING, TERMINALS, PROGRAMS, SERVICES, MODULES];
    let lines = (0..n)
        .map(|i| {
            let mut l = line(
                &format!("part {i}"),
                "owner",
                groups[i % groups.len()],
                i as f32 % 37.0,
                i as u64 * 4096,
            );
            if i % 10 == 0 {
                l.processes = vec![ProcessRow {
                    pid: i as u32,
                    name: Some("program".into()),
                    cpu: 1.0,
                    memory: 10,
                    disk: 0.0,
                    ..Default::default()
                }];
            }
            l
        })
        .collect();
    Arc::new(Frame { lines, ..Frame::default() })
}

/// The median of `runs` timings of `work`.
fn median(runs: usize, mut work: impl FnMut()) -> Duration {
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .collect();
    times.sort();
    times[runs / 2]
}

/// The budget of a look on the UI thread (plan 40, "Performanță"): the list built again —
/// grouped, filtered, sorted, its texts written — in at most 0.3 ms for 200 parts and 1 ms
/// for 1 000 (a release build; measured, not run, in debug).
#[test]
#[cfg_attr(debug_assertions, ignore = "a budget: cargo test -p task-manager --release budget")]
fn budget_a_look_builds_the_list_in_time() {
    for (parts, budget) in [(200, Duration::from_micros(300)), (1000, Duration::from_millis(1))] {
        let frame = many(parts);
        let state = ViewState::default();
        let now = Instant::now();
        let took = median(31, || {
            let model = ViewModel::build(Arc::clone(&frame), &state, (1, 1), now);
            std::hint::black_box(model);
        });
        eprintln!("budget: the list of {parts} parts built in {took:?}");
        assert!(took <= budget, "{parts} parts: {took:?} > {budget:?}");
    }
}
