//! The toolbar's layout: where each control goes for the width there is, worked out once
//! per width and state (a [`kernel::Memo`] in the grid), never overlapping. The pages
//! (`|< <  1–500 of N  > >|`, the page size) grow from the left, the actions (transposed,
//! find, refresh, the data extractor's pill, editing) from the right. When they don't all fit, the lower ones go into
//! a "…" menu at the right end, in [`COLLAPSE`]'s order — the page size first, the pages'
//! arrows last — and what then fits again in the room left comes back (the most wanted
//! first); the range stays (cut, at worst), as do what's under way (Submit, Revert, the
//! cancel and the loader).

/// A control of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    First,
    Back,
    /// The rows shown (`1–500`).
    Range,
    /// `of N` beside the range (only hidden, never in the menu).
    Of,
    Forward,
    Last,
    PageSize,
    Loader,
    Cancel,
    Add,
    Delete,
    Revert,
    Submit,
    Refresh,
    Find,
    Transpose,
    /// The data extractor chosen ("CSV ▾": its popover).
    Extractor,
    /// The "…" holding what didn't fit.
    More,
}

impl Tool {
    /// Its id in the "…" menu's rows (`None`: never there).
    pub fn id(self) -> Option<&'static str> {
        Some(match self {
            Tool::First => "first",
            Tool::Back => "back",
            Tool::Forward => "forward",
            Tool::Last => "last",
            Tool::PageSize => "page-size",
            Tool::Add => "add",
            Tool::Delete => "delete",
            Tool::Refresh => "refresh",
            Tool::Find => "find",
            Tool::Transpose => "transpose",
            Tool::Extractor => "copy-as",
            Tool::Range
            | Tool::Of
            | Tool::Loader
            | Tool::Cancel
            | Tool::Revert
            | Tool::Submit
            | Tool::More => return None,
        })
    }

    /// The tool of a "…" menu row's id.
    pub fn from_id(id: &str) -> Option<Self> {
        MENU_ORDER.into_iter().find(|t| t.id() == Some(id))
    }
}

/// What goes first when there isn't room, step by step (a step's tools together).
pub const COLLAPSE: &[&[Tool]] = &[
    &[Tool::PageSize],
    &[Tool::Transpose],
    &[Tool::Find],
    &[Tool::Extractor],
    &[Tool::Refresh],
    &[Tool::Of],
    &[Tool::First, Tool::Last],
    &[Tool::Add],
    &[Tool::Delete],
    &[Tool::Back, Tool::Forward],
];

/// The menu's order (as the toolbar has them: the pages, then editing, then the view).
pub const MENU_ORDER: [Tool; 11] = [
    Tool::First,
    Tool::Back,
    Tool::Forward,
    Tool::Last,
    Tool::PageSize,
    Tool::Add,
    Tool::Delete,
    Tool::Refresh,
    Tool::Find,
    Tool::Transpose,
    Tool::Extractor,
];

/// The sizes the layout works with (physical pixels: the grid's `ds.px` of its logical
/// sizes, so a layout at scale 1 reads in logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// From the bar's edges.
    pub pad: f32,
    /// An icon button's side, and the gap after one.
    pub button: f32,
    pub gap: f32,
    /// Around the range's text.
    pub label_pad: f32,
    /// After `of N`, and before the page size.
    pub after_of: f32,
    pub before_size: f32,
    /// On each side of the editing group's divider.
    pub divider: f32,
    /// Between Submit and Revert.
    pub after_submit: f32,
    /// The loader's room.
    pub loader: f32,
}

impl Metrics {
    /// At `px` (logical → physical, rounded).
    pub fn new(px: impl Fn(f32) -> f32) -> Self {
        Self {
            pad: px(10.0),
            button: px(28.0),
            gap: px(2.0),
            label_pad: px(6.0),
            after_of: px(8.0),
            before_size: px(8.0),
            divider: px(8.0),
            after_submit: px(4.0),
            loader: px(22.0),
        }
    }
}

