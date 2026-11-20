//! What the profile's details say ([`sections`]): of the step chosen — its operation, its
//! object, its conditions (as SQL, one a line, with their kind), estimated against real (rows,
//! loops, times per loop and in all, cost, width), its I/O, its memory and disk, its workers
//! (a row each), its output (`VERBOSE`), what's specific to its engine (each extra by group,
//! its label, value and unit) and its problems; with none chosen, the statement's own
//! (the engine, planning, execution, JIT, triggers, settings, counters, its extras and the
//! notes). Only the sections with something in them; each value with its raw form in a
//! tooltip, and copied as it's shown.

use database::profile::derive::ProfileReport;
use database::profile::extra::{Extra, ExtraValue, Unit};
use database::profile::node::{Io, NodeId, ObjectKind, Storage};
use database::profile::rules::Finding;
use database::profile::{ProfileDepth, Quantity};

use crate::profile::analysis;
use crate::profile::figures::{self, level};
use crate::profile::tree::op_kind_text;
use ui_components::Level;

/// A line of the details.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A label and its value (`code`: written as SQL), the raw form in its tooltip.
    Value { label: String, value: String, tip: Option<String>, code: bool },
    /// Three columns: what, estimated, real (the head of its table when `head`).
    Compare { label: String, estimated: String, actual: String, head: bool },
    /// A problem of the step: its severity and title.
    Problem { level: Level, title: String, finding: usize },
}

/// A section: its title and its lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub title: String,
    pub lines: Vec<Line>,
}

fn value(label: impl Into<String>, value: impl Into<String>) -> Line {
    Line::Value { label: label.into(), value: value.into(), tip: None, code: false }
}

fn value_tip(label: impl Into<String>, shown: impl Into<String>, tip: String) -> Line {
    Line::Value { label: label.into(), value: shown.into(), tip: Some(tip), code: false }
}

fn t(key: &str) -> String {
    i18n::t(&format!("profile.view.details.{key}"))
}

