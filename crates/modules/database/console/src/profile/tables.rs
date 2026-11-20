//! A profile's tables (plan 39h6, "Tables" in the switcher): a row per table the statement
//! touches ([`database::profile::tables::usage`]) — its aliases, how it was read (a scan, an
//! index, an index alone, a bitmap, with the indexes' names), the rows read and kept, its
//! part of the time, its problems (the worst one's sign in the gutter, the row tinted) — and
//! what the server knows of it, read once, in the background, when the view first opens
//! without it ([`crate::runner::Runner::load_table_stats`]): rows live and dead, the table's
//! and its indexes' sizes, the last `ANALYZE` / `VACUUM`, its sequential and index scans;
//! SQLite's statistics (`sqlite_stat1`) present or not. Its indexes are its rows' children,
//! each "used here" or "never used" (no scan since the statistics were reset).
//!
//! A table chosen chooses its first step in the other views (and so its finding); its
//! right-click menu opens the table, or its structure.

use database::profile::derive::ProfileReport;
use database::profile::rules::{Facts, Finding, TableFacts, same_table};
use database::profile::sql_map::SqlMap;
use database::profile::tables::{Access, TableUsage, usage};
use database::profile::{ExtraValue, IndexStats, TableStats, Unit};
use database::sql::Flavor;
use ui_components::ds::Tone;
use ui_components::{Ink, Level, Span, SpanStyle, TableCell, TableColumn, TableRow};

use crate::profile::analysis;
use crate::profile::figures::{self, level};

/// A column of the tables' view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TCol {
    Table,
    Access,
    Rows,
    Share,
    Live,
    Dead,
    Size,
    IndexSize,
    Analyzed,
    Vacuumed,
    SeqScans,
    IdxScans,
}

impl TCol {
    /// Which go first when the room is short (the table and how it was read never).
    pub const PRIORITY: [TCol; 12] = [
        TCol::Table,
        TCol::Access,
        TCol::Share,
        TCol::Rows,
        TCol::Dead,
        TCol::Analyzed,
        TCol::Size,
        TCol::Live,
        TCol::IndexSize,
        TCol::Vacuumed,
        TCol::IdxScans,
        TCol::SeqScans,
    ];

    pub const ORDER: [TCol; 12] = [
        TCol::Table,
        TCol::Access,
        TCol::Rows,
        TCol::Share,
        TCol::Live,
        TCol::Dead,
        TCol::Size,
        TCol::IndexSize,
        TCol::Analyzed,
        TCol::Vacuumed,
        TCol::SeqScans,
        TCol::IdxScans,
    ];

    fn key(self) -> &'static str {
        match self {
            TCol::Table => "table",
            TCol::Access => "access",
            TCol::Rows => "rows",
            TCol::Share => "share",
            TCol::Live => "live",
            TCol::Dead => "dead",
            TCol::Size => "size",
            TCol::IndexSize => "index_size",
            TCol::Analyzed => "analyzed",
            TCol::Vacuumed => "vacuumed",
            TCol::SeqScans => "seq_scans",
            TCol::IdxScans => "idx_scans",
        }
    }

    pub fn width(self) -> f32 {
        match self {
            TCol::Table => 0.0,
            TCol::Access => 140.0,
            TCol::Rows => 152.0,
            TCol::Share => 118.0,
            TCol::Live => 84.0,
            TCol::Dead => 100.0,
            TCol::Size => 72.0,
            TCol::IndexSize => 72.0,
            TCol::Analyzed => 136.0,
            TCol::Vacuumed => 136.0,
            TCol::SeqScans => 76.0,
            TCol::IdxScans => 84.0,
        }
    }

    fn column(self) -> TableColumn {
        let key = self.key();
        let c = TableColumn::new(i18n::t(&format!("profile.tables.col.{key}")), self.width())
            .with_tip(i18n::t(&format!("profile.tables.col.{key}_tip")));
        match self {
            TCol::Table => c.with_min(190.0),
            TCol::Access | TCol::Analyzed | TCol::Vacuumed => c.with_min(80.0),
            _ => c.right().with_min(48.0),
        }
    }
}

/// What a row of the view is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TablesRow {
    /// A table (an index into the usage).
    Table(usize),
    /// One of its indexes (the table's usage, the index's name).
    Index(usize, String),
}

/// The view's table made of a profile: its columns, rows, what each row is.
#[derive(Debug, Clone, Default)]
pub struct TablesTable {
    pub cols: Vec<TCol>,
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub kinds: Vec<TablesRow>,
    pub usage: Vec<TableUsage>,
    /// Some table has the server's statistics.
    pub has_stats: bool,
}

