//! A profile written out: as text, the way `EXPLAIN` writes a plan ([`to_text`]: what
//! "Copy" gives, the drivers' golden files), and as our own versioned JSON ([`to_json`]:
//! written by hand — the domain has no serde —, its fields in a stable order, the same names
//! the drivers' protocol uses for `profile`).

use std::fmt::Write as _;

use crate::profile::extra::{Extra, ExtraValue};
use crate::profile::node::{
    CacheStats, ChildRole, Condition, Estimates, HashStats, Io, Memory, NodeId, ObjectKind,
    Parallel, ProfileNode, Wal, WorkerStats,
};
use crate::profile::statement::{EngineCounters, Jit, StatementProfile, StatementStats};
use crate::profile::{Profile, ProfileDepth};

/// A number as the exports write it: whole numbers without decimals, others with at most
/// three (`0.04`, `12.345`), never `NaN`.
pub fn number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".to_owned() } else { text.to_owned() }
}

/// Fixed decimals, as `EXPLAIN` writes costs and times.
fn fixed(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}")
}

// ─── Text ───

/// `profile` as text: a header line, its tree as `EXPLAIN` writes one (each node's words,
/// object, estimates and what it did, then its conditions, I/O, memory, workers and extras
/// under it), then the statement's figures and the notes.
pub fn to_text(profile: &Profile) -> String {
    let mut out = String::new();
    let depth = match profile.depth {
        ProfileDepth::Estimated => "estimated",
        ProfileDepth::Actual { timing: true } => "actual",
        ProfileDepth::Actual { timing: false } => "actual, no timing",
    };
    let mut engine = profile.engine.name.clone();
    if let Some(version) = &profile.engine.version {
        engine.push(' ');
        engine.push_str(version);
    }
    let engine = engine.trim();
    if engine.is_empty() {
        let _ = writeln!(out, "-- {depth} · format {}", profile.format);
    } else {
        let _ = writeln!(out, "-- {engine} · {depth} · format {}", profile.format);
    }
    for (id, level) in profile.walk() {
        node_text(&mut out, profile, id, level);
    }
    statement_text(&mut out, &profile.statement);
    for note in &profile.notes {
        match note.argument() {
            Some(n) => {
                let _ = writeln!(out, "Note: {} ({n})", note.id());
            }
            None => {
                let _ = writeln!(out, "Note: {}", note.id());
            }
        }
    }
    out
}

/// A node's head line's text: its words, `using index`, `on object` (what its words don't
/// say already).
pub fn head(node: &ProfileNode) -> String {
    let mut text = node.name.clone();
    if let Some(join) = node.join
        && join != crate::profile::node::JoinKind::Inner
        && !text.contains(join.words())
    {
        text = format!("{} {text}", join.words());
    }
    let table = node.object.as_ref().filter(|o| o.kind != ObjectKind::Index);
    if let Some(index) = &node.index
        && !has_word(&text, index)
    {
        text.push_str(&format!(" using {index}"));
    }
    if let Some(object) = table {
        let label = object.label();
        if !has_word(&text, &object.name) {
            text.push_str(&format!(" on {label}"));
        }
    } else if let Some(object) = &node.object
        && node.index.is_none()
        && !has_word(&text, &object.name)
    {
        text.push_str(&format!(" on {}", object.label()));
    }
    text
}

/// `word` is one of `text`'s words (not a part of one: `accounts` isn't in `accounts_pkey`).
pub fn has_word(text: &str, word: &str) -> bool {
    !word.is_empty()
        && text
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == '$'))
            .any(|w| w == word || w.rsplit('.').next() == Some(word))
}

