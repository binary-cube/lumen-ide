# How a component is made

Every component has the same parts. A new one copies this shape; an existing one that
doesn't is brought to it (plan 95b).

## 1. Its spec: what it shows, with defaults

```rust
pub struct ButtonSpec<'a> { kind, label: Option<&'a str>, icon, key, on, enabled, hovered, … }

impl<'a> ButtonSpec<'a> {
    pub fn new(kind: ButtonKind) -> Self { /* today's look */ }
    pub fn label(mut self, l: &'a str) -> Self { self.label = Some(l); self }
    // one method per option, each returning Self
}
```

- `new(…)` (or `Default`) is the look every caller has today. **A new option never moves
  a pixel where it isn't asked for.**
- Options are named for what they mean (`on`, `busy`, `compact`), never for how they're
  drawn (`blue`, `bold`).
- A spec borrows (`&'a str`): building it each frame costs nothing.

## 2. Its look: from the theme

A component reads its colors, fonts and sizes from `Ctx` (the palette, the scale, the
appearing opacity) — or from a `…Look` made from it (`ChartLook::new(ctx, background)`,
`TextInputStyle`, `GlassLook`). No color, font size or pixel value is written in a view.
A look made each frame allocates nothing (fixed arrays, borrowed fonts).

## 3. Its paint

```rust
pub fn button(layer: &mut Layer, text: &mut TextSystem, icons: &Icons, ctx: &Ctx<'_>, rect: Rect, spec: ButtonSpec<'_>)
```

- It draws in the rectangle it's given and never outside it; it returns the areas a press
  is told by (or its owner keeps them in its hit table).
- Without state: a function. With state (a list's scroll, a chart's layout, a field's
  text): a struct whose `paint(&mut self, …)` draws and remembers where.
- Only quads, glyphs and images exist on the screen. What needs curves (a chart's lines, a
  diagram) is rasterized off the UI thread (`vector-tiles`) and drawn as images.

## 4. Its interaction: `Control`

A part with state implements `ui_components::control::Control`: `area`, `press`
(`Took` / `Held` / `Ignored`), `drag`, `release`, `hover`, `scroll`, `key`, `text`,
`cursor`, `tooltip`, `menu`, `captures_keys`, `frame_deadline`. It implements what it
uses; the rest has defaults. Its owner hands it the input through its `Controls` (or a
host such as `result_pane::PaneHost`) — the owner never re-implements what the part does.
See [interaction.md](interaction.md).

## 5. Its accessibility

It describes itself where it paints (`ui_kit::access`): `group` for a control whose
texts are its name, `node` for one labelled beside it, `open(…)` for a long paint, `silent`
for drawings made of letters; `.index` / `.count` in lists, `.level` in trees, `.lines`
for a field's caret, `.shortcut` for its keys. The description is a closure: nothing is
built while no screen reader runs. Never a secret, never an icon's id as a name.

## 6. Its cost

- **Nothing per frame that could be cached:** what depends on data is made when the data
  changes (a generation, a version, a fingerprint of what it depends on), compared each
  frame without allocating. A component's test "an identical repaint allocates nothing"
  holds for its owner.
- **Nothing heavy on the UI thread:** parsing, preparing, rasterizing go to a pool
  (`kernel::Pool` / `kernel::work`); the result is taken by `poll`, the app woken by the
  view's waker.
- **Exists only while used:** what a hidden part holds (masks, caches) is let go
  (`forget`, `hidden`).

## 7. Its tests (in `tests/unit/<file>.rs`)

Every component has, at least:

- what it shows for its spec (sizes, what's drawn where), at scale 1 and 2;
- a press, a hover, its keys (and that a held press follows a drag to its release);
- its accessibility node (role, name, state);
- for a stateful one: that an identical repaint allocates nothing, and that work it sends
  to a worker comes back by `poll`.
