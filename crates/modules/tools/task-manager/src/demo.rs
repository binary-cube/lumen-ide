//! A Task Manager made up, the same every time (the app's `--snapshot` and the tests): fixed
//! parts in every group — with programs, operations and threads —, two projects for the
//! view of every project, and a history of fixed series, its clock stopped. Nothing is
//! measured: a picture of it is the same at each run.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use i18n::{t, tr};
use kernel::ProjectId;
use workbench_model::resources::{
    APP, INDEXING, LANGUAGE_SERVERS, MODULES, PROGRAMS, SERVICES, TERMINALS,
};
use workbench_model::{
    ActionKind, ActionTarget, Activity, ActivityKind, ActivityUnit, OpenProject, Outcome,
    PartAction,
};

use crate::collector::{Frame, History, Line, ProcessRow};

const MB: u64 = 1024 * 1024;

/// The two projects (their identities made once: the same for every look of a run).
pub fn projects() -> [OpenProject; 2] {
    static PROJECTS: std::sync::OnceLock<[ProjectId; 2]> = std::sync::OnceLock::new();
    let [shop, blog] = *PROJECTS.get_or_init(|| [ProjectId::new(), ProjectId::new()]);
    [
        OpenProject { id: shop, name: "shop".into(), root: "/demo/shop".into() },
        OpenProject { id: blog, name: "blog".into(), root: "/demo/blog".into() },
    ]
}

/// A part of the demo: its name, who it belongs to, its group and project, its CPU and
/// memory, its threads and what it says.
fn part(
    name: &str,
    owner: &str,
    group: &'static str,
    project: Option<ProjectId>,
    (cpu, memory): (f32, Option<u64>),
    threads: &[(&str, f32)],
    detail: &str,
) -> Line {
    let mut line = Line::new(name, owner, group, project);
    line.cpu = cpu;
    line.memory = memory;
    line.detail = detail.to_owned();
    line.threads = threads.len();
    line.thread_cpu = threads.iter().map(|(n, c)| ((*n).to_owned(), *c)).collect();
    line
}

fn process(pid: u32, name: &str, cpu: f32, memory: u64, disk: f32) -> ProcessRow {
    ProcessRow { pid, name: Some(name.to_owned()), cpu, memory, disk, ..ProcessRow::default() }
}

/// A project's own parts.
fn project_parts(project: &OpenProject, now: Instant, busy: f32) -> Vec<Line> {
    let id = Some(project.id);
    let name = &project.name;
    let mut index = part(
        &tr!("tasks.project_index", project = name.as_str()),
        "indexing",
        INDEXING,
        id,
        (12.4 * busy, Some(64 * MB)),
        &[("indexer", 9.1 * busy), ("symbols", 3.3 * busy)],
        &tr!("tasks.project_index_detail", files = 1204, merged = 3, received = 4),
    );
    index.disk = Some(1.2 * MB as f32 * busy);
    let mut terminal = part(
        &t("tasks.terminal"),
        "terminal",
        TERMINALS,
        id,
        (0.4, Some(18 * MB)),
        &[("pty-reader", 0.4)],
        &tr!("tasks.terminal_detail", count = 2),
    );
    terminal.processes = vec![
        process(4242, "zsh", 0.1, 6 * MB, 0.0),
        process(4243, "node", 6.2 * busy, 212 * MB, 48.0 * 1024.0),
    ];
    terminal.actions = vec![
        PartAction::new("close-session", ActionKind::Close, ActionTarget::Process(4242))
            .confirmed(),
    ];
    terminal.cpu += 6.3 * busy;
    terminal.memory = Some(18 * MB + 218 * MB);
    let mut git = part(
        &t("tasks.git_programs"),
        "vcs.git",
        PROGRAMS,
        id,
        (0.8, Some(4 * MB)),
        &[("git-status", 0.8)],
        &tr!("tasks.programs_detail", count = 1),
    );
    git.processes = vec![process(5120, "git", 0.6, 9 * MB, 220.0 * 1024.0)];
    let mut remote = part(
        &tr!("tasks.remote", name = name.as_str()),
        "remote.projects",
        SERVICES,
        id,
        (1.1, Some(22 * MB)),
        &[("remote-sftp", 1.1)],
        &tr!("tasks.remote_detail", transfers = 12, folders = 48, pending = 3),
    );
    remote.net = Some(24.0 * 1024.0 * busy);
    remote.activities = vec![
        Activity {
            id: 0x7a51 + u128::from(busy > 1.0),
            project: id,
            kind: ActivityKind::Upload,
            subject: "src/Shop.php".into(),
            detail: String::new(),
            text: None,
            unit: ActivityUnit::Bytes,
            done: 1_258_291,
            total: Some(3_145_728),
            started: now - Duration::from_secs(18),
            ended: None,
            stoppable: false,
        },
        Activity {
            id: 0x7a60 + u128::from(busy > 1.0),
            project: id,
            kind: ActivityKind::Sync,
            subject: "public/".into(),
            detail: String::new(),
            text: None,
            unit: ActivityUnit::Files,
            done: 42,
            total: Some(42),
            started: now - Duration::from_secs(95),
            ended: Some((now - Duration::from_secs(61), Outcome::Done)),
            stoppable: false,
        },
    ];
    // The upload can be stopped (its owner says so).
    remote.activities[0].stoppable = true;
    remote.actions = vec![PartAction::new(
        workbench_model::resources::STOP_OPERATION,
        ActionKind::Stop,
        ActionTarget::Activity(remote.activities[0].id),
    )];
    vec![index, terminal, git, remote]
}