fn node_text(out: &mut String, profile: &Profile, id: NodeId, level: usize) {
    let node = profile.node(id);
    let pad = if level == 0 { String::new() } else { " ".repeat(level * 6 - 4) };
    let inner = if level == 0 { "  ".to_owned() } else { " ".repeat(level * 6 + 2) };
    if let Some(name) = &node.subplan_name {
        let _ = writeln!(
            out,
            "{}{name}",
            if level == 0 { String::new() } else { " ".repeat(level * 6 - 4) }
        );
    }
    let arrow = if level == 0 { "" } else { "->  " };
    let mut line = format!("{pad}{arrow}{}", head(node));
    let e = &node.estimates;
    if e != &Estimates::default() {
        let mut parts = Vec::new();
        match (e.startup_cost, e.total_cost) {
            (Some(a), Some(b)) => parts.push(format!("cost={}..{}", fixed(a, 2), fixed(b, 2))),
            (None, Some(b)) => parts.push(format!("cost={}", fixed(b, 2))),
            _ => {}
        }
        if let Some(rows) = e.rows {
            parts.push(format!("rows={}", number(rows)));
        }
        if let Some(width) = e.width {
            parts.push(format!("width={}", number(width)));
        }
        if !parts.is_empty() {
            line.push_str(&format!("  ({})", parts.join(" ")));
        }
    }
    let a = &node.actuals;
    if a.never_executed {
        line.push_str(" (never executed)");
    } else if a.rows.is_some() || a.total_ms.is_some() || a.loops.is_some() {
        let mut parts = Vec::new();
        match (a.startup_ms, a.total_ms) {
            (Some(s), Some(t)) => parts.push(format!("time={}..{}", fixed(s, 3), fixed(t, 3))),
            (None, Some(t)) => parts.push(format!("time={}", fixed(t, 3))),
            _ => {}
        }
        if let Some(rows) = a.rows {
            parts.push(format!("rows={}", number(rows)));
        }
        if let Some(loops) = a.loops {
            parts.push(format!("loops={}", number(loops)));
        }
        line.push_str(&format!(" (actual {})", parts.join(" ")));
    }
    let _ = writeln!(out, "{line}");
    let mut sub = |text: String| {
        let _ = writeln!(out, "{inner}{text}");
    };
    // Outer and inner say nothing the tree doesn't.
    if let Some(role) = node.role.filter(|r| !matches!(r, ChildRole::Outer | ChildRole::Inner)) {
        sub(format!("Role: {}", role.id()));
    }
    if !node.output.is_empty() {
        sub(format!("Output: {}", node.output.join(", ")));
    }
    for Condition { kind, text } in &node.conditions {
        let label = kind.label();
        if label.is_empty() { sub(text.clone()) } else { sub(format!("{label}: {text}")) }
    }
    let removed = [
        ("Rows Removed by Filter", a.rows_removed_by_filter),
        ("Rows Removed by Join Filter", a.rows_removed_by_join_filter),
        ("Rows Removed by Index Recheck", a.rows_removed_by_recheck),
        ("Heap Fetches", a.heap_fetches),
        ("Index Searches", a.index_searches),
        ("Exact Heap Blocks", a.exact_blocks),
        ("Lossy Heap Blocks", a.lossy_blocks),
    ];
    for (label, value) in removed {
        if let Some(v) = value {
            sub(format!("{label}: {}", number(v)));
        }
    }
    if let Some(p) = &node.parallel {
        parallel_text(&mut sub, p);
    }
    if let Some(m) = &node.memory {
        memory_text(&mut sub, m);
    }
    if let Some(s) = &node.spill {
        let mut parts = vec![s.kind.id().to_owned()];
        if let Some(b) = s.disk_bytes {
            parts.push(format!("disk={b} bytes"));
        }
        if let Some(b) = s.batches {
            parts.push(format!("batches={b}"));
        }
        sub(format!("Spill: {}", parts.join(" ")));
    }
    if let Some(io) = &node.io {
        io_text(&mut sub, "", io);
    }
    if let Some(w) = &node.wal {
        sub(format!("WAL: {}", wal_text(w)));
    }
    let f = node.flags;
    if f.disabled {
        sub("Disabled: true".to_owned());
    }
    if f.async_capable {
        sub("Async Capable: true".to_owned());
    }
    if f.partial {
        sub("Partial: true".to_owned());
    }
    for extra in &node.extra {
        extra_text(&mut sub, extra);
    }
}

