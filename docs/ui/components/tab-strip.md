# Tab strip (`ui_components::tab_strip`)

**For:** tabs a view keeps of its own: pill tabs (a bottom tool window's views, a
diagram's pages, the Services window) and underlined tabs (a form's sections).

**Where it's used:** the app's bottom tool window, the Services window, the diagram
editor's pages, the HTTP client's editor and environments.

## Anatomy

```
 ( ⚙ Output  × ) ( 🗄 shop.main ) ( 📌 Pinned )        ← PillTab: icon · label · slot
 ───────────────────────────────────────────────       ← strip_border

   Headers   Body   Auth                               ← UnderlineTab
   ‾‾‾‾‾‾‾                                             ← accent line under the one in force
```

- `PillTab`: a spec; `pill_tab` draws it from `x`, centered on `mid`, and returns
  `PillTabRects { pill, close }`. Its owner keeps which tab is which and what a click does.
- `UnderlineTab`: a spec; `underline_tab` draws it in a bar and returns its area.
- `strip_border`, `pill`, `font`, `TAB_H` (24), `TAB_GAP` (4).

## Options

| Option | Default | Effect |
|---|---|---|
| `PillTab::new(label)` | no icon, no slot | — |
| `.icon(name)` | none | an icon before the label |
| `.slot(TabSlot::Close { shown, lit })` | `None` | room for a × (shown on the tab in force or under the mouse) |
| `.slot(TabSlot::Pin)` | — | a pin: no stray click closes it |
| `.on(on, lit)` | off | in force; `lit`: its strip is the current one (accent, else grey) |
| `.hovered(true)` | off | lit under the mouse |
| `UnderlineTab::new(label).on(true)` | off | the one in force: text color over an accent line |
| `.pad(p)` | 10 | room each side of its label (a narrow bar's tabs closer) |

## States and interaction

Drawing only: its owner hit-tests `pill` / `close`, keeps the hover, and does the press
(choose, close, drag a tab).

## Accessibility

Each pill or underlined tab is a `Tab` named by its label, `selected` while in force; its
label, drawn in it, isn't read again.

## Cost

A pill's width is measured by shaping its label; nothing kept.

## Example

```rust
let pill = PillTab::new(&tab.title).icon(tab.icon).slot(slot).on(on, true).hovered(hovered);
let drawn = tab_strip::pill_tab(layer, text, icons, ds, (x, mid), &pill);
self.tab_hits.push((drawn.pill, TabHit::Tab(i)));
x = drawn.pill.max_x() + px(tab_strip::TAB_GAP);
```

## Don't

- Draw a tab as a rounded quad and a label: these, so every strip looks alike.
- Find a tab by its title: by its key or its id.