/// The sections of `node`'s details in `report` (none chosen: the statement's), `findings`
/// the ones shown (their index is a [`Line::Problem`]'s).
pub fn sections(
    report: &ProfileReport,
    node: Option<NodeId>,
    findings: &[&Finding],
) -> Vec<Section> {
    let mut out = Vec::new();
    let mut push = |title: String, lines: Vec<Line>| {
        if !lines.is_empty() {
            out.push(Section { title, lines });
        }
    };
    let Some(id) = node.filter(|id| report.profile.get(*id).is_some()) else {
        for (title, lines) in statement(report) {
            push(title, lines);
        }
        return out;
    };
    let p = &report.profile;
    let n = p.node(id);
    let d = report.derived(id);
    // The operation.
    let mut lines = vec![value(t("kind"), op_kind_text(n.op)), value(t("words"), n.name.clone())];
    if let Some(join) = n.join {
        lines.push(value(t("join"), join.words().to_owned()));
    }
    if let Some(role) = n.role {
        lines.push(value(t("role"), role.id().replace('_', " ")));
    }
    if let Some(name) = &n.subplan_name {
        lines.push(value(t("subplan"), name.clone()));
    }
    if let Some(dir) = n.scan_direction {
        lines.push(value(t("direction"), dir.id().to_owned()));
    }
    if n.actuals.never_executed {
        lines.push(value(t("executed"), i18n::t("profile.view.never_executed")));
    }
    if n.flags.disabled {
        lines.push(value(t("disabled"), i18n::t("profile.view.disabled")));
    }
    push(t("operation"), lines);
    // The object.
    let mut lines = Vec::new();
    if let Some(o) = &n.object {
        if let Some(schema) = &o.schema {
            lines.push(value(t("schema"), schema.clone()));
        }
        let key = match o.kind {
            ObjectKind::Table => "table",
            ObjectKind::Index => "index",
            ObjectKind::Cte => "cte",
            ObjectKind::Function => "function",
            ObjectKind::Subquery => "subquery",
            ObjectKind::Other => "object",
        };
        lines.push(value(t(key), o.name.clone()));
        if let Some(alias) = &o.alias {
            lines.push(value(t("alias"), alias.clone()));
        }
    }
    if let Some(index) = &n.index {
        lines.push(value(t("index"), index.clone()));
    }
    push(t("object"), lines);
    // The conditions.
    let lines = n
        .conditions
        .iter()
        .map(|c| Line::Value {
            label: c.kind.label().to_owned(),
            value: c.text.clone(),
            tip: Some(c.text.clone()),
            code: true,
        })
        .collect();
    push(t("conditions"), lines);
    // Estimated against real.
    let mut lines = Vec::new();
    let e = &n.estimates;
    let a = &n.actuals;
    let dash = "—".to_owned();
    let opt = |v: Option<String>| v.unwrap_or_else(|| dash.clone());
    if e.rows.is_some() || a.rows.is_some() {
        lines.push(Line::Compare {
            label: String::new(),
            estimated: t("estimated"),
            actual: t("actual"),
            head: true,
        });
        lines.push(Line::Compare {
            label: t("rows_per_loop"),
            estimated: opt(e.rows.map(figures::count)),
            actual: opt(a.rows.map(figures::number)),
            head: false,
        });
        if a.loops.is_some() {
            let loops = a.loops.unwrap_or(1.0).max(1.0);
            lines.push(Line::Compare {
                label: t("rows_all"),
                estimated: opt(e.rows.map(|r| figures::count(r * loops))),
                actual: opt(d.total_rows.map(figures::count)),
                head: false,
            });
            lines.push(Line::Compare {
                label: t("loops"),
                estimated: dash.clone(),
                actual: figures::count(loops),
                head: false,
            });
        }
        if a.total_ms.is_some() {
            lines.push(Line::Compare {
                label: t("time_per_loop"),
                estimated: dash.clone(),
                actual: format!(
                    "{} … {}",
                    opt(a.startup_ms.map(figures::ms_exact)),
                    opt(a.total_ms.map(figures::ms_exact))
                ),
                head: false,
            });
            lines.push(Line::Compare {
                label: t("time_all"),
                estimated: dash.clone(),
                actual: opt(d.inclusive_ms.map(figures::ms_exact)),
                head: false,
            });
            lines.push(Line::Compare {
                label: t("time_own"),
                estimated: dash.clone(),
                actual: opt(d.self_ms.map(figures::ms_exact)),
                head: false,
            });
        }
        if e.total_cost.is_some() {
            lines.push(Line::Compare {
                label: t("cost"),
                estimated: format!(
                    "{} … {}",
                    opt(e.startup_cost.map(figures::number)),
                    opt(e.total_cost.map(figures::number))
                ),
                actual: dash.clone(),
                head: false,
            });
        }
        if let Some(w) = e.width {
            lines.push(Line::Compare {
                label: t("width"),
                estimated: format!("{} B", figures::count(w)),
                actual: dash.clone(),
                head: false,
            });
        }
    }
    for (key, v) in [
        ("removed_filter", a.rows_removed_by_filter),
        ("removed_join", a.rows_removed_by_join_filter),
        ("removed_recheck", a.rows_removed_by_recheck),
        ("heap_fetches", a.heap_fetches),
        ("index_searches", a.index_searches),
        ("exact_blocks", a.exact_blocks),
        ("lossy_blocks", a.lossy_blocks),
    ] {
        if let Some(v) = v {
            lines.push(value(t(key), figures::count(v)));
        }
    }
    if d.approx {
        lines.push(value(t("approx"), i18n::t("profile.view.approx")));
    }
    push(t("estimated_actual"), lines);
    // I/O.
    if let Some(io) = &n.io {
        push(t("io"), io_lines(io));
    }
    // Memory and disk.
    let mut lines = Vec::new();
    if let Some(m) = &n.memory {
        if let Some(b) = m.peak_bytes {
            lines.push(bytes_line(t("peak"), b));
        }
        if let Some(method) = &m.sort_method {
            lines.push(value(t("sort_method"), method.clone()));
        }
        if let Some((b, place)) = m.sort_space {
            let place = match place {
                Storage::Memory => t("in_memory"),
                Storage::Disk => t("on_disk"),
            };
            lines.push(value_tip(
                t("sort_space"),
                format!("{} · {place}", figures::bytes(b)),
                format!("{b} B"),
            ));
        }
        if let Some(h) = &m.hash {
            if let Some(b) = h.buckets {
                lines.push(value(t("buckets"), figures::count(b as f64)));
            }
            if let Some(b) = h.batches {
                let original = h.original_batches.filter(|o| *o != b);
                let shown = match original {
                    Some(o) => format!("{} ({} {})", figures::count(b as f64), t("planned"), o),
                    None => figures::count(b as f64),
                };
                lines.push(value(t("batches"), shown));
            }
        }
        if let Some(c) = &m.cache {
            for (key, v) in [
                ("hits", c.hits),
                ("misses", c.misses),
                ("evictions", c.evictions),
                ("overflows", c.overflows),
            ] {
                if let Some(v) = v {
                    lines.push(value(t(key), figures::count(v as f64)));
                }
            }
        }
    }
    if let Some(spill) = &n.spill {
        let mut shown = spill.kind.id().replace('_', " ");
        if let Some(b) = spill.disk_bytes {
            shown = format!("{} · {shown}", figures::bytes(b));
        }
        lines.push(value(t("spill"), shown));
        if let Some(b) = spill.batches {
            lines.push(value(t("batches"), figures::count(b as f64)));
        }
    }
    push(t("memory"), lines);
    // Parallelism.
    if let Some(par) = &n.parallel {
        let mut lines = Vec::new();
        if let Some(planned) = par.planned {
            lines.push(value(t("planned_workers"), planned.to_string()));
        }
        if let Some(launched) = par.launched {
            lines.push(value(t("launched_workers"), launched.to_string()));
        }
        if par.aware {
            lines.push(value(t("parallel_aware"), t("yes")));
        }
        for w in &par.per_worker {
            let mut parts = Vec::new();
            if let Some(r) = w.rows {
                parts.push(i18n::tr!(
                    "profile.view.details.worker_rows",
                    rows = figures::count(r).as_str()
                ));
            }
            if let Some(ms) = w.total_ms {
                parts.push(figures::ms(ms));
            }
            lines.push(value(
                i18n::tr!("profile.view.details.worker", number = i64::from(w.number)),
                parts.join(" · "),
            ));
        }
        push(t("parallel"), lines);
    }
    if let Some(wal) = &n.wal {
        let mut lines = Vec::new();
        if let Some(r) = wal.records {
            lines.push(value(t("wal_records"), figures::count(r as f64)));
        }
        if let Some(r) = wal.full_page_images {
            lines.push(value(t("wal_fpi"), figures::count(r as f64)));
        }
        if let Some(b) = wal.bytes {
            lines.push(bytes_line(t("wal_bytes"), b));
        }
        push(t("wal"), lines);
    }
    // Output.
    let lines = n
        .output
        .iter()
        .enumerate()
        .map(|(i, o)| Line::Value {
            label: format!("{}", i + 1),
            value: o.clone(),
            tip: None,
            code: true,
        })
        .collect();
    push(t("output"), lines);
    // The engine's own.
    let engine = engine_title(report);
    push(engine, extras(&n.extra));
    // The step's problems.
    let lines = findings
        .iter()
        .enumerate()
        .filter(|(_, f)| f.nodes.contains(&id))
        .map(|(i, f)| Line::Problem {
            level: level(f.severity),
            title: analysis::texts(f).title,
            finding: i,
        })
        .collect();
    push(t("problems"), lines);
    out
}