/// The look shown: this project's (`shop`) parts and the app's — or, `all`, both projects'.
pub fn frame(all: bool, now: Instant) -> Frame {
    let [shop, blog] = projects();
    let mut lines = vec![
        part(
            &t("tasks.ui"),
            "app",
            APP,
            None,
            (3.1, Some(96 * MB)),
            &[("main", 3.1)],
            "The window, its events, the layout",
        ),
        part(
            &t("tasks.files"),
            "app",
            APP,
            None,
            (0.0, Some(12 * MB)),
            &[],
            &tr!("tasks.files_detail", count = 14, undo = i18n::bytes(2 * MB + MB / 10)),
        ),
        {
            let mut render = part(
                &t("tasks.render"),
                "app",
                APP,
                None,
                (1.6, Some(8 * MB)),
                &[("render", 1.6)],
                &t("tasks.render_detail"),
            );
            render.gpu = Some(48 * MB);
            render
        },
        part(
            &t("tasks.syntax"),
            "app",
            APP,
            None,
            (0.2, Some(22 * MB)),
            &[("syntax", 0.2)],
            "6 grammars loaded",
        ),
    ];
    lines.extend(project_parts(&shop, now, 1.0));
    if all {
        lines.extend(project_parts(&blog, now, 0.4));
    }
    let mut formatters = part(
        &t("tasks.quality"),
        "quality",
        PROGRAMS,
        None,
        (2.4, Some(3 * MB)),
        &[],
        &tr!("tasks.programs_detail", count = 1),
    );
    formatters.processes = vec![process(5200, "php-cs-fixer", 2.4, 41 * MB, 0.0)];
    formatters.memory = Some(44 * MB);
    formatters.actions = vec![
        PartAction::new("stop-program", ActionKind::Stop, ActionTarget::Process(5200)).confirmed(),
    ];
    lines.push(formatters);
    lines.push({
        let mut sessions = part(
            &t("tasks.database"),
            "database.explorer",
            SERVICES,
            None,
            (0.0, Some(6 * MB)),
            &[],
            &tr!("tasks.database_detail", open = 1, sources = 2),
        );
        sessions.actions =
            vec![PartAction::new("close-idle", ActionKind::Disconnect, ActionTarget::Part)];
        sessions
    });
    for (name, owner, memory) in [
        ("PHP", "lang.php", 38),
        ("CSS", "lang.css", 9),
        ("Git", "vcs.git", 5),
        ("Task Manager", "tasks.manager", 2),
    ] {
        let mut module = part(
            name,
            owner,
            MODULES,
            None,
            (0.0, Some(memory * MB)),
            &[],
            &t("tasks.module_loaded"),
        );
        module.actions = vec![PartAction::new("release", ActionKind::Release, ActionTarget::Part)];
        lines.push(module);
    }
    lines.push(part(
        &t("tasks.other_threads"),
        APP,
        APP,
        None,
        (0.9, None),
        &[("notify-rs fsevents loop", 0.5), ("com.apple.NSEventThread", 0.4)],
        "notify-rs fsevents loop · com.apple.NSEventThread",
    ));
    let known: u64 = lines.iter().filter_map(|l| l.memory).sum();
    let memory = known + 38 * MB;
    lines.push(part(&t("tasks.unattributed"), APP, APP, None, (0.0, Some(38 * MB)), &[], ""));
    let cpu = lines.iter().map(|l| l.cpu).sum();
    let threads = lines.iter().map(|l| l.threads).sum::<usize>() + 96;
    Frame {
        cpu,
        memory,
        gpu: Some(48 * MB),
        net: lines.iter().filter_map(|l| l.net).sum(),
        disk: Some(1.4 * MB as f32),
        threads,
        projects: if all { vec![shop, blog] } else { vec![shop] },
        all,
        lines,
    }
}