/// The least the table's column asks for (logical pixels): its name, aliases, an index's.
pub const TABLE_MIN: f32 = 250.0;

/// Of `cols`, those that fit in `width` logical pixels with the table's column at least
/// [`TABLE_MIN`]: the least important left out first ([`TCol::PRIORITY`]); what's left out
/// is still in the table's tooltip.
pub fn fitting(cols: &[TCol], width: f32) -> Vec<TCol> {
    let mut kept = cols.to_vec();
    for drop in TCol::PRIORITY.iter().rev() {
        let sum: f32 = kept.iter().map(|c| c.width()).sum();
        if sum + TABLE_MIN <= width || kept.len() <= 2 {
            break;
        }
        if !matches!(drop, TCol::Table | TCol::Access) {
            kept.retain(|c| c != drop);
        }
    }
    kept
}

impl TablesTable {
    /// Its columns and rows with only `keep`'s columns (in its order).
    pub fn narrowed(&self, keep: &[TCol]) -> (Vec<TableColumn>, Vec<TableRow>) {
        let at: Vec<usize> =
            (0..self.cols.len()).filter(|&i| keep.contains(&self.cols[i])).collect();
        let columns = at.iter().map(|&i| self.columns[i].clone()).collect();
        let rows = self
            .rows
            .iter()
            .map(|r| {
                let mut r = r.clone();
                r.cells = at.iter().filter_map(|&i| r.cells.get(i).cloned()).collect();
                r
            })
            .collect();
        (columns, rows)
    }
}

/// What's known of `table` (its facts).
fn facts_of<'a>(facts: Option<&'a Facts>, u: &TableUsage) -> Option<&'a TableFacts> {
    facts?.tables.iter().find(|t| same_table(&t.table, &u.table))
}

/// `report`'s tables (`statement` its text, `flavor` its dialect; `findings` those shown;
/// `facts` what's known of the tables).
pub fn build(
    report: &ProfileReport,
    statement: &str,
    flavor: Flavor,
    findings: &[&Finding],
    facts: Option<&Facts>,
) -> TablesTable {
    let map = SqlMap::new(statement, flavor);
    let usage = usage(report, &map);
    let stats: Vec<Option<&TableStats>> =
        usage.iter().map(|u| facts_of(facts, u).and_then(|f| f.stats.as_ref())).collect();
    let has_stats = stats.iter().any(Option::is_some);
    let any = |f: &dyn Fn(&TableStats) -> bool| stats.iter().flatten().any(|s| f(s));
    let cols: Vec<TCol> = TCol::ORDER
        .into_iter()
        .filter(|c| match c {
            TCol::Table | TCol::Access => true,
            TCol::Rows => usage.iter().any(|u| u.rows_read.is_some() || u.rows_returned.is_some()),
            TCol::Share => usage.iter().any(|u| u.share.is_some()),
            TCol::Live => {
                any(&|s| s.live_rows.is_some())
                    || usage.iter().any(|u| facts_of(facts, u).and_then(|f| f.rows).is_some())
            }
            TCol::Dead => any(&|s| s.dead_rows.is_some()),
            TCol::Size => any(&|s| s.table_bytes.is_some() || s.total_bytes.is_some()),
            TCol::IndexSize => {
                any(&|s| s.index_bytes.is_some() || s.indexes.iter().any(|i| i.bytes.is_some()))
            }
            TCol::Analyzed => any(&|s| {
                s.last_analyze.is_some()
                    || s.last_autoanalyze.is_some()
                    || s.stats_present.is_some()
                    || s.modified_since_analyze.is_some()
            }),
            TCol::Vacuumed => any(&|s| s.last_vacuum.is_some()),
            TCol::SeqScans => any(&|s| s.seq_scans.is_some()),
            TCol::IdxScans => {
                any(&|s| s.idx_scans.is_some() || s.indexes.iter().any(|i| i.scans.is_some()))
            }
        })
        .collect();
    let columns = cols.iter().map(|c| c.column()).collect();
    let mut rows = Vec::new();
    let mut kinds = Vec::new();
    for (ui, u) in usage.iter().enumerate() {
        let tf = facts_of(facts, u);
        let s = stats[ui];
        let worst = u
            .findings
            .iter()
            .filter_map(|i| report.findings.get(*i))
            .filter(|f| findings.iter().any(|shown| std::ptr::eq(*shown, *f)))
            .max_by(|a, b| a.severity.cmp(&b.severity).then(a.impact.total_cmp(&b.impact)));
        let ink = worst.map(|f| Ink::Severity(level(f.severity)));
        let cells = cols.iter().map(|c| table_cell(report, u, tf, s, *c, findings)).collect();
        let mut row = TableRow::new(None, cells).with_icon("table").tinted(ink);
        if let Some(f) = worst {
            let shown: Vec<&Finding> = u
                .findings
                .iter()
                .filter_map(|i| report.findings.get(*i))
                .filter(|f| findings.iter().any(|x| std::ptr::eq(*x, *f)))
                .collect();
            let tip = shown
                .iter()
                .map(|f| {
                    format!("{} · {}", analysis::severity(f.severity), analysis::texts(f).title)
                })
                .collect::<Vec<_>>()
                .join("\n");
            row = row.with_sign(
                level(f.severity).icon(),
                ink.unwrap_or(Ink::Tone(Tone::Neutral)),
                tip,
            );
        }
        let at = rows.len();
        rows.push(row);
        kinds.push(TablesRow::Table(ui));
        // Its indexes: the server's statistics', else its structure's.
        for index in indexes_of(tf) {
            let cells = cols.iter().map(|c| index_cell(u, &index, *c)).collect();
            let icon = if index.unique { "index-unique" } else { "index" };
            rows.push(TableRow::new(Some(at), cells).with_icon(icon));
            kinds.push(TablesRow::Index(ui, index.name.clone()));
        }
    }
    TablesTable { cols, columns, rows, kinds, usage, has_stats }
}

