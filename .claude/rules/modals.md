# Modals and windows over the editor

Part of the project's rules: [index](project-conventions.md). Related:
[UI components](ui-components.md), [modules](modules.md), [performance](performance.md).

- **Never closed by a click outside**: only by ×, Esc or its buttons — a stray click must
  not lose the user's work. While one is open nothing behind it gets keys, text, commands,
  scroll or hover (`Workbench::modal_open`).
- **One stack**: every window over the editor is an `OverlaySlot` (`Overlay::ALL`: drawing
  order = input order); a module brings its own as a view (`host.open_modal` framed,
  `host.open_overlay` bare, modal or popover), never as app code. Popovers keep
  `ds::POPOVER_GAP` from the bottom bar.
- **The footer holds the buttons and the keys**: a modal's actions (OK, Cancel, Save,
  Delete…) go in its bottom bar, on the right (`ds::modal_footer_bar`,
  `ds::footer_buttons`, `ds::MODAL_FOOTER_H`; a view drawing it says
  `Viewer::draws_footer`), and on the left of the same bar the navigation keys it
  answers to (`↑ ↓ move · ↵ open · esc close`: `Viewer::key_hints`, `ds::footer` /
  `ds::footer_in_card`). Never buttons floating in the content, never a modal without
  the keys it takes. The main button is in the accent with its `↵`; what can't be undone
  is in the danger color; a group of its own gets a `|` before it.
- **Work under way can't be left**: while it runs, the close buttons and Esc are off
  (`Viewer::modal_closable` → `false`) and the modal's content becomes the progress (a
  bar, what's being done) — as the closing modal does for the processes ending.
- **A question replaces the content, not another window**: asking before acting
  (`press` → `Question`) shows in the same frame, its answers as footer buttons.
- **The content scrolls with the shared scrollbar** in a lane of its own, with room
  between it and the items (never touching them).