/// "Specific to PostgreSQL" (the engine's name, else the driver's).
fn engine_title(report: &ProfileReport) -> String {
    let e = &report.profile.engine;
    let name = if e.name.is_empty() { e.driver.clone() } else { e.name.clone() };
    i18n::tr!("profile.view.details.specific", engine = name.as_str())
}

fn bytes_line(label: String, b: u64) -> Line {
    value_tip(label, figures::bytes(b), format!("{} B", figures::count(b as f64)))
}

fn io_lines(io: &Io) -> Vec<Line> {
    let mut lines = Vec::new();
    let blocks = |label: String, v: Option<u64>, lines: &mut Vec<Line>| {
        if let Some(v) = v {
            let shown = format!("{} · {}", figures::count(v as f64), figures::bytes(io.bytes(v)));
            lines.push(value_tip(
                label,
                shown,
                i18n::tr!("profile.view.details.blocks_tip", blocks = v as i64),
            ));
        }
    };
    blocks(t("shared_hit"), io.shared_hit, &mut lines);
    blocks(t("shared_read"), io.shared_read, &mut lines);
    blocks(t("shared_dirtied"), io.shared_dirtied, &mut lines);
    blocks(t("shared_written"), io.shared_written, &mut lines);
    blocks(t("local_hit"), io.local_hit, &mut lines);
    blocks(t("local_read"), io.local_read, &mut lines);
    blocks(t("temp_read"), io.temp_read, &mut lines);
    blocks(t("temp_written"), io.temp_written, &mut lines);
    for (key, v) in [
        ("read_time", io.read_ms),
        ("write_time", io.write_ms),
        ("temp_read_time", io.temp_read_ms),
        ("temp_write_time", io.temp_write_ms),
    ] {
        if let Some(ms) = v {
            lines.push(value(t(key), figures::ms_exact(ms)));
        }
    }
    lines
}

