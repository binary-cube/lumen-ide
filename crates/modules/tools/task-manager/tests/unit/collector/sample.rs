use super::*;
use std::time::Duration;

fn part(name: &str, memory: Option<u64>, threads: Vec<&'static str>) -> ResourceUse {
    let mut part = ResourceUse::new(name, "app");
    part.memory = memory;
    part.threads = threads;
    part
}

#[test]
fn threads_count_for_their_part_and_the_rest_is_shown_too() {
    let snapshot = ResourceSnapshot {
        memory: 1000,
        gpu: Some(50),
        parts: vec![
            part("Index", Some(300), vec!["indexer", "symbols"]),
            part("Git", None, vec!["git-"]),
        ],
        ..ResourceSnapshot::default()
    };
    let threads = vec![
        ("indexer".to_owned(), 10.0),
        ("symbols".to_owned(), 5.0),
        ("git-log".to_owned(), 2.5),
        ("mystery".to_owned(), 1.0),
    ];
    let labels = FrameLabels {
        other_threads: "Other threads".into(),
        unattributed: "Not attributed".into(),
        outside_heap: "Code, GPU, system".into(),
    };
    let table = frame(snapshot, &threads, &HashMap::new(), &HashMap::new(), &labels);
    let line = |name: &str| table.lines.iter().find(|l| l.name == name).unwrap();
    assert_eq!((line("Index").cpu, line("Index").threads), (15.0, 2));
    assert_eq!(line("Git").cpu, 2.5);
    // Each part's threads by name with their CPU (the detail lists them).
    assert_eq!(
        line("Index").thread_cpu,
        [("indexer".to_owned(), 10.0), ("symbols".to_owned(), 5.0)]
    );
    assert_eq!(line("Other threads").thread_cpu, [("mystery".to_owned(), 1.0)]);
    assert_eq!(
        (line("Other threads").cpu, line("Other threads").detail.as_str()),
        (1.0, "mystery")
    );
    assert_eq!(line("Not attributed").memory, Some(700), "the process's memory nobody reported");
    assert_eq!((table.cpu, table.memory, table.gpu, table.threads), (18.5, 1000, Some(50), 4));
}

#[test]
fn cpu_is_the_time_used_since_the_last_look() {
    let mut sampler = Sampler::default();
    let at = Instant::now();
    let thread = |cpu: u64| process_stats::ThreadTime {
        id: 7,
        name: "syntax".into(),
        cpu: Duration::from_millis(cpu),
    };
    assert_eq!(sampler.sample(&[thread(1000)], at)[0].1, 0.0, "nothing to compare with yet");
    let busy = sampler.sample(&[thread(1500)], at + Duration::from_secs(1));
    assert!((busy[0].1 - 50.0).abs() < 0.01, "half a core: {busy:?}");
}

#[test]
fn the_network_and_disk_rates_are_the_bytes_since_the_last_look() {
    let mut rates = Rates::default();
    let at = Instant::now();
    let shop = kernel::ProjectId::new();
    let mut remote = ResourceUse::new("Remote: shop", "remote.projects");
    remote.project = Some(shop);
    remote.sent = Some(1000);
    remote.received = Some(0);
    remote.written = Some(0);
    let key = key("remote.projects", "Remote: shop", Some(shop));
    let (first, process) =
        rates.sample(std::slice::from_ref(&remote), &HashMap::new(), Some((10, 10)), at);
    assert_eq!((first[&key].net, first[&key].disk, process), (Some(0.0), Some(0.0), Some(0.0)));
    remote.received = Some(4096);
    remote.written = Some(4096);
    let (later, process) = rates.sample(
        std::slice::from_ref(&remote),
        &HashMap::new(),
        Some((10, 2058)),
        at + Duration::from_secs(2),
    );
    assert_eq!(later[&key].net, Some(2048.0), "4 KB over 2 s");
    assert_eq!(later[&key].disk, Some(2048.0), "written as received");
    assert_eq!(process, Some(1024.0), "the process's disk: 2 KB over 2 s");
    // Parts that count nothing aren't counted.
    assert!(
        rates.sample(&[ResourceUse::new("Git", "app")], &HashMap::new(), None, at).0.is_empty()
    );
}