/// The look with the project's language servers too (`servers`): vtsls ready — its Node and
/// the tsserver it started, its figures — and one that crashed too often.
pub fn frame_with_servers(now: Instant) -> Frame {
    let mut frame = frame(false, now);
    let [shop, _] = projects();
    let id = Some(shop.id);
    let languages = "JavaScript, TypeScript";
    let ready = t("lsp.state_ready");
    let detail = tr!("lsp.detail", languages = languages, state = ready.as_str(), count = 3);
    let mut vtsls = part("vtsls", "lsp", LANGUAGE_SERVERS, id, (1.8, None), &[], &detail);
    vtsls.processes = vec![
        process(6100, "node", 0.4, 74 * MB, 0.0),
        process(6101, "node (tsserver)", 1.4, 286 * MB, 12.0 * 1024.0),
    ];
    vtsls.memory = Some(360 * MB);
    let latency =
        tr!("lsp.latency_value", p50 = tr!("lsp.ms", count = 18), p95 = tr!("lsp.ms", count = 96));
    vtsls.facts = vec![
        (t("lsp.fact_languages"), languages.into()),
        (t("lsp.fact_state"), ready),
        (t("lsp.fact_pid"), "6100".into()),
        (t("lsp.fact_uptime"), tr!("lsp.minutes", minutes = 12, seconds = 4)),
        (t("lsp.fact_documents"), "3".into()),
        (t("lsp.fact_requests"), "42".into()),
        (t("lsp.fact_latency"), latency),
        (t("lsp.fact_last_error"), t("lsp.none")),
        (t("lsp.fact_version"), "0.3.0".into()),
    ];
    vtsls.actions = vec![
        PartAction::new("restart", ActionKind::Restart, ActionTarget::Part),
        PartAction::new("stop", ActionKind::Stop, ActionTarget::Part),
        PartAction::new("log", ActionKind::ShowLog, ActionTarget::Part),
        PartAction::new("settings", ActionKind::Settings, ActionTarget::Part),
    ];
    let crashed_state = t("lsp.state_crashed");
    let detail = tr!("lsp.detail", languages = "Lua", state = crashed_state.as_str(), count = 1);
    let mut crashed = part("my-server", "lsp", LANGUAGE_SERVERS, id, (0.0, Some(0)), &[], &detail);
    crashed.actions = vec![PartAction::new("restart", ActionKind::Restart, ActionTarget::Part)];
    let at = frame.lines.iter().position(|l| l.group == MODULES).unwrap_or(frame.lines.len());
    frame.lines.insert(at, crashed);
    frame.lines.insert(at, vtsls);
    frame.cpu += 1.8;
    frame.memory += 360 * MB;
    frame
}

/// A fixed wave (0..1) at look `i` of `n`: a slow swell and a few spikes.
fn wave(i: usize, phase: usize) -> f32 {
    let t = (i + phase) as f32;
    let swell = 0.5 + 0.35 * (t / 9.0).sin() + 0.1 * (t / 2.3).cos();
    let spike = if (i + phase).is_multiple_of(17) { 0.6 } else { 0.0 };
    (swell + spike).clamp(0.0, 1.4)
}

/// A history of `looks` looks of `frame` (its values swelling and falling, the same every
/// time), the last one `now`.
pub fn history(frame: &Frame, looks: usize, now: Instant) -> Arc<Mutex<History>> {
    let mut history = History::new(Duration::from_secs(60), Duration::from_secs(1));
    let start = now - Duration::from_secs(looks as u64);
    for i in 0..looks {
        let mut look = frame.clone();
        let k = wave(i, 0);
        look.cpu = frame.cpu * k;
        // Memory creeps up, then settles.
        look.memory = (frame.memory as f32 * (0.92 + 0.08 * (i as f32 / looks as f32))) as u64;
        look.net = frame.net * wave(i, 5);
        look.disk = frame.disk.map(|d| d * wave(i, 11));
        for (j, line) in look.lines.iter_mut().enumerate() {
            let k = wave(i, j * 3);
            line.cpu *= k;
            line.net = line.net.map(|n| n * k);
            line.disk = line.disk.map(|d| d * wave(i, j + 7));
            line.memory = line.memory.map(|m| (m as f32 * (0.95 + 0.05 * k)) as u64);
        }
        history.record(&look, start + Duration::from_secs(i as u64 + 1));
    }
    Arc::new(Mutex::new(history))
}
