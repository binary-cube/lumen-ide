# What every part does the same way

## The pointer

- A press goes to the topmost part under it. `Press::Held` keeps the pointer: every move
  goes to that part (`drag`) until it's let go (`release`), wherever the mouse is.
- Hover is told on every move (`hover(Some(p))`) and when the mouse leaves (`None`); a
  part says whether what it shows changed (a redraw only then).
- The cursor comes from the part under the mouse (`Pointer` on what's pressed, `Text` on
  text, `ResizeVertical` / `ResizeHorizontal` on a separator).
- A double click is a press with `clicks == 2` (a chart's: all of X again).

## Keys and focus

- One part has the keyboard (`Controls`' focus); Tab moves it along the parts that take
  it (`focusable`, `in_tab_order`), a part with its own Tab says `keeps_tab`.
- A list or a menu open over a part takes the keys, the text and the commands, whatever
  has the focus (`captures_keys`): ↑ ↓ move, ↵ chooses, Esc closes. What it doesn't use
  goes on.
- Esc closes the innermost thing open (a list, then a find bar, then a modal).

## Tooltips

- A part's `tooltip(p)` gives a `Tip` (its anchor, its text, its keys). It appears after
  the shared delay; a wide anchor follows the mouse (a chart's reading).
- A button showing only an icon always has one (it's also its name to a screen reader).
- Multi-line tips separate lines with `\n`: the first line names, the next tell values.

## Menus

- A right-click asks the part under it (`menu(p)`); rows come back as the view's commands.
- A menu under a view's own button goes through `Viewer::take_menu`. Menus are the app's
  one component: a module never draws one.

## Scrolling

- The wheel goes to the part under the mouse that scrolls (`scroll`), else to the focused
  one; a part at its end can keep the wheel (`takes_wheel`).
- A scrollbar is always the shared one, wired whole: drawn, hovered, its thumb dragged.

## Popovers

- Placed by `ds::popover_rect(anchor, size, view, PopoverPlace { side, right, gap, margin,
  bottom })`: under its anchor when it fits (the bottom bar's room kept), else over it, or
  always over it (a footer's, the status bar's); `ds::POPOVER_GAP` from the anchor; never
  out of the window, however small.
- Its appearing motion (a few pixels toward its anchor, `anim::Appear`) is its own.

## Modals

- Closed only by ×, Esc or their buttons, never by a click outside; nothing behind them
  gets input while they're open; to a screen reader they're a modal dialog.
- Work under way can't be left: closing is off while it runs.

## A chart (the shared `charts::Chart`)

- The mouse over a category: the column lit, a guide down the plot, every line's dot, a
  tooltip with the category and each series' value in its unit.
- A press on the legend hides or shows its series.
- Over a measured X (a number, a time): the wheel zooms around the mouse, a drag moves
  along it, a double click shows all of it.
