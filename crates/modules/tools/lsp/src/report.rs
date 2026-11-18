//! The servers in the Task Manager: one part per server running for a project, in the
//! "Language servers" group — its process (its children's memory and CPU counting too:
//! tsserver under vtsls), its state, its documents, its requests per minute and their
//! latency, how long it's run, its last error — with Restart, Stop, Show Log and Settings.

use std::time::{Duration, Instant};

use i18n::{t, tr};
use lsp_client::status::{Snapshot, State};
use workbench_model::{
    ActOutcome, ActionKind, ActionTarget, PartAction, PartRef, ResourceReporter, ResourceUse,
};

use crate::projects::{self, ProjectServers, Running};

/// The module's id (the parts' owner).
pub const OWNER: &str = "lsp";

/// The Task Manager's view of the servers.
#[derive(Debug, Default)]
pub struct Reporter;

/// A language's name as shown.
pub fn language_name(id: &str) -> String {
    const KNOWN: [(&str, &str); 16] = [
        ("javascript", "JavaScript"),
        ("typescript", "TypeScript"),
        ("tsx", "TSX"),
        ("php", "PHP"),
        ("html", "HTML"),
        ("css", "CSS"),
        ("scss", "SCSS"),
        ("json", "JSON"),
        ("yaml", "YAML"),
        ("toml", "TOML"),
        ("c", "C"),
        ("cpp", "C++"),
        ("lua", "Lua"),
        ("zig", "Zig"),
        ("bash", "Bash"),
        ("dockerfile", "Dockerfile"),
    ];
    KNOWN.iter().find(|(k, _)| *k == id).map_or_else(|| id.to_owned(), |(_, n)| (*n).to_owned())
}

/// A server's state as said.
pub fn state_text(snapshot: &Snapshot) -> String {
    match &snapshot.state {
        State::Starting => t("lsp.state_starting"),
        State::Ready | State::Idle if snapshot.progress.is_some() => {
            let (title, percent) = snapshot.progress.clone().unwrap_or_default();
            match percent {
                Some(p) => tr!("lsp.state_working_percent", title = title, percent = p),
                None => tr!("lsp.state_working", title = title),
            }
        }
        State::Ready => t("lsp.state_ready"),
        State::Idle => t("lsp.state_idle"),
        State::Restarting => t("lsp.state_restarting"),
        State::Stopped { crashed: true } => t("lsp.state_crashed"),
        State::Stopped { crashed: false } => t("lsp.state_stopped"),
        State::Failed(_) => t("lsp.state_failed"),
    }
}

/// A duration as said (`850 ms`, `2 min 5 s`, `3 h 2 min`).
pub fn duration_text(d: Duration) -> String {
    let secs = d.as_secs();
    if secs == 0 {
        return tr!("lsp.ms", count = d.as_millis() as i64);
    }
    match secs {
        0..60 => tr!("lsp.seconds", count = secs as i64),
        60..3600 => tr!("lsp.minutes", minutes = (secs / 60) as i64, seconds = (secs % 60) as i64),
        _ => tr!("lsp.hours", hours = (secs / 3600) as i64, minutes = (secs % 3600 / 60) as i64),
    }
}

/// Server `running`'s part's name: its own, with its root when it isn't the project's.
pub fn part_name(project: &ProjectServers, running: &Running) -> String {
    match running.root.strip_prefix(&project.root) {
        Ok(relative) if !relative.as_os_str().is_empty() => {
            format!("{} · {}", running.spec.name, relative.to_string_lossy().replace('\\', "/"))
        }
        _ => running.spec.name.clone(),
    }
}