/// Two projects with the same name stay two rows (their identity is in the key); a part
/// measuring its own CPU gets that, its shared threads claimed, not added.
#[test]
fn projects_are_kept_apart_and_measured_cpu_counts() {
    let (a, b) = (kernel::ProjectId::new(), kernel::ProjectId::new());
    let index = |id, cpu: u64| {
        let mut p = ResourceUse::new("Index: shop", "indexing");
        p.project = Some(id);
        p.threads = vec!["indexer"];
        p.cpu = Some(Duration::from_millis(cpu));
        p
    };
    let mut rates = Rates::default();
    let at = Instant::now();
    rates.sample(&[index(a, 0), index(b, 0)], &HashMap::new(), None, at);
    let parts = vec![index(a, 500), index(b, 100)];
    let (measured, _) = rates.sample(&parts, &HashMap::new(), None, at + Duration::from_secs(1));
    let snapshot = ResourceSnapshot { parts, ..ResourceSnapshot::default() };
    let threads = vec![("indexer".to_owned(), 60.0), ("indexer".to_owned(), 10.0)];
    let labels = FrameLabels {
        other_threads: "Other".into(),
        unattributed: "Unattributed".into(),
        outside_heap: "Code, GPU, system".into(),
    };
    let table = frame(snapshot, &threads, &measured, &HashMap::new(), &labels);
    let line = |id| table.lines.iter().find(|l| l.project == Some(id)).unwrap();
    assert_ne!(line(a).key(), line(b).key(), "the same name, two projects");
    assert!((line(a).cpu - 50.0).abs() < 0.5, "its own measure: {}", line(a).cpu);
    assert!((line(b).cpu - 10.0).abs() < 0.5, "its own measure: {}", line(b).cpu);
    assert!(table.lines.iter().all(|l| l.name != "Other"), "the shared threads are claimed");
}

/// A part's programs (here: this test itself, with whatever it started): their memory and
/// disk added to the part's, their CPU time returned for its rate.
#[test]
fn a_parts_programs_count_for_it() {
    let mut part = ResourceUse::new("Terminal", "terminal");
    part.processes = vec![std::process::id()];
    let mut trees = process_stats::ProcessTrees::default();
    let mut parts = [part];
    let (cpu, used) = add_programs(&mut trees, &mut parts);
    assert!(parts[0].memory.is_some_and(|m| m > 0), "{:?}", parts[0].memory);
    assert!(cpu.contains_key(&key("terminal", "Terminal", None)));
    // Its own row: its id, its name, its memory; no CPU the first time (nothing to compare).
    let mut rates = ProcessRates::default();
    let at = Instant::now();
    let rows = rates.sample(&used, |pid| (trees.name(pid), trees.invoked(pid)), at);
    let me = &rows[&std::process::id()];
    assert!(me.memory > 0 && me.cpu == 0.0, "{me:?}");
    assert_eq!(me.name, process_stats::process_name(std::process::id()));
    // A part's rows in the frame.
    let snapshot = ResourceSnapshot { parts: parts.to_vec(), ..ResourceSnapshot::default() };
    let labels = FrameLabels {
        other_threads: "Other".into(),
        unattributed: "Unattributed".into(),
        outside_heap: "Code, GPU, system".into(),
    };
    let frame = frame(snapshot, &[], &HashMap::new(), &rows, &labels);
    let terminal = frame.lines.iter().find(|l| l.name == "Terminal").unwrap();
    assert_eq!(terminal.processes.len(), 1);
    assert_eq!(terminal.processes[0].pid, std::process::id());
    // Programs that ended aren't counted (nothing to add).
    let mut gone = [ResourceUse { processes: vec![u32::MAX - 1], ..ResourceUse::new("x", "y") }];
    let (_, used) = add_programs(&mut trees, &mut gone);
    assert_eq!(gone[0].memory, Some(0));
    assert!(used.is_empty());
}

/// A program's CPU and disk are its use since the last look.
#[test]
fn a_programs_rates_are_since_the_last_look() {
    let mut rates = ProcessRates::default();
    let at = Instant::now();
    let tree = |cpu: u64, read: u64| process_stats::TreeUse {
        cpu: Duration::from_millis(cpu),
        memory: 100,
        read,
        written: 0,
        ..Default::default()
    };
    let first =
        rates.sample(&HashMap::from([(7, tree(1000, 0))]), |_| (Some("git".into()), None), at);
    assert_eq!((first[&7].cpu, first[&7].disk), (0.0, 0.0));
    let later = rates.sample(
        &HashMap::from([(7, tree(1500, 4096))]),
        |_| (Some("git".into()), None),
        at + Duration::from_secs(2),
    );
    assert!((later[&7].cpu - 25.0).abs() < 0.01, "half a second over two: {}", later[&7].cpu);
    assert_eq!(later[&7].disk, 2048.0);
    assert_eq!(later[&7].name.as_deref(), Some("git"));
}

