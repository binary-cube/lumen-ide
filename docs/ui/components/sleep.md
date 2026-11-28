# Sleep (`ui_components::sleep`)

**For:** a heavy view (an agent's session in a terminal, a diagram's page) that can be put to
sleep when left alone: its module stops what it holds, keeps its tab and its state, and
brings it back as it was when the user acts on it. Plan 101.

**Where it's used:** the terminal's sessions (`terminal`, kind `agent-session`). The timer,
the requests and the commands are the `sleep` module's (`modules/tools/sleep`), which names no
view.

## Who does what

| Part | Where | Does |
|---|---|---|
| The rule | `sleep` (domain) | `awake_because(facts, idle, now)`, `can_sleep(facts)`, `order_holds` |
| A view's part | `SleepPart` | its slot, registered by key; orders taken at its poll; menu rows, the moon, the card |
| The timer | `sleep` module | looks once a minute, orders what was left alone, logs why the rest stays awake |
| The view's module | its own | what keeps it awake (`Veto`), how quiet it is (`Quiet`), stopping and coming back |

## Using it in a view

1. Declare the kind once: `static KIND: SleepKind { id, viewer, hint, texts }` — its words are
   its own i18n keys (`SleepTexts`: "Session asleep", "Put to Sleep"…).
2. Tell its idle time from the module's settings, at startup and at each change:
   `sleep::configure(&KIND, Some(idle))` (`None`: off).
3. In the view: `SleepPart::new(&KIND, key, project, SleepPlace::Editor { .. })` when it
   knows its key; `started(quiet)` when what it runs starts; `touch()` in its input handlers
   (a key, text, a paste, a press, the wheel, a command, `set_active(true)`); never on hover,
   frames or `set_shown`.
4. Its reasons: `set_veto(Some(veto))` when they change (`by_hand`: holds against the user's
   order too; `never`: its row hidden).
5. Its poll: `take_wake()` → come back; `take_order()` → stop what it holds, then
   `fell_asleep(why)`; coming back: `waking(true)`, settled: `waking(false)`.
6. Its `Viewer`: `wants_poll` → `order_waits()`, `tab_menu` → `tab_menu()`, `tab_icon` →
   `tab_icon()`, `command(SLEEP_ACTION)` → `sleep_by_hand()` (refused: `refusal(can)` as a
   notification), paint the card with `paint_card(layer, …, area, resuming)`.

## The card

```
        ┌──────────────────────────────┐
        │            ( ☾ )             │   the moon in a soft accent circle
        │       Session asleep         │   its kind's title (resuming: its "coming back")
        │ by hand · about 412 MB freed │   why, and what it held
        │        [ Wake Up  ↵ ]        │   primary button (none while resuming)
        │  or press any key or click   │
        └──────────────────────────────┘
```

Opaque (`popup_background`), centered in the area given, `Elevation::Medium`, told to screen
readers as a group with its button. Under it, the view's last screen: a grid of text redrawn
(the terminal), or a picture (`paint_picture`: at its own size in pixels from the top left —
never stretched or shrunk, cut by a smaller area, the background beyond it — then dimmed with
`SCRIM_ALPHA`). A page's picture comes from the app (`WebPage::freeze` → `WebEvent::Picture`).

## Rules

- Asleep until the user wakes it (a key, a click, "Wake Up"): moving onto its tab doesn't.
- An idle order is dropped if the view was touched since; any order is dropped after
  `ORDER_FRESH` (20 s).
- States are published once per change (`SleepStateChanged`, the project's bus) and when the
  view goes (`None`).
