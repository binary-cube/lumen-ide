//! The statement's summary under the profile's bar: a pill per figure it has — planning,
//! execution, the runner's whole time, the rows, the buffers found in memory and read, the
//! temporary files, JIT, the workers launched of those planned, the server's history of the
//! same query (`pg_stat_statements`: its mean in so many calls), SQLite's counters — each
//! with its exact value in its tooltip; the pills without a value aren't there. On its right,
//! the findings by severity (● 1 critical ▲ 2 warnings ⓘ 3), which filter the list.

use database::profile::derive::ProfileReport;
use database::profile::node::OpKind;
use database::profile::rules::Finding;
use ui_components::Level;

use crate::profile::figures::{self, level};

/// A figure of the summary.
#[derive(Debug, Clone, PartialEq)]
pub struct Pill {
    /// Stable (tests, the layout's order).
    pub id: &'static str,
    pub icon: &'static str,
    pub label: String,
    pub value: String,
    pub tip: String,
    /// It's a concern (drawn in the warning's color): a spill, workers short.
    pub warn: bool,
}

impl Pill {
    fn new(id: &'static str, icon: &'static str, value: String, tip: String) -> Self {
        let label = i18n::t(&format!("profile.view.pill.{id}"));
        Self { id, icon, label, value, tip, warn: false }
    }

    fn warned(mut self, warn: bool) -> Self {
        self.warn = warn;
        self
    }
}