/// What the toolbar holds now, and how wide its texts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolbarSpec {
    /// The bar's left and right edges.
    pub min_x: f32,
    pub max_x: f32,
    pub metrics: Metrics,
    /// Pages (the arrows and the size); otherwise every row in one.
    pub paged: bool,
    /// Rows can be added and deleted.
    pub editable: bool,
    /// Changes wait (Submit and Revert shown).
    pub pending: bool,
    /// A slow read can be stopped.
    pub cancel: bool,
    pub loading: bool,
    /// The range's text, `of N`'s (none: no total), the page size's pill, Submit's.
    pub range_w: f32,
    pub of_w: Option<f32>,
    pub size_w: f32,
    pub submit_w: f32,
    /// The data extractor's pill (`0`: none).
    pub extractor_w: f32,
}

/// Where everything goes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolbarLayout {
    /// Each control shown: its left edge and width (the texts' edges as they fall, drawn
    /// rounded).
    pub placed: Vec<(Tool, f32, f32)>,
    /// What's in the "…" menu, in [`MENU_ORDER`].
    pub collapsed: Vec<Tool>,
    /// The editing group's divider (its x), when it has something beside it.
    pub divider: Option<f32>,
    /// How wide the range may be drawn (cut with "…" below its width).
    pub range_room: f32,
}

impl ToolbarLayout {
    /// Control `tool`'s left edge and width, when shown.
    pub fn get(&self, tool: Tool) -> Option<(f32, f32)> {
        self.placed.iter().find(|(t, ..)| *t == tool).map(|&(_, x, w)| (x, w))
    }

    pub fn shows(&self, tool: Tool) -> bool {
        self.get(tool).is_some()
    }
}

/// The tools `spec` has (whatever the room).
fn present(spec: &ToolbarSpec) -> Vec<Tool> {
    let mut tools = Vec::new();
    if spec.paged {
        tools.extend([Tool::First, Tool::Back, Tool::Forward, Tool::Last, Tool::PageSize]);
    } else {
        tools.push(Tool::PageSize);
    }
    if spec.of_w.is_some() {
        tools.push(Tool::Of);
    }
    if spec.editable {
        tools.extend([Tool::Add, Tool::Delete]);
    }
    tools.extend([Tool::Refresh, Tool::Find, Tool::Transpose]);
    if spec.extractor_w > 0.0 {
        tools.push(Tool::Extractor);
    }
    tools
}

/// The layout of `spec`: everything when it fits, else less and less (see the module's doc).
pub fn layout(spec: &ToolbarSpec) -> ToolbarLayout {
    let present = present(spec);
    let mut hidden: Vec<Tool> = Vec::new();
    let mut taken: Vec<&[Tool]> = Vec::new();
    let mut steps = COLLAPSE.iter().filter(|step| step.iter().any(|t| present.contains(t)));
    let fits = loop {
        if place(spec, &hidden).1 {
            break true;
        }
        let Some(step) = steps.next() else { break false };
        hidden.extend(step.iter().copied());
        taken.push(step);
    };
    // What was put away on the way and fits again in the room left comes back, the most
    // wanted first (a narrow button where a wide pill didn't fit).
    if fits {
        for step in taken.iter().rev() {
            let fewer: Vec<Tool> = hidden.iter().copied().filter(|t| !step.contains(t)).collect();
            if place(spec, &fewer).1 {
                hidden = fewer;
            }
        }
    }
    let (placed, fits) = place(spec, &hidden);
    finish(spec, placed, &hidden, &present, fits)
}