fn parallel_text(sub: &mut dyn FnMut(String), p: &Parallel) {
    if let Some(n) = p.planned {
        sub(format!("Workers Planned: {n}"));
    }
    if let Some(n) = p.launched {
        sub(format!("Workers Launched: {n}"));
    }
    if p.aware {
        sub("Parallel Aware: true".to_owned());
    }
    for WorkerStats { number: n, rows, loops, startup_ms, total_ms, io, extra } in &p.per_worker {
        let mut parts = Vec::new();
        match (startup_ms, total_ms) {
            (Some(s), Some(t)) => parts.push(format!("time={}..{}", fixed(*s, 3), fixed(*t, 3))),
            (None, Some(t)) => parts.push(format!("time={}", fixed(*t, 3))),
            _ => {}
        }
        if let Some(r) = rows {
            parts.push(format!("rows={}", number(*r)));
        }
        if let Some(l) = loops {
            parts.push(format!("loops={}", number(*l)));
        }
        sub(format!("Worker {n}: {}", parts.join(" ")));
        if let Some(io) = io {
            io_text(sub, &format!("Worker {n} "), io);
        }
        for e in extra {
            sub(format!("Worker {n} {}: {}", e.key, e.value.text()));
        }
    }
}

fn memory_text(sub: &mut dyn FnMut(String), m: &Memory) {
    if let Some(method) = &m.sort_method {
        let mut text = format!("Sort Method: {method}");
        if let Some((bytes, place)) = m.sort_space {
            text.push_str(&format!("  {}: {bytes} bytes", place.id()));
        }
        sub(text);
    } else if let Some((bytes, place)) = m.sort_space {
        sub(format!("Sort Space: {bytes} bytes ({})", place.id()));
    }
    if let Some(HashStats { buckets, original_buckets, batches, original_batches }) = m.hash {
        let mut parts = Vec::new();
        if let Some(b) = buckets {
            parts.push(match original_buckets {
                Some(o) if o != b => format!("Buckets: {b} (originally {o})"),
                _ => format!("Buckets: {b}"),
            });
        }
        if let Some(b) = batches {
            parts.push(match original_batches {
                Some(o) if o != b => format!("Batches: {b} (originally {o})"),
                _ => format!("Batches: {b}"),
            });
        }
        if !parts.is_empty() {
            sub(parts.join("  "));
        }
    }
    if let Some(CacheStats { hits, misses, evictions, overflows }) = m.cache {
        let parts: Vec<String> = [
            ("Hits", hits),
            ("Misses", misses),
            ("Evictions", evictions),
            ("Overflows", overflows),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k}: {v}")))
        .collect();
        sub(parts.join("  "));
    }
    if let Some(storage) = m.storage {
        sub(format!("Storage: {}", storage.id()));
    }
    if let Some(peak) = m.peak_bytes {
        sub(format!("Peak Memory: {peak} bytes"));
    }
}