/// A table's indexes as the view lists them: the statistics' (their sizes and scans), else
/// its structure's.
fn indexes_of(facts: Option<&TableFacts>) -> Vec<IndexStats> {
    let Some(f) = facts else { return Vec::new() };
    if let Some(s) = f.stats.as_ref().filter(|s| !s.indexes.is_empty()) {
        return s.indexes.clone();
    }
    f.indexes()
        .into_iter()
        .map(|i| IndexStats {
            name: i.name,
            columns: i.columns,
            unique: i.unique,
            bytes: None,
            scans: None,
        })
        .collect()
}

/// A time as the server wrote it, shorter: `2026-10-29 03:12` (the zone and seconds in its
/// tooltip).
pub fn short_time(text: &str) -> String {
    let t = text.trim();
    match t.get(..16) {
        Some(head)
            if t.len() > 16
                && head.as_bytes().get(10).is_some_and(|b| *b == b' ' || *b == b'T') =>
        {
            head.replace('T', " ")
        }
        _ => t.to_owned(),
    }
}

fn access_label(a: Access) -> String {
    i18n::t(&format!("profile.tables.access.{}", a.id()))
}

fn table_cell(
    report: &ProfileReport,
    u: &TableUsage,
    facts: Option<&TableFacts>,
    s: Option<&TableStats>,
    c: TCol,
    findings: &[&Finding],
) -> TableCell {
    let empty = TableCell::empty;
    match c {
        TCol::Table => {
            let mut spans = vec![Span::new(u.table.to_string(), SpanStyle::Strong)];
            if !u.aliases.is_empty() {
                spans.push(Span::new(u.aliases.join(", "), SpanStyle::Muted));
            }
            let problems = u
                .findings
                .iter()
                .filter_map(|i| report.findings.get(*i))
                .filter(|f| findings.iter().any(|x| std::ptr::eq(*x, *f)))
                .count();

            let steps: Vec<String> = u
                .scans
                .iter()
                .filter_map(|sc| report.profile.get(sc.node))
                .map(database::profile::export::head)
                .collect();
            let mut tip = format!("{}\n{}", u.table, steps.join("\n"));
            if problems > 0 {
                tip.push_str(&format!(
                    "\n{}",
                    i18n::tr!("profile.tables.problems", count = problems as i64)
                ));
            }
            if let Some(s) = s {
                // Every figure, those whose columns don't fit included.
                for (key, value) in [
                    ("live", s.live_rows.map(|v| figures::count(v as f64))),
                    ("dead", s.dead_rows.map(|v| figures::count(v as f64))),
                    ("size", s.table_bytes.map(figures::bytes)),
                    ("index_size", s.index_bytes.map(figures::bytes)),
                    ("vacuumed", s.last_vacuum.clone()),
                    ("seq_scans", s.seq_scans.map(|v| figures::count(v as f64))),
                    ("idx_scans", s.idx_scans.map(|v| figures::count(v as f64))),
                ] {
                    if let Some(v) = value {
                        let label = i18n::t(&format!("profile.tables.col.{key}"));
                        tip.push_str(&format!("\n{label}: {v}"));
                    }
                }
                for e in &s.extra {
                    if let ExtraValue::Quantity(q) = &e.value
                        && q.unit == Unit::Bytes
                    {
                        let label = i18n::t(&format!("profile.tables.extra.{}", e.key));
                        let label =
                            if label.starts_with("profile.") { e.key.clone() } else { label };
                        tip.push_str(&format!("\n{label}: {}", figures::bytes(q.value as u64)));
                    }
                }
            }
            TableCell::spans(spans).with_tip(tip)
        }
        TCol::Access => {
            let mut spans = Vec::new();
            for a in u.accesses() {
                let ink = match a {
                    Access::Scan | Access::AutoIndex => Ink::Severity(Level::Warning),
                    Access::Write => Ink::Tone(Tone::Neutral),
                    _ => Ink::Tone(Tone::Accent),
                };
                spans.push(Span::new(access_label(a), SpanStyle::Pill(ink)));
            }
            let used = u.indexes_used();
            if !used.is_empty() {
                spans.push(Span::new(used.join(", "), SpanStyle::Muted));
            }
            let tip = u
                .scans
                .iter()
                .map(|sc| {
                    let mut line = access_label(sc.access);
                    if let Some(i) = &sc.index {
                        line = format!("{line} · {i}");
                    }
                    if let Some(a) = &sc.alias {
                        line = format!("{line} ({a})");
                    }
                    line
                })
                .collect::<Vec<_>>()
                .join("\n");
            TableCell::spans(spans).with_tip(tip)
        }
        TCol::Rows => {
            let read = u.rows_read;
            let kept = u.rows_returned;
            match (read, kept) {
                (Some(r), Some(k)) if !u.estimated && (r - k).abs() > 0.5 => {
                    let spans = vec![
                        Span::new(figures::count(r), SpanStyle::Muted),
                        Span::new("→", SpanStyle::Muted),
                        Span::new(figures::count(k), SpanStyle::Normal),
                    ];
                    TableCell::spans(spans).with_tip(i18n::tr!(
                        "profile.tables.rows_tip",
                        read = figures::count(r).as_str(),
                        kept = figures::count(k).as_str()
                    ))
                }
                (Some(r), _) | (None, Some(r)) => {
                    let tip = if u.estimated {
                        i18n::tr!("profile.tables.rows_est_tip", rows = figures::count(r).as_str())
                    } else {
                        figures::count(r)
                    };
                    TableCell::text(figures::count(r)).with_tip(tip)
                }
                _ => empty(),
            }
        }
        TCol::Share => match u.share {
            Some(share) => {
                let label = match u.self_ms.filter(|_| !u.estimated) {
                    Some(ms) => format!("{} · {}", figures::ms(ms), figures::share(share)),
                    None => figures::share(share),
                };
                let tip = if u.estimated {
                    i18n::tr!(
                        "profile.tables.share_cost_tip",
                        share = figures::share_exact(share).as_str()
                    )
                } else {
                    i18n::tr!(
                        "profile.tables.share_tip",
                        share = figures::share_exact(share).as_str()
                    )
                };
                TableCell::bar(share as f32, Ink::heat(share), label).with_tip(tip)
            }
            None => empty(),
        },
        TCol::Live => {
            let live = s.and_then(|s| s.live_rows).or_else(|| facts.and_then(|f| f.rows));
            live.map_or_else(empty, |l| TableCell::text(figures::count(l as f64)))
        }
        TCol::Dead => match (s.and_then(|s| s.dead_rows), s.and_then(|s| s.live_rows)) {
            (Some(dead), live) => {
                let share = live.map(|l| dead as f64 / (l as f64).max(1.0));
                let mut spans = vec![Span::new(figures::count(dead as f64), SpanStyle::Normal)];
                if let Some(sh) = share.filter(|_| dead > 0) {
                    let ink = if sh >= 0.2 {
                        Ink::Severity(Level::Warning)
                    } else {
                        Ink::Tone(Tone::Neutral)
                    };
                    spans.insert(0, Span::new(figures::share(sh), SpanStyle::Pill(ink)));
                }
                TableCell::spans(spans).with_tip(i18n::tr!(
                    "profile.tables.dead_tip",
                    dead = figures::count(dead as f64).as_str(),
                    live = figures::count(live.unwrap_or(0) as f64).as_str()
                ))
            }
            _ => empty(),
        },
        TCol::Size => match s.and_then(|s| s.table_bytes.or(s.total_bytes)) {
            Some(b) => {
                let tip = i18n::tr!(
                    "profile.tables.size_tip",
                    table = figures::bytes(b).as_str(),
                    total = figures::bytes(s.and_then(|s| s.total_bytes).unwrap_or(b)).as_str()
                );
                TableCell::text(figures::bytes(b)).with_tip(tip)
            }
            None => empty(),
        },
        TCol::IndexSize => {
            let b = s.and_then(|s| {
                s.index_bytes.or_else(|| {
                    let sizes: Vec<u64> = s.indexes.iter().filter_map(|i| i.bytes).collect();
                    (!sizes.is_empty()).then(|| sizes.iter().sum())
                })
            });
            b.map_or_else(empty, |b| TableCell::text(figures::bytes(b)))
        }
        TCol::Analyzed => {
            let Some(s) = s else { return empty() };
            let last = match (&s.last_analyze, &s.last_autoanalyze) {
                (Some(a), Some(b)) => Some(if a >= b { a } else { b }),
                (a, b) => a.as_ref().or(b.as_ref()),
            };
            let mut tip = Vec::new();
            if let Some(a) = &s.last_analyze {
                tip.push(i18n::tr!("profile.tables.analyze_tip", time = a.as_str()));
            }
            if let Some(a) = &s.last_autoanalyze {
                tip.push(i18n::tr!("profile.tables.autoanalyze_tip", time = a.as_str()));
            }
            if let Some(m) = s.modified_since_analyze {
                tip.push(i18n::tr!(
                    "profile.tables.modified_tip",
                    rows = figures::count(m as f64).as_str()
                ));
            }
            let cell = match (last, s.stats_present) {
                (Some(t), _) => TableCell::text(short_time(t)),
                (None, Some(true)) => TableCell::spans(vec![Span::new(
                    i18n::t("profile.tables.stats_present"),
                    SpanStyle::Normal,
                )]),
                (None, Some(false)) => TableCell::spans(vec![Span::new(
                    i18n::t("profile.tables.stats_missing"),
                    SpanStyle::Pill(Ink::Severity(Level::Warning)),
                )]),
                (None, None) if s.modified_since_analyze.is_some() || s.live_rows.is_some() => {
                    TableCell::spans(vec![Span::new(
                        i18n::t("profile.tables.never"),
                        SpanStyle::Pill(Ink::Severity(Level::Warning)),
                    )])
                }
                _ => return empty(),
            };
            if tip.is_empty() { cell } else { cell.with_tip(tip.join("\n")) }
        }
        TCol::Vacuumed => match s.and_then(|s| s.last_vacuum.as_ref()) {
            Some(t) => TableCell::text(short_time(t)).with_tip(t.clone()),
            None => empty(),
        },
        TCol::SeqScans => s
            .and_then(|s| s.seq_scans)
            .map_or_else(empty, |n| TableCell::text(figures::count(n as f64))),
        TCol::IdxScans => s
            .and_then(|s| s.idx_scans)
            .map_or_else(empty, |n| TableCell::text(figures::count(n as f64))),
    }
}