/// The layout with `hidden` left out, and whether the left side fits beside the right one.
fn place(spec: &ToolbarSpec, hidden: &[Tool]) -> (ToolbarLayout, bool) {
    let m = spec.metrics;
    let shown = |t: Tool| !hidden.contains(&t);
    let mut out = ToolbarLayout::default();
    // The right side, from the edge.
    let mut right = spec.max_x - m.pad;
    let button = |out: &mut ToolbarLayout, right: &mut f32, tool: Tool| {
        *right -= m.button;
        out.placed.push((tool, *right, m.button));
        *right -= m.gap;
    };
    let menu = hidden.iter().any(|t| *t != Tool::Of);
    if menu {
        button(&mut out, &mut right, Tool::More);
    }
    for tool in [Tool::Transpose, Tool::Find, Tool::Refresh] {
        if shown(tool) {
            button(&mut out, &mut right, tool);
        }
    }
    if spec.extractor_w > 0.0 && shown(Tool::Extractor) {
        right -= m.divider;
        let x = (right - spec.extractor_w).round();
        out.placed.push((Tool::Extractor, x, spec.extractor_w));
        right = x - m.gap;
    }
    let edits = spec.editable && (shown(Tool::Add) || shown(Tool::Delete));
    if edits || spec.pending {
        right -= m.divider;
        out.divider = Some(right.round());
        right -= m.divider;
        if spec.pending {
            let x = (right - spec.submit_w).round();
            out.placed.push((Tool::Submit, x, spec.submit_w));
            right = x - m.after_submit;
            button(&mut out, &mut right, Tool::Revert);
        }
        if spec.editable {
            for tool in [Tool::Delete, Tool::Add] {
                if shown(tool) {
                    button(&mut out, &mut right, tool);
                }
            }
        }
    }
    if spec.cancel {
        button(&mut out, &mut right, Tool::Cancel);
    }
    if spec.loading {
        right -= m.loader;
        out.placed.push((Tool::Loader, right, m.loader));
    }
    let limit = right - m.pad;

    // The left side.
    let mut x = spec.min_x + m.pad;
    let mut end = x;
    let left_button = |out: &mut ToolbarLayout, x: &mut f32, end: &mut f32, tool: Tool| {
        let at = x.round();
        out.placed.push((tool, at, m.button));
        *end = at + m.button;
        *x = *end + m.gap;
    };
    if spec.paged {
        for tool in [Tool::First, Tool::Back] {
            if shown(tool) {
                left_button(&mut out, &mut x, &mut end, tool);
            }
        }
    }
    x += m.label_pad;
    out.placed.push((Tool::Range, x, spec.range_w));
    out.range_room = spec.range_w;
    end = x + spec.range_w;
    x += spec.range_w + m.label_pad;
    if let Some(of_w) = spec.of_w.filter(|_| shown(Tool::Of)) {
        out.placed.push((Tool::Of, x, of_w));
        end = x + of_w;
        x += of_w + m.after_of;
    }
    if spec.paged {
        for tool in [Tool::Forward, Tool::Last] {
            if shown(tool) {
                left_button(&mut out, &mut x, &mut end, tool);
            }
        }
    }
    if shown(Tool::PageSize) {
        x += m.before_size;
        let at = x.round();
        out.placed.push((Tool::PageSize, at, spec.size_w));
        end = at + spec.size_w;
    }
    (out, end <= limit)
}

/// The last layout tried, its menu filled, the range cut to what's left when even the
/// last step didn't make room.
fn finish(
    spec: &ToolbarSpec,
    mut out: ToolbarLayout,
    hidden: &[Tool],
    present: &[Tool],
    fits: bool,
) -> ToolbarLayout {
    out.collapsed =
        MENU_ORDER.iter().copied().filter(|t| hidden.contains(t) && present.contains(t)).collect();
    if fits {
        out.range_room = spec.range_w;
        return out;
    }
    // Nothing more to put away: the range takes the room up to the right side.
    let right = out
        .placed
        .iter()
        .filter(|(t, ..)| !matches!(t, Tool::First | Tool::Back | Tool::Range))
        .map(|&(_, x, _)| x)
        .fold(spec.max_x - spec.metrics.pad, f32::min);
    let start = out.get(Tool::Range).map_or(spec.min_x, |(x, _)| x);
    out.range_room = (right - spec.metrics.pad - start).clamp(0.0, spec.range_w);
    if out.range_room <= 0.0 {
        // Not even a "…" of it: left out (the right side alone is wider than the bar).
        out.placed.retain(|(t, ..)| *t != Tool::Range);
    }
    // The arrows after it follow its end (they were placed after its whole width).
    let shift = spec.range_w - out.range_room;
    for (tool, x, _) in &mut out.placed {
        if matches!(tool, Tool::Forward | Tool::Last | Tool::Of | Tool::PageSize) {
            *x = (*x - shift).round();
        }
    }
    out
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/toolbar.rs"]
mod tests;