fn io_text(sub: &mut dyn FnMut(String), prefix: &str, io: &Io) {
    let group = |name: &str, list: [(&str, Option<u64>); 4]| -> Option<String> {
        let parts: Vec<String> =
            list.iter().filter_map(|(k, v)| v.map(|v| format!("{k}={v}"))).collect();
        (!parts.is_empty()).then(|| format!("{name} {}", parts.join(" ")))
    };
    let blocks: Vec<String> = [
        group(
            "shared",
            [
                ("hit", io.shared_hit),
                ("read", io.shared_read),
                ("dirtied", io.shared_dirtied),
                ("written", io.shared_written),
            ],
        ),
        group(
            "local",
            [
                ("hit", io.local_hit),
                ("read", io.local_read),
                ("dirtied", io.local_dirtied),
                ("written", io.local_written),
            ],
        ),
        group(
            "temp",
            [("read", io.temp_read), ("written", io.temp_written), ("", None), ("", None)],
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !blocks.is_empty() {
        sub(format!("{prefix}Buffers: {}", blocks.join(", ")));
    }
    let times: Vec<String> = [
        ("shared read", io.read_ms),
        ("shared write", io.write_ms),
        ("local read", io.local_read_ms),
        ("local write", io.local_write_ms),
        ("temp read", io.temp_read_ms),
        ("temp write", io.temp_write_ms),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.map(|v| format!("{k}={}", fixed(v, 3))))
    .collect();
    if !times.is_empty() {
        sub(format!("{prefix}I/O Timings: {}", times.join(" ")));
    }
}

fn wal_text(w: &Wal) -> String {
    [
        ("records", w.records),
        ("fpi", w.full_page_images),
        ("bytes", w.bytes),
        ("buffers full", w.buffers_full),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.map(|v| format!("{k}={v}")))
    .collect::<Vec<_>>()
    .join(" ")
}

fn extra_text(sub: &mut dyn FnMut(String), extra: &Extra) {
    let key = match &extra.group {
        Some(group) => format!("{group} › {}", extra.key),
        None => extra.key.clone(),
    };
    sub(format!("{key}: {}", extra.value.text()));
}

fn statement_text(out: &mut String, s: &StatementProfile) {
    let mut line = |text: String| {
        let _ = writeln!(out, "{text}");
    };
    if let Some(io) = &s.planning_io {
        io_text(&mut line, "Planning ", io);
    }
    if let Some(m) = &s.planning_memory {
        let parts: Vec<String> = [("used", m.used_bytes), ("allocated", m.allocated_bytes)]
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| format!("{k}={v} bytes")))
            .collect();
        line(format!("Planning Memory: {}", parts.join(" ")));
    }
    let times = [
        ("Planning Time", s.planning_ms),
        ("Execution Time", s.execution_ms),
        ("Wall Time", s.wall_ms),
        ("First Row", s.first_row_ms),
        ("Fetch Time", s.fetch_ms),
    ];
    for (label, value) in times {
        if let Some(v) = value {
            line(format!("{label}: {} ms", fixed(v, 3)));
        }
    }
    if let Some(r) = s.rows_returned {
        line(format!("Rows Returned: {r}"));
    }
    if let Some(r) = s.rows_affected {
        line(format!("Rows Affected: {r}"));
    }
    if let Some(ser) = &s.serialize {
        let mut parts = Vec::new();
        if let Some(ms) = ser.ms {
            parts.push(format!("time={} ms", fixed(ms, 3)));
        }
        if let Some(b) = ser.bytes {
            parts.push(format!("output={b} bytes"));
        }
        if let Some(f) = &ser.format {
            parts.push(format!("format={f}"));
        }
        line(format!("Serialization: {}", parts.join(" ")));
    }
    if let Some(Jit {
        functions,
        generation_ms,
        inlining_ms,
        optimization_ms,
        emission_ms,
        deform_ms,
        total_ms,
    }) = s.jit
    {
        let mut parts = Vec::new();
        if let Some(f) = functions {
            parts.push(format!("functions={f}"));
        }
        for (k, v) in [
            ("generation", generation_ms),
            ("deform", deform_ms),
            ("inlining", inlining_ms),
            ("optimization", optimization_ms),
            ("emission", emission_ms),
            ("total", total_ms),
        ] {
            if let Some(v) = v {
                parts.push(format!("{k}={}", fixed(v, 3)));
            }
        }
        line(format!("JIT: {}", parts.join(" ")));
    }
    for t in &s.triggers {
        let mut text = format!("Trigger {}", t.name);
        if let Some(r) = &t.relation {
            text.push_str(&format!(" on {r}"));
        }
        if let Some(ms) = t.ms {
            text.push_str(&format!(": time={}", fixed(ms, 3)));
        }
        if let Some(c) = t.calls {
            text.push_str(&format!(" calls={c}"));
        }
        line(text);
    }
    if !s.settings.is_empty() {
        let parts: Vec<String> =
            s.settings.iter().map(|x| format!("{} = '{}'", x.name, x.value)).collect();
        line(format!("Settings: {}", parts.join(", ")));
    }
    if let Some(q) = s.query_id {
        line(format!("Query Identifier: {q}"));
    }
    if let Some(c) = &s.counters {
        line(format!("Counters: {}", counters_text(c)));
    }
    if let Some(h) = &s.history {
        line(format!("History: {}", history_text(h)));
    }
    for w in &s.waits {
        let mut text = format!("Wait {}", w.event);
        if let Some(ms) = w.ms {
            text.push_str(&format!(" time={}", fixed(ms, 3)));
        }
        if let Some(c) = w.count {
            text.push_str(&format!(" count={c}"));
        }
        line(text);
    }
    for extra in &s.extra {
        extra_text(&mut line, extra);
    }
}

