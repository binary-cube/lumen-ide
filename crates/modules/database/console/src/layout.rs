//! The console bar's layout: where each control goes for the width there is, worked out
//! once per width and state (a [`kernel::Memo`] in the bar), never overlapping. Left to
//! right: Run (▶ and its ▾), the history and the parameters, the
//! transaction's mode (Commit and Rollback while one is open) and Stop, Playground /
//! Script, the settings and the Services window's toggle; on the right, the database and
//! schema it runs on, a read-only or an old driver's note before it, and what the last run
//! did in the room left; an attached SQL file's source (39n) left of the schema. When they
//! don't all fit, the run's words shrink first, then the
//! lower controls go into a "…" menu after the left group, in [`COLLAPSE`]'s order; Run,
//! Stop, the transaction, the schema, an attached file's source and a production source's
//! badge always stay.

/// A control of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    Run,
    /// Run's ▾: Execute, Execute Script, Explain Plan, Explain Analyze.
    RunMenu,
    History,
    Params,
    /// `Tx: Auto ▾`.
    Tx,
    Commit,
    Rollback,
    Stop,
    /// `Playground ▾` / `Script ▾`.
    Mode,
    Settings,
    Services,
    /// What the last run did (`12 rows · 4 ms`), or what runs.
    Status,
    /// "old driver: limited results".
    Limited,
    /// "read-only".
    ReadOnly,
    /// "PROD": a production source (always shown).
    Production,
    /// `database.schema ▾`.
    Place,
    /// An attached SQL file's source `▾` (the sources, Detach).
    Source,
    /// The "…" holding what didn't fit.
    More,
}

/// What goes first when there isn't room, step by step (a step's tools together).
pub const COLLAPSE: &[&[Tool]] = &[
    &[Tool::Services],
    &[Tool::Settings],
    &[Tool::History, Tool::Params],
    &[Tool::Limited],
    &[Tool::ReadOnly],
    &[Tool::Mode],
];

/// The "…" menu's order (as the bar has them).
pub const MENU_ORDER: [Tool; 7] = [
    Tool::History,
    Tool::Params,
    Tool::Mode,
    Tool::Settings,
    Tool::Services,
    Tool::ReadOnly,
    Tool::Limited,
];

/// The sizes the layout works with (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub pad: f32,
    /// An icon button's side.
    pub button: f32,
    /// Run's ▾ beside it.
    pub chevron: f32,
    pub gap: f32,
    /// On each side of a group's divider.
    pub divider: f32,
    /// The least the status keeps before it goes (its icon and a few letters).
    pub status_min: f32,
}

impl Metrics {
    /// At `px` (logical → physical, rounded).
    pub fn new(px: impl Fn(f32) -> f32) -> Self {
        Self {
            pad: px(8.0),
            button: px(28.0),
            chevron: px(16.0),
            gap: px(2.0),
            divider: px(7.0),
            status_min: px(64.0),
        }
    }
}

/// What the bar holds now, and how wide its texts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSpec {
    pub min_x: f32,
    pub max_x: f32,
    pub metrics: Metrics,
    /// A transaction is open (Commit and Rollback shown).
    pub transaction: bool,
    /// The pills' widths: the transaction's, the mode's, the place's.
    pub tx_w: f32,
    pub mode_w: f32,
    pub place_w: f32,
    /// The status's full width (none: nothing to say).
    pub status_w: Option<f32>,
    pub limited_w: Option<f32>,
    pub read_only_w: Option<f32>,
    /// The production badge's width (none: not production).
    pub production_w: Option<f32>,
    /// An attached file's source pill's width (none: a console's bar).
    pub source_w: Option<f32>,
}

/// Where everything goes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BarLayout {
    /// Each control shown: its left edge and width.
    pub placed: Vec<(Tool, f32, f32)>,
    /// What's in the "…" menu, in [`MENU_ORDER`].
    pub collapsed: Vec<Tool>,
    /// The groups' dividers (their x).
    pub dividers: Vec<f32>,
    /// How wide the status may be drawn (cut with "…" below its width).
    pub status_room: f32,
}

impl BarLayout {
    /// Control `tool`'s left edge and width, when shown.
    pub fn get(&self, tool: Tool) -> Option<(f32, f32)> {
        self.placed.iter().find(|(t, ..)| *t == tool).map(|&(_, x, w)| (x, w))
    }

    pub fn shows(&self, tool: Tool) -> bool {
        self.get(tool).is_some()
    }
}

/// The layout of `spec`: everything when it fits, else less and less (see the module's doc).
pub fn layout(spec: &BarSpec) -> BarLayout {
    let mut hidden: Vec<Tool> = Vec::new();
    let present = present(spec);
    let mut steps = COLLAPSE.iter().filter(|step| step.iter().any(|t| present.contains(t)));
    loop {
        let (out, fits) = place(spec, &hidden);
        if fits {
            return finish(out, &hidden, &present);
        }
        match steps.next() {
            Some(step) => hidden.extend(step.iter().copied()),
            None => return finish(out, &hidden, &present),
        }
    }
}