/// A language server's part: listed in its group, under its project, with its program
/// (memory and CPU, its children's too) and what its reporter knows (its state, its
/// latency) for the detail.
#[test]
fn a_language_server_is_listed_with_its_program_and_its_facts() {
    let project = kernel::ProjectId::new();
    let mut server = ResourceUse::new("vtsls", "lsp");
    server.group = workbench_model::resources::LANGUAGE_SERVERS;
    server.project = Some(project);
    server.processes = vec![std::process::id()];
    server.facts = vec![
        ("State".into(), "ready".into()),
        ("Latency (p50 / p95)".into(), "12 ms / 40 ms".into()),
    ];
    server.actions = vec![workbench_model::PartAction::new(
        "restart",
        workbench_model::ActionKind::Restart,
        workbench_model::ActionTarget::Part,
    )];
    let mut trees = process_stats::ProcessTrees::default();
    let mut parts = [server];
    let (_, used) = add_programs(&mut trees, &mut parts);
    assert!(parts[0].memory.is_some_and(|m| m > 0));
    let rows = ProcessRates::default().sample(
        &used,
        |pid| (trees.name(pid), trees.invoked(pid)),
        Instant::now(),
    );
    let snapshot = ResourceSnapshot { parts: parts.to_vec(), ..ResourceSnapshot::default() };
    let labels = FrameLabels {
        other_threads: "Other".into(),
        unattributed: "Unattributed".into(),
        outside_heap: "Code, GPU, system".into(),
    };
    let frame = frame(snapshot, &[], &HashMap::new(), &rows, &labels);
    let line = frame.lines.iter().find(|l| l.name == "vtsls").unwrap();
    assert_eq!(line.group, workbench_model::resources::LANGUAGE_SERVERS);
    assert_eq!(line.project, Some(project));
    assert_eq!(line.processes[0].pid, std::process::id());
    assert_eq!(line.facts[1].1, "12 ms / 40 ms");
    assert_eq!(line.actions[0].kind, workbench_model::ActionKind::Restart);
}

/// The terminals (every project's): each program names its project, this window's or
/// another window's; one of none says none.
#[test]
fn a_terminal_s_programs_name_their_projects() {
    let open = |name: &str| workbench_model::OpenProject {
        id: kernel::ProjectId::new(),
        name: name.into(),
        root: std::path::PathBuf::from("/w").join(name),
    };
    let (here, there) = (open("php-editor"), open("api.acme.test"));
    let mut terminal = ResourceUse::new("Terminal", "terminal");
    terminal.processes = vec![1, 2, 3];
    terminal.process_projects = vec![(1, here.id), (2, there.id)];
    let row = |pid| ProcessRow { pid, ..ProcessRow::default() };
    let rows: HashMap<u32, ProcessRow> = [(1, row(1)), (2, row(2)), (3, row(3))].into();
    let snapshot = ResourceSnapshot {
        parts: vec![terminal],
        projects: vec![here],
        elsewhere: vec![there],
        ..ResourceSnapshot::default()
    };
    let labels = FrameLabels::default();
    let frame = frame(snapshot, &[], &HashMap::new(), &rows, &labels);
    let line = frame.lines.iter().find(|l| l.name == "Terminal").unwrap();
    let projects: Vec<Option<&str>> = line.processes.iter().map(|p| p.project.as_deref()).collect();
    assert_eq!(projects, [Some("php-editor"), Some("api.acme.test"), None]);
}

/// With the heap counted, the memory nobody reported splits into the data nobody reported
/// and what isn't data (the code, the GPU's, the system's), which carries the GPU's.
#[test]
fn the_counted_heap_splits_what_nobody_reported() {
    const MB: u64 = 1 << 20;
    let labels = FrameLabels {
        other_threads: "Other".into(),
        unattributed: "Unattributed".into(),
        outside_heap: "Code, GPU, system".into(),
    };
    let snapshot = ResourceSnapshot {
        memory: 180 * MB,
        heap: Some(30 * MB),
        gpu: Some(80 * MB),
        parts: vec![part("Index", Some(20 * MB), Vec::new())],
        ..ResourceSnapshot::default()
    };
    let table = frame(snapshot, &[], &HashMap::new(), &HashMap::new(), &labels);
    let outside =
        table.line(&key(workbench_model::resources::APP, "Code, GPU, system", None)).unwrap();
    assert_eq!((outside.memory, outside.gpu), (Some(150 * MB), Some(80 * MB)));
    let unattributed = table.lines.last().unwrap();
    assert_eq!((unattributed.name.as_str(), unattributed.memory), ("Unattributed", Some(10 * MB)));
}