/// The engine's counters on a line (`fullscan_steps=49999 sorts=1`).
pub fn counters_text(c: &EngineCounters) -> String {
    counters(c)
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k}={v}")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn counters(c: &EngineCounters) -> [(&'static str, Option<u64>); 13] {
    [
        ("fullscan_steps", c.fullscan_steps),
        ("sorts", c.sorts),
        ("autoindex", c.autoindex),
        ("vm_steps", c.vm_steps),
        ("reprepare", c.reprepare),
        ("runs", c.runs),
        ("filter_hits", c.filter_hits),
        ("filter_misses", c.filter_misses),
        ("mem_used", c.mem_used),
        ("rows_examined", c.rows_examined),
        ("select_scan", c.select_scan),
        ("sort_merge_passes", c.sort_merge_passes),
        ("tmp_disk_tables", c.tmp_disk_tables),
    ]
}

fn history_text(h: &StatementStats) -> String {
    let mut parts = Vec::new();
    if let Some(c) = h.calls {
        parts.push(format!("calls={c}"));
    }
    for (k, v) in [
        ("mean", h.mean_ms),
        ("stddev", h.stddev_ms),
        ("min", h.min_ms),
        ("max", h.max_ms),
        ("hit_ratio", h.hit_ratio),
    ] {
        if let Some(v) = v {
            parts.push(format!("{k}={}", number(v)));
        }
    }
    for (k, v) in [("rows", h.rows), ("temp_blocks", h.temp_blocks), ("wal_bytes", h.wal_bytes)] {
        if let Some(v) = v {
            parts.push(format!("{k}={v}"));
        }
    }
    parts.join(" ")
}

// ─── JSON ───

/// A JSON value, written in a stable order (what [`to_json`] builds).
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
}

impl Json {
    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Num(n) => {
                if n.is_finite() {
                    if n.fract() == 0.0 && n.abs() < 1e15 {
                        let _ = write!(out, "{}", *n as i64);
                    } else {
                        let _ = write!(out, "{n}");
                    }
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => string(out, s),
            Json::Arr(items) if items.is_empty() => out.push_str("[]"),
            Json::Arr(items) => {
                // Short lists of plain values on one line.
                if items.len() <= 16
                    && items.iter().all(|i| matches!(i, Json::Int(_) | Json::Num(_)))
                {
                    out.push('[');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        item.write(out, indent);
                    }
                    out.push(']');
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&"  ".repeat(indent + 1));
                    item.write(out, indent + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(indent));
                out.push(']');
            }
            Json::Obj(fields) if fields.is_empty() => out.push_str("{}"),
            Json::Obj(fields) => {
                out.push_str("{\n");
                for (i, (key, value)) in fields.iter().enumerate() {
                    out.push_str(&"  ".repeat(indent + 1));
                    string(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    if i + 1 < fields.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(indent));
                out.push('}');
            }
        }
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }
}

fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// An object's fields, those without a value left out.
#[derive(Default)]
struct Obj(Vec<(&'static str, Json)>);

impl Obj {
    fn put(mut self, key: &'static str, value: Json) -> Self {
        self.0.push((key, value));
        self
    }
    fn f(self, key: &'static str, value: Option<f64>) -> Self {
        match value {
            Some(v) => self.put(key, Json::Num(v)),
            None => self,
        }
    }
    fn u(self, key: &'static str, value: Option<u64>) -> Self {
        match value {
            Some(v) => self.put(key, Json::Int(v.min(i64::MAX as u64) as i64)),
            None => self,
        }
    }
    fn s(self, key: &'static str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.put(key, Json::Str(v.to_owned())),
            None => self,
        }
    }
    fn b(self, key: &'static str, value: bool) -> Self {
        if value { self.put(key, Json::Bool(true)) } else { self }
    }
    fn o(self, key: &'static str, value: Option<Json>) -> Self {
        match value {
            Some(Json::Obj(f)) if f.is_empty() => self,
            Some(v) => self.put(key, v),
            None => self,
        }
    }
    fn list(self, key: &'static str, items: Vec<Json>) -> Self {
        if items.is_empty() { self } else { self.put(key, Json::Arr(items)) }
    }
    fn done(self) -> Json {
        Json::Obj(self.0)
    }
}

/// The depth's id in the JSON: `estimated`, `actual`, `actual_no_timing`.
pub fn depth_id(depth: ProfileDepth) -> &'static str {
    match depth {
        ProfileDepth::Estimated => "estimated",
        ProfileDepth::Actual { timing: true } => "actual",
        ProfileDepth::Actual { timing: false } => "actual_no_timing",
    }
}