fn index_cell(u: &TableUsage, index: &IndexStats, c: TCol) -> TableCell {
    match c {
        TCol::Table => {
            let mut spans = vec![Span::new(index.name.clone(), SpanStyle::Normal)];
            if !index.columns.is_empty() {
                spans.push(Span::new(format!("({})", index.columns.join(", ")), SpanStyle::Muted));
            }
            if index.unique {
                spans.push(Span::new(
                    i18n::t("profile.tables.unique"),
                    SpanStyle::Pill(Ink::Tone(Tone::Neutral)),
                ));
            }
            TableCell::spans(spans).with_tip(format!(
                "{} ({})",
                index.name,
                index.columns.join(", ")
            ))
        }
        TCol::Access => {
            if u.uses_index(&index.name) {
                TableCell::spans(vec![Span::new(
                    i18n::t("profile.tables.used_here"),
                    SpanStyle::Pill(Ink::Tone(Tone::Accent)),
                )])
                .with_tip(i18n::t("profile.tables.used_here_tip"))
            } else if index.scans == Some(0) {
                TableCell::spans(vec![Span::new(
                    i18n::t("profile.tables.never_used"),
                    SpanStyle::Pill(Ink::Severity(Level::Warning)),
                )])
                .with_tip(i18n::t("profile.tables.never_used_tip"))
            } else {
                TableCell::empty()
            }
        }
        TCol::IndexSize => {
            index.bytes.map_or_else(TableCell::empty, |b| TableCell::text(figures::bytes(b)))
        }
        TCol::IdxScans => {
            index.scans.map_or_else(TableCell::empty, |n| TableCell::text(figures::count(n as f64)))
        }
        _ => TableCell::empty(),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/profile/tables.rs"]
mod tests;
