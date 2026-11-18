//! The Task Manager's texts: the numbers as shown (`3.4%`, `412 MB`, `12 KB/s`) and what an
//! operation says — written once per look by the view model, never while drawing.

use std::time::{Duration, Instant};

use i18n::{t, tr};
use workbench_model::{Activity, ActivityKind, ActivityUnit, Outcome};

use super::view_model::Column;
use crate::collector::{Line, ProcessRow};

/// `3.4%`.
pub fn percent(cpu: f32) -> String {
    i18n::decimal(format!("{cpu:.1}%"))
}

/// `12 KB/s`.
pub fn rate(bytes: f32) -> String {
    match bytes {
        b if b < 1.0 => "0 B/s".to_owned(),
        b if b < 1024.0 => format!("{b:.0} B/s"),
        b if b < 1024.0 * 1024.0 => format!("{:.0} KB/s", b / 1024.0),
        b => i18n::decimal(format!("{:.1} MB/s", b / (1024.0 * 1024.0))),
    }
}

/// `3 s`, `2 min 5 s`, `<1 s`.
pub fn duration(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0 => "<1 s".to_owned(),
        s if s < 60 => format!("{s} s"),
        s => format!("{} min {} s", s / 60, s % 60),
    }
}

/// A part's numbers as its row shows them (`None`: not known, a dash).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartTexts {
    /// Only for a part with threads counted (else nothing is measured).
    pub cpu: Option<String>,
    pub memory: Option<String>,
    pub gpu: Option<String>,
    pub net: Option<String>,
    pub disk: Option<String>,
    pub threads: Option<String>,
}

impl PartTexts {
    pub fn of(line: &Line) -> Self {
        Self {
            // Measured when it has threads of its own or programs it started (a language
            // server: its processes' CPU, summed).
            cpu: (line.threads > 0 || !line.processes.is_empty()).then(|| percent(line.cpu)),
            memory: line.memory.map(i18n::bytes),
            gpu: line.gpu.map(i18n::bytes),
            net: line.net.map(rate),
            disk: line.disk.map(rate),
            threads: (line.threads > 0).then(|| line.threads.to_string()),
        }
    }

    /// Column `column`'s text (`None`: not known; the name has none here).
    pub fn get(&self, column: Column) -> Option<&str> {
        match column {
            Column::Name => None,
            Column::Cpu => self.cpu.as_deref(),
            Column::Memory => self.memory.as_deref(),
            Column::Gpu => self.gpu.as_deref(),
            Column::Net => self.net.as_deref(),
            Column::Disk => self.disk.as_deref(),
            Column::Threads => self.threads.as_deref(),
        }
    }

    /// Its numbers other than the one sorted by, as a two-line list's second line shows
    /// them (`412 MB · ↑↓ 12 KB/s`).
    pub fn others(&self, sorted: Column) -> String {
        let mut out = String::new();
        let numbers = [
            (Column::Cpu, ""),
            (Column::Memory, ""),
            (Column::Net, "↓↑ "),
            (Column::Disk, "◫ "),
            (Column::Gpu, "GPU "),
        ];
        for (column, mark) in numbers {
            let Some(text) = self.get(column).filter(|_| column != sorted) else { continue };
            if !out.is_empty() {
                out.push_str(" · ");
            }
            out.push_str(mark);
            out.push_str(text);
        }
        out
    }
}

/// How long a program has been running: `42 s`, `12 min`, `5 h 12 min` (hours past a day
/// too: `52 h 3 min`).
pub fn running(d: Duration) -> String {
    let secs = d.as_secs();
    let (hours, minutes) = (secs / 3600, secs % 3600 / 60);
    match secs {
        s if s < 60 => format!("{s} s"),
        _ if hours == 0 => format!("{minutes} min"),
        _ => format!("{hours} h {minutes} min"),
    }
}