pub fn depth_from_id(id: &str) -> Option<ProfileDepth> {
    Some(match id {
        "estimated" => ProfileDepth::Estimated,
        "actual" => ProfileDepth::Actual { timing: true },
        "actual_no_timing" => ProfileDepth::Actual { timing: false },
        _ => return None,
    })
}

/// `profile` as our JSON (its format first).
pub fn to_json(profile: &Profile) -> String {
    let mut text = profile_json(profile).text();
    text.push('\n');
    text
}

pub fn profile_json(p: &Profile) -> Json {
    let engine = Obj::default()
        .put("driver", Json::Str(p.engine.driver.clone()))
        .put("name", Json::Str(p.engine.name.clone()))
        .s("version", p.engine.version.as_deref())
        .u("version_num", p.engine.version_num.map(u64::from))
        .done();
    let collected = Json::Obj(
        p.collected
            .fields()
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(k, _)| (k, Json::Bool(true)))
            .collect(),
    );
    let notes = p
        .notes
        .iter()
        .map(|n| {
            Obj::default()
                .put("id", Json::Str(n.id().to_owned()))
                .u("argument", n.argument())
                .done()
        })
        .collect();
    let raw = p.raw.as_ref().map(|r| {
        Obj::default()
            .put("format", Json::Str(r.format.id().to_owned()))
            .put("text", Json::Str(r.text.to_string()))
            .done()
    });
    Obj::default()
        .put("format", Json::Int(i64::from(p.format)))
        .put("id", Json::Str(p.id.to_string()))
        .put("engine", engine)
        .put("depth", Json::Str(depth_id(p.depth).to_owned()))
        .put("statement", statement_json(&p.statement))
        .put("nodes", Json::Arr(p.nodes.iter().map(node_json).collect()))
        .put("roots", Json::Arr(p.roots.iter().map(|r| Json::Int(i64::from(r.0))).collect()))
        .put("collected", collected)
        .list("notes", notes)
        .o("raw", raw)
        .done()
}

fn node_json(n: &ProfileNode) -> Json {
    let object = n.object.as_ref().map(|o| {
        Obj::default()
            .put("kind", Json::Str(o.kind.id().to_owned()))
            .s("schema", o.schema.as_deref())
            .put("name", Json::Str(o.name.clone()))
            .s("alias", o.alias.as_deref())
            .done()
    });
    let conditions = n
        .conditions
        .iter()
        .map(|c| {
            let label = match &c.kind {
                crate::profile::node::CondKind::Other(l) if !l.is_empty() => Some(l.as_str()),
                _ => None,
            };
            Obj::default()
                .put("kind", Json::Str(c.kind.id().to_owned()))
                .s("label", label)
                .put("text", Json::Str(c.text.clone()))
                .done()
        })
        .collect();
    let e = &n.estimates;
    let estimates = Obj::default()
        .f("startup_cost", e.startup_cost)
        .f("total_cost", e.total_cost)
        .f("rows", e.rows)
        .f("width", e.width)
        .done();
    let a = &n.actuals;
    let actuals = Obj::default()
        .f("rows", a.rows)
        .f("loops", a.loops)
        .f("startup_ms", a.startup_ms)
        .f("total_ms", a.total_ms)
        .b("never_executed", a.never_executed)
        .f("rows_removed_by_filter", a.rows_removed_by_filter)
        .f("rows_removed_by_join_filter", a.rows_removed_by_join_filter)
        .f("rows_removed_by_recheck", a.rows_removed_by_recheck)
        .f("heap_fetches", a.heap_fetches)
        .f("index_searches", a.index_searches)
        .f("exact_blocks", a.exact_blocks)
        .f("lossy_blocks", a.lossy_blocks)
        .done();
    let memory = n.memory.as_ref().map(memory_json);
    let spill = n.spill.map(|s| {
        Obj::default()
            .put("kind", Json::Str(s.kind.id().to_owned()))
            .u("disk_bytes", s.disk_bytes)
            .u("batches", s.batches)
            .done()
    });
    let flags = Obj::default()
        .b("disabled", n.flags.disabled)
        .b("async_capable", n.flags.async_capable)
        .b("partial", n.flags.partial)
        .done();
    Obj::default()
        .put("parent", n.parent.map_or(Json::Null, |p| Json::Int(i64::from(p.0))))
        .put("children", Json::Arr(n.children.iter().map(|c| Json::Int(i64::from(c.0))).collect()))
        .s("role", n.role.map(|r| r.id()))
        .s("subplan_name", n.subplan_name.as_deref())
        .put("op", Json::Str(n.op.id().to_owned()))
        .put("name", Json::Str(n.name.clone()))
        .s("join", n.join.map(|j| j.id()))
        .o("object", object)
        .s("index", n.index.as_deref())
        .s("scan_direction", n.scan_direction.map(|d| d.id()))
        .list("conditions", conditions)
        .list("output", n.output.iter().map(|o| Json::Str(o.clone())).collect())
        .o("estimates", Some(estimates))
        .o("actuals", Some(actuals))
        .o("io", n.io.as_ref().map(io_json))
        .o("memory", memory)
        .o("spill", spill)
        .o("parallel", n.parallel.as_ref().map(parallel_json))
        .o("wal", n.wal.map(|w| wal_json(&w)))
        .o("flags", Some(flags))
        .list("extra", n.extra.iter().map(extra_json).collect())
        .done()
}