/// An extra's value as shown (its unit's way), and its raw form.
pub fn extra_value(v: &ExtraValue) -> (String, String) {
    match v {
        ExtraValue::Quantity(q) => (quantity(*q), v.text()),
        ExtraValue::Bool(b) => (if *b { t("yes") } else { t("no") }, b.to_string()),
        ExtraValue::Int(n) => (figures::count(*n as f64), n.to_string()),
        ExtraValue::Float(f) => (figures::number(*f), f.to_string()),
        other => (other.text(), other.text()),
    }
}

fn quantity(q: Quantity) -> String {
    match q.unit {
        Unit::Bytes => figures::bytes(q.value.max(0.0) as u64),
        Unit::Millis => figures::ms(q.value),
        Unit::Percent => analysis::quantity(q),
        Unit::Blocks => format!("{} {}", figures::count(q.value), t("blocks")),
        Unit::Pages => format!("{} {}", figures::count(q.value), t("pages")),
        Unit::Rows => format!("{} {}", figures::count(q.value), t("rows")),
        _ => figures::number(q.value),
    }
}

/// Extras as lines, by group (`JIT › Functions`), a group's members under its name.
fn extras(list: &[Extra]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut groups: Vec<&str> = Vec::new();
    for e in list {
        let g = e.group.as_deref().unwrap_or("");
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    for g in groups {
        for e in list.iter().filter(|e| e.group.as_deref().unwrap_or("") == g) {
            let label = e.label.clone().unwrap_or_else(|| e.key.clone());
            let label = if g.is_empty() { label } else { format!("{g} › {label}") };
            match &e.value {
                ExtraValue::Group(items) => {
                    for item in items {
                        let (shown, raw) = extra_value(&item.value);
                        let name = item.label.clone().unwrap_or_else(|| item.key.clone());
                        lines.push(value_tip(format!("{label} › {name}"), shown, raw));
                    }
                }
                v => {
                    let (shown, raw) = extra_value(v);
                    lines.push(value_tip(label, shown, format!("{} = {raw}", e.key)));
                }
            }
        }
    }
    lines
}

/// The statement's sections (nothing chosen).
fn statement(report: &ProfileReport) -> Vec<(String, Vec<Line>)> {
    let p = &report.profile;
    let s = &p.statement;
    let mut out = Vec::new();
    let mut lines = Vec::new();
    let e = &p.engine;
    let engine = match &e.version {
        Some(v) => format!("{} {v}", e.name),
        None => e.name.clone(),
    };
    if !engine.trim().is_empty() {
        lines.push(value(t("engine"), engine));
    }
    let depth = match p.depth {
        ProfileDepth::Estimated => t("depth_estimated"),
        ProfileDepth::Actual { timing: true } => t("depth_actual"),
        ProfileDepth::Actual { timing: false } => t("depth_actual_no_timing"),
    };
    lines.push(value(t("depth"), depth));
    lines.push(value(t("steps"), figures::count(p.nodes.len() as f64)));
    for (key, v) in [
        ("planning", s.planning_ms),
        ("execution", s.execution_ms),
        ("wall", s.wall_ms),
        ("first_row", s.first_row_ms),
        ("fetch", s.fetch_ms),
    ] {
        if let Some(ms) = v {
            lines.push(value_tip(t(key), figures::ms(ms), figures::ms_exact(ms)));
        }
    }
    if let Some(r) = s.rows_returned {
        lines.push(value(t("rows_returned"), figures::count(r as f64)));
    }
    if let Some(r) = s.rows_affected {
        lines.push(value(t("rows_affected"), figures::count(r as f64)));
    }
    if let Some(q) = s.query_id {
        lines.push(value(t("query_id"), q.to_string()));
    }
    out.push((t("statement"), lines));
    if let Some(io) = report.totals.io.or(s.io_total) {
        out.push((t("io"), io_lines(&io)));
    }
    if let Some(jit) = &s.jit {
        let mut lines = Vec::new();
        if let Some(f) = jit.functions {
            lines.push(value(t("jit_functions"), f.to_string()));
        }
        for (key, v) in [
            ("jit_generation", jit.generation_ms),
            ("jit_inlining", jit.inlining_ms),
            ("jit_optimization", jit.optimization_ms),
            ("jit_emission", jit.emission_ms),
            ("jit_total", jit.total_ms),
        ] {
            if let Some(ms) = v {
                lines.push(value(t(key), figures::ms_exact(ms)));
            }
        }
        out.push(("JIT".to_owned(), lines));
    }
    let lines = s
        .triggers
        .iter()
        .map(|tr| {
            let mut parts = Vec::new();
            if let Some(c) = tr.calls {
                parts.push(i18n::tr!("profile.view.details.calls", count = c as i64));
            }
            if let Some(ms) = tr.ms {
                parts.push(figures::ms(ms));
            }
            value(tr.name.clone(), parts.join(" · "))
        })
        .collect();
    out.push((t("triggers"), lines));
    let lines = s.settings.iter().map(|st| Line::Value {
        label: st.name.clone(),
        value: st.value.clone(),
        tip: None,
        code: true,
    });
    out.push((t("settings"), lines.collect()));
    if let Some(c) = &s.counters {
        let mut lines = Vec::new();
        for (key, v) in [
            ("vm_steps", c.vm_steps),
            ("fullscan", c.fullscan_steps),
            ("sorts", c.sorts),
            ("autoindex", c.autoindex),
            ("reprepare", c.reprepare),
            ("runs", c.runs),
            ("filter_hits", c.filter_hits),
            ("filter_misses", c.filter_misses),
        ] {
            if let Some(v) = v {
                lines.push(value(
                    i18n::t(&format!("profile.view.pill.{key}")),
                    figures::count(v as f64),
                ));
            }
        }
        if let Some(b) = c.mem_used {
            lines.push(bytes_line(t("mem_used"), b));
        }
        out.push((t("counters"), lines));
    }
    out.push((engine_title(report), extras(&s.extra)));
    let lines = p
        .notes
        .iter()
        .map(|n| {
            let text = i18n::t(&format!("profile.view.note.{}", n.id()));
            match n.argument() {
                Some(a) => value(text, figures::count(a as f64)),
                None => value(text, String::new()),
            }
        })
        .collect();
    out.push((t("notes"), lines));
    out
}

// ─── Drawing ───

/// Where a press on the details falls.
#[derive(Debug, Clone, PartialEq)]
pub enum DetailHit {
    /// A value (copied when clicked).
    Value(String),
    /// A problem of the step: its finding.
    Problem(usize),
}

/// The details' list: its sections drawn, scrolled, their values' tooltips.
#[derive(Debug, Default)]
pub struct DetailsList {
    pub scroll: f32,
    bar: ui_components::EdgeScrollbar,
    hits: Vec<(kernel::Rect, DetailHit, Option<String>)>,
    pub hover: Option<usize>,
    area: kernel::Rect,
    content: f32,
}

/// Sizes (logical pixels).
const HEAD_H: f32 = 28.0;
const ROW_H: f32 = 22.0;

impl DetailsList {
    pub fn hit(&self, p: kernel::Point) -> Option<DetailHit> {
        if !self.area.contains(p) {
            return None;
        }
        self.hits.iter().find(|(r, ..)| r.contains(p)).map(|(_, h, _)| h.clone())
    }

    pub fn tooltip_at(&self, p: kernel::Point) -> Option<ui_components::Tip> {
        if !self.area.contains(p) {
            return None;
        }
        let (r, _, tip) = self.hits.iter().find(|(r, ..)| r.contains(p))?;
        Some(ui_components::Tip::new(*r, tip.clone()?))
    }

    /// The mouse at `p`: the row under it lit (its scrollbar is the view's control).
    pub fn hover(&mut self, p: kernel::Point) -> bool {
        let at = if self.area.contains(p) {
            self.hits.iter().position(|(r, ..)| r.contains(p))
        } else {
            None
        };
        let changed = at != self.hover;
        self.hover = at;
        changed
    }

    /// Its scrollbar and the offset it moves, as a control of the view's.
    pub fn thumb(&mut self) -> ui_components::control::Thumb<'_> {
        ui_components::control::Thumb { bar: &mut self.bar, offset: &mut self.scroll }
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        let max = (self.content - self.area.size.height).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    /// How tall `sections` are drawn (physical pixels).
    pub fn height_of(sections: &[Section], scale: f32) -> f32 {
        let px = |v: f32| (v * scale).round();
        sections.iter().map(|s| px(HEAD_H) + px(ROW_H) * s.lines.len() as f32).sum::<f32>()
            + px(8.0)
    }

    pub fn paint(
        &mut self,
        painter: ui_components::Painter<'_>,
        scale: f32,
        area: kernel::Rect,
        sections: &[Section],
    ) {
        let ui_components::Painter { text, scene, palette, icons } = painter;
        use kernel::Rect;
        use render_api::Quad;
        use ui_components::ds::{self, Ctx};
        use ui_kit::{draw_icon, draw_text};
        let p = palette;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.area = area;
        self.hits.clear();
        self.content = Self::height_of(sections, scale);
        self.scroll = self.scroll.clamp(0.0, (self.content - area.size.height).max(0.0));
        let layer = scene.push_layer(Some(area));
        let head = ds.ui_font(-1.5).with_weight(text_layout::Font::SEMIBOLD);
        let font = ds.ui_font(-1.0);
        let code = ds.code_font(-1.5);
        let pad = px(12.0);
        let (x0, x1) = (area.min_x() + pad, area.max_x() - pad);
        let label_w = ((x1 - x0) * 0.42).round();
        let mut y = area.min_y() - self.scroll;
        let mut index = 0usize;
        for section in sections {
            if y + px(HEAD_H) >= area.min_y() && y <= area.max_y() {
                let title = section.title.to_uppercase();
                let shaped = text.shape_line(&title, &head);
                draw_text(
                    layer,
                    &shaped,
                    x0.round(),
                    ds::optical_baseline(y + px(6.0), px(HEAD_H) - px(6.0), &head),
                    p.text_muted,
                );
                ds::divider(
                    layer,
                    &ds,
                    (x0 + shaped.width + px(8.0)).round(),
                    x1,
                    (y + px(HEAD_H) / 2.0 + px(3.0)).round(),
                );
            }
            y += px(HEAD_H);
            for line in &section.lines {
                let row = Rect::new(area.min_x(), y.round(), area.size.width, px(ROW_H));
                let visible = row.max_y() >= area.min_y() && row.min_y() <= area.max_y();
                let base = ds::optical_baseline(row.min_y(), row.size.height, &font);
                match line {
                    Line::Value { label, value, tip, code: is_code } => {
                        if visible {
                            if self.hover == Some(index) && !value.is_empty() {
                                layer.quad(
                                    Quad::filled(
                                        Rect::new(
                                            x0 - px(6.0),
                                            row.min_y(),
                                            x1 - x0 + px(12.0),
                                            row.size.height,
                                        ),
                                        p.text.with_alpha(0.05),
                                    )
                                    .rounded(px(4.0)),
                                );
                            }
                            let shown = ds::ellipsize(text, label, &font, label_w - px(8.0));
                            let shaped = text.shape_line(&shown, &font);
                            draw_text(layer, &shaped, x0.round(), base, p.text_muted);
                            let vx = x0 + label_w;
                            if *is_code {
                                let one = kernel::text::one_line(value).into_owned();
                                let metrics = text.metrics(&code);
                                ui_components::code_tokens::draw_code(
                                    layer,
                                    text,
                                    ui_components::CodeLine {
                                        source: &one,
                                        extension: "sql",
                                        font: &code,
                                        x: vx,
                                        baseline: ui_kit::centered_baseline(
                                            row.min_y(),
                                            row.size.height,
                                            &metrics,
                                        ),
                                        palette: p,
                                        max_x: x1,
                                    },
                                );
                            } else {
                                let shown = ds::ellipsize(text, value, &font, x1 - vx);
                                let shaped = text.shape_line(&shown, &font);
                                draw_text(layer, &shaped, vx.round(), base, p.text);
                            }
                        }
                        let full = if value.is_empty() { label.clone() } else { value.clone() };
                        let tip = tip
                            .clone()
                            .or_else(|| (full.chars().count() > 28).then(|| full.clone()));
                        self.hits.push((row, DetailHit::Value(full), tip));
                    }
                    Line::Compare { label, estimated, actual, head: is_head } => {
                        if visible {
                            let f = if *is_head { &head } else { &font };
                            let col = (x1 - x0 - label_w) / 2.0;
                            let color = if *is_head { p.text_muted } else { p.text };
                            let shaped = text.shape_line(label, f);
                            draw_text(layer, &shaped, x0.round(), base, p.text_muted);
                            for (i, v) in [estimated, actual].into_iter().enumerate() {
                                let shown = ds::ellipsize(text, v, f, col - px(6.0));
                                let shaped = text.shape_line(&shown, f);
                                let right = x0 + label_w + col * (i as f32 + 1.0);
                                draw_text(
                                    layer,
                                    &shaped,
                                    (right - shaped.width).round(),
                                    base,
                                    if i == 1 && !*is_head { p.text } else { color },
                                );
                            }
                        }
                        self.hits.push((
                            row,
                            DetailHit::Value(format!("{label}\t{estimated}\t{actual}")),
                            None,
                        ));
                    }
                    Line::Problem { level: l, title, finding } => {
                        if visible {
                            if self.hover == Some(index) {
                                layer.quad(
                                    Quad::filled(
                                        Rect::new(
                                            x0 - px(6.0),
                                            row.min_y(),
                                            x1 - x0 + px(12.0),
                                            row.size.height,
                                        ),
                                        l.color(p).with_alpha(0.08),
                                    )
                                    .rounded(px(4.0)),
                                );
                            }
                            if let Some(id) = icons.id(l.icon()) {
                                let s = px(13.0);
                                draw_icon(
                                    layer,
                                    icons,
                                    id,
                                    Rect::new(
                                        x0.round(),
                                        (row.min_y() + (row.size.height - s) / 2.0).round(),
                                        s,
                                        s,
                                    ),
                                    l.color(p),
                                );
                            }
                            let shown = ds::ellipsize(text, title, &font, x1 - x0 - px(20.0));
                            let shaped = text.shape_line(&shown, &font);
                            draw_text(layer, &shaped, (x0 + px(20.0)).round(), base, p.text);
                        }
                        self.hits.push((row, DetailHit::Problem(*finding), None));
                    }
                }
                index += 1;
                y += px(ROW_H);
            }
        }
        self.bar.paint(scene, p, scale, area, self.content, self.scroll);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/profile/details.rs"]
mod tests;