/// A program's numbers as its row shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessTexts {
    /// What it is, whatever it runs: the name it was started by and its program's when they
    /// differ, the project it works for (a terminal's), its id, how long it has run, how many
    /// programs its tree holds — `claude (2.1.325) · shop · 71540 · 5 h 12 min · 4 processes`.
    pub name: String,
    pub cpu: String,
    pub memory: String,
    pub disk: String,
}

impl ProcessTexts {
    pub fn of(row: &ProcessRow) -> Self {
        let program = match (&row.invoked, &row.name) {
            (Some(invoked), Some(name)) if !invoked.eq_ignore_ascii_case(name) => {
                format!("{invoked} ({name}) · ")
            }
            (_, Some(name)) => format!("{name} · "),
            (Some(invoked), None) => format!("{invoked} · "),
            (None, None) => String::new(),
        };
        let project = row.project.as_deref().map(|p| format!("{p} · ")).unwrap_or_default();
        let mut name = format!("{program}{project}{}", row.pid);
        if !row.running.is_zero() {
            name.push_str(" · ");
            name.push_str(&running(row.running));
        }
        if row.processes > 1 {
            name.push_str(" · ");
            name.push_str(&tr!("tasks.process_tree", count = row.processes));
        }
        Self { name, cpu: percent(row.cpu), memory: i18n::bytes(row.memory), disk: rate(row.disk) }
    }
}

/// What an operation is, in the user's language.
pub fn kind_label(kind: ActivityKind) -> String {
    t(&format!("tasks.op_{}", kind.key()))
}

pub fn kind_icon(kind: ActivityKind) -> &'static str {
    kind.icon()
}

/// How much an operation did (of how much): `1.2 MB / 3.0 MB`, `12 / 30 files`.
pub fn progress(a: &Activity) -> String {
    match (a.unit, a.total) {
        (ActivityUnit::Bytes, Some(total)) => {
            format!("{} / {}", i18n::bytes(a.done), i18n::bytes(total))
        }
        (ActivityUnit::Bytes, None) => i18n::bytes(a.done),
        (ActivityUnit::Files, Some(total)) => {
            tr!("tasks.op_files_of", done = a.done, count = total)
        }
        (ActivityUnit::Files, None) => tr!("tasks.op_files", count = a.done),
        (ActivityUnit::Rows, Some(total)) => {
            tr!("tasks.op_rows_of", done = a.done, count = total)
        }
        // Nothing counted yet (a query before its rows): nothing said.
        (ActivityUnit::Rows, None) if a.done == 0 && a.running() => String::new(),
        (ActivityUnit::Rows, None) => tr!("tasks.op_rows", count = a.done),
    }
}

/// Where an operation is: running — how far, how fast, how long it has left (from its
/// pace so far); ended — how it ended, how long it took.
pub fn status(a: &Activity, now: Instant) -> String {
    match &a.ended {
        None => {
            let elapsed = now.duration_since(a.started).as_secs_f32();
            let speed = (elapsed > 0.5).then(|| a.done as f32 / elapsed).filter(|s| *s > 0.0);
            let mut parts = Vec::new();
            if let Some(total) = a.total.filter(|t| *t > 0) {
                parts.push(format!("{:.0}%", a.done as f32 / total as f32 * 100.0));
            }
            if let (Some(speed), ActivityUnit::Bytes) = (speed, a.unit) {
                parts.push(rate(speed));
            }
            if let (Some(speed), Some(total)) = (speed, a.total) {
                let left = total.saturating_sub(a.done) as f32 / speed;
                parts.push(tr!("tasks.op_left", time = duration(Duration::from_secs_f32(left))));
            }
            if parts.is_empty() {
                parts.push(duration(now.duration_since(a.started)));
            }
            parts.join(" · ")
        }
        Some((at, Outcome::Done)) => {
            tr!("tasks.op_done", time = duration(at.duration_since(a.started)))
        }
        Some((_, Outcome::Failed(_))) => t("tasks.op_failed"),
        Some((_, Outcome::Stopped)) => t("tasks.op_stopped"),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/model/format.rs"]
mod tests;