pub fn io_json(io: &Io) -> Json {
    Obj::default()
        .u("shared_hit", io.shared_hit)
        .u("shared_read", io.shared_read)
        .u("shared_dirtied", io.shared_dirtied)
        .u("shared_written", io.shared_written)
        .u("local_hit", io.local_hit)
        .u("local_read", io.local_read)
        .u("local_dirtied", io.local_dirtied)
        .u("local_written", io.local_written)
        .u("temp_read", io.temp_read)
        .u("temp_written", io.temp_written)
        .f("read_ms", io.read_ms)
        .f("write_ms", io.write_ms)
        .f("local_read_ms", io.local_read_ms)
        .f("local_write_ms", io.local_write_ms)
        .f("temp_read_ms", io.temp_read_ms)
        .f("temp_write_ms", io.temp_write_ms)
        .u("block_size", io.block_size.map(u64::from))
        .done()
}

fn memory_json(m: &Memory) -> Json {
    let hash = m.hash.map(|h| {
        Obj::default()
            .u("buckets", h.buckets)
            .u("original_buckets", h.original_buckets)
            .u("batches", h.batches)
            .u("original_batches", h.original_batches)
            .done()
    });
    let cache = m.cache.map(|c| {
        Obj::default()
            .u("hits", c.hits)
            .u("misses", c.misses)
            .u("evictions", c.evictions)
            .u("overflows", c.overflows)
            .done()
    });
    let sort_space = m.sort_space.map(|(bytes, place)| {
        Obj::default()
            .u("bytes", Some(bytes))
            .put("storage", Json::Str(place.id().to_owned()))
            .done()
    });
    Obj::default()
        .u("peak_bytes", m.peak_bytes)
        .s("sort_method", m.sort_method.as_deref())
        .o("sort_space", sort_space)
        .o("hash", hash)
        .o("cache", cache)
        .s("storage", m.storage.map(|s| s.id()))
        .done()
}

fn parallel_json(p: &Parallel) -> Json {
    let workers = p
        .per_worker
        .iter()
        .map(|w| {
            Obj::default()
                .put("number", Json::Int(i64::from(w.number)))
                .f("rows", w.rows)
                .f("loops", w.loops)
                .f("startup_ms", w.startup_ms)
                .f("total_ms", w.total_ms)
                .o("io", w.io.as_ref().map(io_json))
                .list("extra", w.extra.iter().map(extra_json).collect())
                .done()
        })
        .collect();
    Obj::default()
        .u("planned", p.planned.map(u64::from))
        .u("launched", p.launched.map(u64::from))
        .b("aware", p.aware)
        .list("per_worker", workers)
        .done()
}

fn wal_json(w: &Wal) -> Json {
    Obj::default()
        .u("records", w.records)
        .u("full_page_images", w.full_page_images)
        .u("bytes", w.bytes)
        .u("buffers_full", w.buffers_full)
        .done()
}

pub fn extra_json(e: &Extra) -> Json {
    Obj::default()
        .put("key", Json::Str(e.key.clone()))
        .s("label", e.label.as_deref())
        .s("group", e.group.as_deref())
        .put("value", value_json(&e.value))
        .done()
}