/// The tools `spec` has (whatever the room).
fn present(spec: &BarSpec) -> Vec<Tool> {
    let mut tools = vec![
        Tool::Run,
        Tool::RunMenu,
        Tool::History,
        Tool::Params,
        Tool::Tx,
        Tool::Stop,
        Tool::Mode,
        Tool::Settings,
        Tool::Services,
        Tool::Place,
    ];
    if spec.transaction {
        tools.extend([Tool::Commit, Tool::Rollback]);
    }
    if spec.limited_w.is_some() {
        tools.push(Tool::Limited);
    }
    if spec.read_only_w.is_some() {
        tools.push(Tool::ReadOnly);
    }
    if spec.production_w.is_some() {
        tools.push(Tool::Production);
    }
    if spec.source_w.is_some() {
        tools.push(Tool::Source);
    }
    if spec.status_w.is_some() {
        tools.push(Tool::Status);
    }
    // No Services window to toggle: no button for it.
    if !workbench_model::presence::command(workbench_model::TOGGLE_SERVICES) {
        tools.retain(|t| *t != Tool::Services);
    }
    tools
}

/// The layout with `hidden` left out, and whether it fits (the status at least its least).
fn place(spec: &BarSpec, hidden: &[Tool]) -> (BarLayout, bool) {
    let m = spec.metrics;
    let shown = |t: Tool| !hidden.contains(&t);
    let mut out = BarLayout::default();
    let mut x = spec.min_x + m.pad;
    let push = |out: &mut BarLayout, x: &mut f32, tool: Tool, w: f32| {
        let at = x.round();
        out.placed.push((tool, at, w));
        *x = at + w + m.gap;
    };
    let divider = |out: &mut BarLayout, x: &mut f32| {
        *x += m.divider - m.gap;
        out.dividers.push(x.round());
        *x += m.divider;
    };
    push(&mut out, &mut x, Tool::Run, m.button);
    x -= m.gap;
    push(&mut out, &mut x, Tool::RunMenu, m.chevron);
    if shown(Tool::History) {
        divider(&mut out, &mut x);
        push(&mut out, &mut x, Tool::History, m.button);
        push(&mut out, &mut x, Tool::Params, m.button);
    }
    divider(&mut out, &mut x);
    push(&mut out, &mut x, Tool::Tx, spec.tx_w);
    if spec.transaction {
        push(&mut out, &mut x, Tool::Commit, m.button);
        push(&mut out, &mut x, Tool::Rollback, m.button);
    }
    push(&mut out, &mut x, Tool::Stop, m.button);
    if shown(Tool::Mode) {
        divider(&mut out, &mut x);
        push(&mut out, &mut x, Tool::Mode, spec.mode_w);
    }
    let tail: Vec<Tool> =
        [Tool::Settings, Tool::Services].into_iter().filter(|t| shown(*t)).collect();
    if !tail.is_empty() {
        divider(&mut out, &mut x);
        for tool in tail {
            push(&mut out, &mut x, tool, m.button);
        }
    }
    let menu = hidden.iter().any(|t| *t != Tool::Status);
    if menu {
        push(&mut out, &mut x, Tool::More, m.button);
    }
    let left_end = x - m.gap;

    // The right side, from the edge.
    let mut right = spec.max_x - m.pad;
    let from_right = |out: &mut BarLayout, right: &mut f32, tool: Tool, w: f32| {
        let at = (*right - w).round();
        out.placed.push((tool, at, w));
        *right = at - m.gap * 3.0;
    };
    from_right(&mut out, &mut right, Tool::Place, spec.place_w);
    // An attached file's source: what it runs on, never put away (cut to the room left,
    // its name then shortened).
    if let Some(w) = spec.source_w {
        let room = right - left_end - m.divider * 2.0;
        from_right(&mut out, &mut right, Tool::Source, w.min(room.max(m.button * 2.0)).round());
    }
    // Production: never put away (it's why every write asks).
    if let Some(w) = spec.production_w {
        from_right(&mut out, &mut right, Tool::Production, w);
    }
    if let Some(w) = spec.limited_w.filter(|_| shown(Tool::Limited)) {
        from_right(&mut out, &mut right, Tool::Limited, w);
    }
    if let Some(w) = spec.read_only_w.filter(|_| shown(Tool::ReadOnly)) {
        from_right(&mut out, &mut right, Tool::ReadOnly, w);
    }
    let room = right - left_end - m.divider * 2.0;
    let mut fits = room >= 0.0;
    if let Some(w) = spec.status_w {
        let take = w.min(room);
        fits = room >= w.min(m.status_min);
        if take > 0.0 {
            let at = (right - take).round();
            out.placed.push((Tool::Status, at, take));
            out.status_room = take;
        }
    }
    (out, fits)
}

fn finish(mut out: BarLayout, hidden: &[Tool], present: &[Tool]) -> BarLayout {
    out.collapsed =
        MENU_ORDER.iter().copied().filter(|t| hidden.contains(t) && present.contains(t)).collect();
    if out.collapsed.is_empty() {
        out.placed.retain(|(t, ..)| *t != Tool::More);
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/layout.rs"]
mod tests;
