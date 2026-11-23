# The UI guide

One way of making what's seen and touched, written down (plan 95): the same look, the same
behavior and the same code wherever it is. Read the page for what you're about to do; the
rules in `.claude/rules/ui-components.md` say *which* component to use, these pages say
*how* one is built and how it behaves.

| Page | What it says |
|---|---|
| [tokens.md](tokens.md) | The visual language: space, corners, sizes, type, colors by role, elevation, motion |
| [anatomy.md](anatomy.md) | How a component is made: its spec, its look, its paint, its interaction, its accessibility, its cost, its tests |
| [interaction.md](interaction.md) | What every part does the same way: focus, keys, the pointer, tooltips, menus, scrolling, modals |
| [recipes.md](recipes.md) | Step by step: a new component, an option for one, a module's view, a pane over data, a chart |
| [components/](components/) | A page per component, by the same template |

## Where things live

```
kernel            geometry, colors, caches, work pools, probes
render-api        Scene / Layer / Quad / ImageData: what's drawn (quads, glyphs, images)
text-layout       fonts, shaping, glyphs
ui-kit            the lowest pieces: text drawn, icons, key caps, the text field, accessibility recording
ui-components     the components every view is built from (ds, controls, lists, grids, charts…)
  └─ ds           the design system's tokens and its small parts (buttons, fields, badges, feedback)
ui-workbench      the app's own screens (only the app depends on it)
modules           features; they build their views from ui-components, never ui-workbench
domain crates     pure models a component draws (charts-model, data-tables, …): no drawing, no theme
```

- **A component knows no module.** What a place needs is an option of the component, its
  default the look every caller has today.
- **A module knows no other module.** Where one offers something over another's view (a
  chart over a result), they meet through a contract in `ui-components` or `contracts`,
  registered by the app (see [recipes.md](recipes.md#a-pane-over-another-modules-data)).
- **Pure work lives in the domain.** Layout, parsing, scales, geometry are functions of
  data, tested without a screen; the component only draws what they computed.