fn value_json(v: &ExtraValue) -> Json {
    let typed = |value: Json| {
        Obj::default().put("type", Json::Str(v.type_id().to_owned())).put("value", value)
    };
    match v {
        ExtraValue::Bool(b) => typed(Json::Bool(*b)).done(),
        ExtraValue::Int(n) => typed(Json::Int(*n)).done(),
        ExtraValue::Float(n) => typed(Json::Num(*n)).done(),
        ExtraValue::Text(t) => typed(Json::Str(t.clone())).done(),
        ExtraValue::Quantity(q) => {
            typed(Json::Num(q.value)).put("unit", Json::Str(q.unit.id().to_owned())).done()
        }
        ExtraValue::List(items) => typed(Json::Arr(items.iter().map(value_json).collect())).done(),
        ExtraValue::Group(items) => typed(Json::Arr(items.iter().map(extra_json).collect())).done(),
    }
}

fn statement_json(s: &StatementProfile) -> Json {
    let jit = s.jit.map(|j| {
        Obj::default()
            .u("functions", j.functions)
            .f("generation_ms", j.generation_ms)
            .f("inlining_ms", j.inlining_ms)
            .f("optimization_ms", j.optimization_ms)
            .f("emission_ms", j.emission_ms)
            .f("deform_ms", j.deform_ms)
            .f("total_ms", j.total_ms)
            .done()
    });
    let triggers = s
        .triggers
        .iter()
        .map(|t| {
            Obj::default()
                .put("name", Json::Str(t.name.clone()))
                .s("relation", t.relation.as_deref())
                .u("calls", t.calls)
                .f("ms", t.ms)
                .done()
        })
        .collect();
    let settings = s
        .settings
        .iter()
        .map(|x| {
            Obj::default()
                .put("name", Json::Str(x.name.clone()))
                .put("value", Json::Str(x.value.clone()))
                .done()
        })
        .collect();
    let counters = s.counters.map(|c| {
        let mut o = Obj::default();
        for (k, v) in counters(&c) {
            o = o.u(k, v);
        }
        o.done()
    });
    let history = s.history.map(|h| {
        Obj::default()
            .u("calls", h.calls)
            .f("mean_ms", h.mean_ms)
            .f("stddev_ms", h.stddev_ms)
            .f("min_ms", h.min_ms)
            .f("max_ms", h.max_ms)
            .u("rows", h.rows)
            .f("hit_ratio", h.hit_ratio)
            .u("temp_blocks", h.temp_blocks)
            .u("wal_bytes", h.wal_bytes)
            .done()
    });
    let waits = s
        .waits
        .iter()
        .map(|w| {
            Obj::default()
                .put("event", Json::Str(w.event.clone()))
                .f("ms", w.ms)
                .u("count", w.count)
                .done()
        })
        .collect();
    let planning_memory = s.planning_memory.map(|m| {
        Obj::default().u("used_bytes", m.used_bytes).u("allocated_bytes", m.allocated_bytes).done()
    });
    let serialize = s.serialize.as_ref().map(|x| {
        Obj::default().f("ms", x.ms).u("bytes", x.bytes).s("format", x.format.as_deref()).done()
    });
    Obj::default()
        .f("planning_ms", s.planning_ms)
        .f("execution_ms", s.execution_ms)
        .f("wall_ms", s.wall_ms)
        .f("first_row_ms", s.first_row_ms)
        .f("fetch_ms", s.fetch_ms)
        .u("rows_returned", s.rows_returned)
        .u("rows_affected", s.rows_affected)
        .o("planning_io", s.planning_io.as_ref().map(io_json))
        .o("io_total", s.io_total.as_ref().map(io_json))
        .o("planning_memory", planning_memory)
        .o("serialize", serialize)
        .o("jit", jit)
        .list("triggers", triggers)
        .list("settings", settings)
        .put_opt_i64("query_id", s.query_id)
        .o("counters", counters)
        .o("history", history)
        .list("waits", waits)
        .list("extra", s.extra.iter().map(extra_json).collect())
        .done()
}

impl Obj {
    fn put_opt_i64(self, key: &'static str, value: Option<i64>) -> Self {
        match value {
            Some(v) => self.put(key, Json::Int(v)),
            None => self,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/profile/export.rs"]
mod tests;