/// Server `running`'s part, as it is at `now`.
pub fn part(project: &ProjectServers, running: &Running, now: Instant) -> ResourceUse {
    let snapshot = running.client.status().snapshot(now);
    let languages: Vec<String> = if running.languages.is_empty() {
        running.spec.languages.iter().map(|l| language_name(l)).collect()
    } else {
        running.languages.iter().map(|l| language_name(l)).collect()
    };
    let languages = languages.join(", ");
    let state = state_text(&snapshot);
    let mut part = ResourceUse::new(part_name(project, running), OWNER);
    part.group = workbench_model::resources::LANGUAGE_SERVERS;
    part.project = Some(project.id);
    part.processes = snapshot.pid.into_iter().collect();
    part.detail = tr!(
        "lsp.detail",
        languages = languages.as_str(),
        state = state.as_str(),
        count = snapshot.documents as i64
    );
    let latency = match (snapshot.p50, snapshot.p95) {
        (Some(p50), Some(p95)) => {
            tr!("lsp.latency_value", p50 = duration_text(p50), p95 = duration_text(p95))
        }
        _ => t("lsp.none"),
    };
    let mut facts = vec![
        (t("lsp.fact_languages"), languages),
        (t("lsp.fact_state"), state),
        (t("lsp.fact_pid"), snapshot.pid.map_or_else(|| t("lsp.none"), |p| p.to_string())),
        (t("lsp.fact_uptime"), snapshot.uptime.map_or_else(|| t("lsp.none"), duration_text)),
        (t("lsp.fact_documents"), snapshot.documents.to_string()),
        (t("lsp.fact_requests"), snapshot.per_minute.to_string()),
        (t("lsp.fact_latency"), latency),
        (t("lsp.fact_last_error"), snapshot.last_error.clone().unwrap_or_else(|| t("lsp.none"))),
    ];
    if let Some(version) = &running.version {
        facts.push((t("lsp.fact_version"), version.clone()));
    }
    if snapshot.crashes > 0 {
        facts.push((t("lsp.fact_crashes"), snapshot.crashes.to_string()));
    }
    part.facts = facts;
    part.actions = vec![
        PartAction::new("restart", ActionKind::Restart, ActionTarget::Part),
        PartAction::new("stop", ActionKind::Stop, ActionTarget::Part),
        PartAction::new("log", ActionKind::ShowLog, ActionTarget::Part),
        PartAction::new("settings", ActionKind::Settings, ActionTarget::Part),
    ];
    part
}

/// Every project's servers' parts, and — for every project — the part of the servers the user
/// installs once one was downloaded: their downloads, as operations (Stop while they run).
pub fn parts(now: Instant) -> Vec<ResourceUse> {
    let mut out = Vec::new();
    for project in projects::all() {
        project.servers(|servers| out.extend(servers.iter().map(|r| part(&project, r, now))));
    }
    let mut installs = ResourceUse::new(t("lsp.installs_part"), OWNER);
    installs.group = workbench_model::resources::LANGUAGE_SERVERS;
    installs.threads = vec!["lsp-install"];
    crate::install::ACTIVITIES.report(&mut installs);
    if !installs.activities.is_empty() {
        installs.detail = t("lsp.installs_detail");
        out.push(installs);
    }
    out
}

/// The command opening the servers' log.
pub const SHOW_LOG: &str = "lsp.show_log";

impl ResourceReporter for Reporter {
    fn report(&self) -> Vec<ResourceUse> {
        parts(Instant::now())
    }

    fn act(&self, part: &PartRef, action: &str, target: ActionTarget) -> ActOutcome {
        if part.project.is_none() && action == workbench_model::resources::STOP_OPERATION {
            return crate::install::ACTIVITIES.act(target);
        }
        let Some(project) = part.project.and_then(projects::project) else {
            return ActOutcome::Unsupported;
        };
        let id = project.servers(|servers| {
            servers.iter().find(|r| part_name(&project, r) == part.name).map(|r| r.id)
        });
        let Some(id) = id else { return ActOutcome::Unsupported };
        match action {
            "restart" if project.restart(id) => ActOutcome::Done,
            "stop" if project.stop(id) => ActOutcome::Done,
            "log" => {
                project.choose_log(id);
                ActOutcome::Command(SHOW_LOG.to_owned())
            }
            "settings" => ActOutcome::Command("app.settings".to_owned()),
            _ => ActOutcome::Unsupported,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/report.rs"]
mod tests;
