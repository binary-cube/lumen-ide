# Icicles (`ui_components::icicle`)

**For:** a tree whose nodes have sizes, read as bars: a flame graph drawn top down. A
level is a row, each node as wide as its value within its parent, its children under it
from the left — so what's left of a parent uncovered is its own part.

**Where it's used:** the database console's query profile (Flame), the profiler's flame
view.

## Anatomy

```
 ┌ band: ← back · All › parse › execute          width = time ▁▂▃▅▇ heat ┐
 ├ axis: 0      20 ms      40 ms      60 ms                              ┤
 │ ┌───────────────── root ───────────────────────────────────────────┐ │
 │ ┌──── child A ─────────┐┌─ child B ─┐┌…┐   ← narrow ones joined     │
 │ ┌ A1 ┐┌─ A2 ─┐          ┌⚠ B1 ┐           ← outlined, with a sign    │
 └──────────────────────────────────────────────────────────────────────┘
```

- `layout` (pure): the bars of the nodes in a width, from the zoomed node or every root;
  children worth more than their parent (rounding, parallel workers) are scaled into it
  and marked approximate; bars under `MIN_PX` next to one another are joined into one "…".
- `Icicle`: keeps the nodes, the zoom and its history, the choice, the hover; lays out
  once per width, nodes and zoom (`kernel::Memo`).

## Options

| Option | Default | Effect |
|---|---|---|
| `Icicle::new()` | — | empty, at scale 1 |
| `set_nodes(Vec<IcicleNode>)` | — | the tree (any order); zoom and choice kept while still nodes |
| `IcicleNode` | — | `parent`, `value`, `self_value`, `label`, `detail`, `tone: Ink` (its fill, a heat), `mark: Option<Ink>` (an outline: a problem), `sign` (an icon), `tip` |
| `set_labels(IcicleLabels)` | empty | the caller's words: `all`, `back`, `legend`, `merged` (`{count}`), `empty`, `approx` |
| `set_format(fn(f64) -> String)` | none: no axis | how values are written (axis, merged bars' tooltips) |
| `select(Some(i))` | `None` | the node chosen elsewhere (another view of the same plan) |
| `IciclePaint { clip }` | `None` | what of it shows when a page scrolls under it |

## States and interaction

- It is a `Control`: a press chooses a bar and zooms into it (its subtree the whole
  width), a crumb or ← goes back; `take_event` says `Selected(i)` or `Zoomed`.
- Hover lights a bar; the cursor is a hand over one; the tooltip is the node's `tip`
  (a merged bar's says how many it joins, a scaled one adds `approx`).
- The wheel goes down its levels.
- Keys (focusable): ↑ the parent, ↓ the widest child, ← → its siblings, ↵ zooms in,
  ⌫ / Esc back, Home all.
- Nothing with a value: the empty state with `labels.empty`.

## Accessibility

Nothing yet: neither the icicle nor its bars describe themselves (see the audit). A bar
should be a tree item (its label, its level, its value), the band's crumbs buttons.

## Cost

- An identical repaint allocates nothing (a test holds it): the cells read from their
  cache, the crumbs' path a kept buffer, the labels cut while drawn.
- Layout: once per (nodes' generation, width, zoom); 10 000 nodes are laid out once per
  width (`ten_thousand_nodes_are_laid_out_once_per_width`).
- A frame draws only the rows in view, cut to `clip`.
- Today a frame still copies the laid-out bars and builds the crumbs' and the axis'
  strings (see the audit).

## Example

```rust
self.icicle.set_format(if timed { time } else { cost });
self.icicle.set_labels(IcicleLabels {
    all: i18n::t("profile.view.flame_all"),
    back: i18n::t("profile.view.flame_back"),
    legend: i18n::t("profile.view.flame_legend"),
    merged: i18n::t("profile.view.flame_merged"),
    empty: i18n::t("profile.view.flame_empty"),
    approx: i18n::t("profile.view.approx"),
});
self.icicle.set_nodes(nodes(report, findings));
// paint:
self.icicle.paint(IciclePaint { bounds: area, scale, text, scene, palette, icons, clip: None });
```

## Don't

- Call `set_nodes` every frame: only when the tree changes (it bumps the generation and
  the layout is made again).
- Color bars by hand in the view: give each node its `tone` (`Ink::heat(t)`) and `mark`.
- Re-implement its keys or crumbs in the owner: hand it the input as a `Control`.