/// The pills of `report`, in their order.
pub fn pills(report: &ProfileReport) -> Vec<Pill> {
    let p = &report.profile;
    let s = &p.statement;
    let mut out = Vec::new();
    let exact = |ms: f64| figures::ms_exact(ms);
    if let Some(ms) = s.planning_ms {
        out.push(Pill::new("planning", "clock", figures::ms(ms), exact(ms)));
    }
    if let Some(ms) = s.execution_ms.or(report.totals.time_ms) {
        out.push(Pill::new("execution", "play", figures::ms(ms), exact(ms)));
    }
    if let Some(ms) = s.wall_ms {
        out.push(Pill::new(
            "wall",
            "clock",
            figures::ms(ms),
            i18n::tr!("profile.view.pill.wall_tip", time = exact(ms).as_str()),
        ));
    }
    if let Some(ms) = s.fetch_ms {
        out.push(Pill::new("fetch", "download", figures::ms(ms), exact(ms)));
    }
    let rows = s.rows_returned.map(|r| r as f64).or_else(|| {
        let roots: Vec<f64> =
            p.roots.iter().filter_map(|r| report.derived(*r).total_rows).collect();
        (!roots.is_empty()).then(|| roots.iter().sum())
    });
    if let Some(rows) = rows {
        out.push(Pill::new("rows", "rows", figures::count(rows), figures::count(rows)));
    }
    if let Some(affected) = s.rows_affected {
        out.push(Pill::new(
            "affected",
            "file-pen",
            figures::count(affected as f64),
            figures::count(affected as f64),
        ));
    }
    if let Some(io) = report.totals.io.or(s.io_total)
        && (io.shared_hit.is_some() || io.shared_read.is_some())
    {
        let (hit, read) = (io.shared_hit.unwrap_or(0), io.shared_read.unwrap_or(0));
        let value = i18n::tr!(
            "profile.view.pill.buffers_value",
            hit = figures::bytes(io.bytes(hit)).as_str(),
            read = figures::bytes(io.bytes(read)).as_str()
        );
        let tip = i18n::tr!(
            "profile.view.buffers_tip",
            hit = figures::count(hit as f64).as_str(),
            hit_bytes = figures::bytes(io.bytes(hit)).as_str(),
            read = figures::count(read as f64).as_str(),
            read_bytes = figures::bytes(io.bytes(read)).as_str()
        );
        out.push(Pill::new("buffers", "memory", value, tip));
        let temp = io.temp_read.unwrap_or(0) + io.temp_written.unwrap_or(0);
        if temp > 0 {
            out.push(
                Pill::new(
                    "temp",
                    "hard-drive",
                    figures::bytes(io.bytes(temp)),
                    i18n::tr!(
                        "profile.view.pill.temp_tip",
                        blocks = figures::count(temp as f64).as_str()
                    ),
                )
                .warned(true),
            );
        }
    }
    if let Some(jit) = &s.jit
        && let Some(ms) = jit.total_ms
    {
        let tip = i18n::tr!(
            "profile.view.pill.jit_tip",
            functions = jit.functions.unwrap_or(0) as i64,
            time = exact(ms).as_str()
        );
        out.push(Pill::new("jit", "cpu", figures::ms(ms), tip));
    }
    // The workers of every gather, together.
    let (mut planned, mut launched, mut any) = (0u32, 0u32, false);
    for node in p.nodes.iter().filter(|n| matches!(n.op, OpKind::Gather | OpKind::GatherMerge)) {
        if let Some(par) = &node.parallel {
            planned += par.planned.unwrap_or(0);
            launched += par.launched.or(par.planned).unwrap_or(0);
            any = true;
        }
    }
    if any {
        out.push(
            Pill::new(
                "workers",
                "cpu",
                format!("{launched}/{planned}"),
                i18n::tr!(
                    "profile.view.workers_tip",
                    launched = i64::from(launched),
                    planned = i64::from(planned)
                ),
            )
            .warned(launched < planned),
        );
    }
    // The server's history of the same query (`pg_stat_statements`).
    if let Some(h) = &s.history
        && let (Some(calls), Some(mean)) = (h.calls, h.mean_ms)
    {
        let value = i18n::tr!(
            "profile.view.pill.history_value",
            mean = figures::ms(mean).as_str(),
            calls = figures::count(calls as f64).as_str()
        );
        let mut tip = vec![i18n::tr!(
            "profile.view.pill.history_tip",
            mean = exact(mean).as_str(),
            calls = figures::count(calls as f64).as_str()
        )];
        if let (Some(min), Some(max)) = (h.min_ms, h.max_ms) {
            tip.push(i18n::tr!(
                "profile.view.pill.history_range",
                min = exact(min).as_str(),
                max = exact(max).as_str()
            ));
        }
        if let Some(sd) = h.stddev_ms {
            tip.push(i18n::tr!("profile.view.pill.history_stddev", time = exact(sd).as_str()));
        }
        if let Some(rows) = h.rows {
            tip.push(i18n::tr!(
                "profile.view.pill.history_rows",
                rows = figures::count(rows as f64).as_str()
            ));
        }
        if let Some(ratio) = h.hit_ratio {
            tip.push(i18n::tr!(
                "profile.view.pill.history_hits",
                share = figures::share_exact(ratio).as_str()
            ));
        }
        out.push(Pill::new("history", "server", value, tip.join("\n")));
    }
    if let Some(c) = &s.counters {
        for (id, value) in [
            ("vm_steps", c.vm_steps),
            ("fullscan", c.fullscan_steps),
            ("sorts", c.sorts),
            ("autoindex", c.autoindex),
        ] {
            if let Some(v) = value {
                let pill = Pill::new(id, "cpu", figures::count(v as f64), figures::count(v as f64));
                out.push(pill.warned(matches!(id, "autoindex" | "fullscan") && v > 0));
            }
        }
    }
    out
}

/// How many findings of each severity (the hot spot apart), the worst first.
pub fn counts(findings: &[&Finding]) -> Vec<(Level, usize)> {
    let mut out = Vec::new();
    for l in [Level::Critical, Level::Warning, Level::Info] {
        let n = findings.iter().filter(|f| f.is_problem() && level(f.severity) == l).count();
        if n > 0 {
            out.push((l, n));
        }
    }
    out
}

/// A severity's count in words (`1 critical`, `2 warnings`).
pub fn count_text(l: Level, n: usize) -> String {
    let key = match l {
        Level::Critical => "profile.view.count_critical",
        Level::Warning => "profile.view.count_warning",
        Level::Info => "profile.view.count_info",
    };
    i18n::tr!(key, count = n as i64)
}
