//! `lumen --snapshot <output.png> [paths...]`: renders the first screen with the software
//! driver into a PNG, without a window (visual checks, CI).
//!
//! What the screen shows is set with `EDITOR_SNAPSHOT_*` environment variables, read once
//! into [`Options`]; each one is a small, named step in [`Steps`] (in a fixed order):
//!
//! | Variable | Effect |
//! |---|---|
//! | `THEME=<id>` | renders with that theme (e.g. `default-light`) |
//! | `THEMED=<family>` | renders with the family's variant of the theme's appearance (`christmas` → `christmas-dark` / `christmas-light`); the theme's decorations are always a still frame |
//! | `MY_DECORATIONS=<preset>` | the user's own decorations, a preset's (`autumn`) or one effect's (`matrix`, `aurora@panels`: shown behind the panels too; `synthwave:city@panels`: in a style of its own), drawn over the theme's (a still frame) |
//! | `STUDIO=1` | the theme studio is open (`STUDIO_SCROLL=<px>`: its list scrolled down; `STUDIO_COMMAND=<a>,<b>`: its commands run — `preset:autumn`, `category:effects` (or `scroll-to:<category or section>`), `search:<text>` (`!`: its first result shown); `STUDIO_LIVE=1`: its work-in-progress theme applied to the window, as the app does) |
//! | `SETTINGS=<n>` | settings are open, `n` pages down; `SETTINGS_QUERY=<text>` searches (`SETTINGS_OPEN=1`: its first result opened); `SETTINGS_PAGE=<id>`: that page; `SETTINGS_LANGUAGE=<id>`: the Languages page's language; `SETTINGS_ADVANCED=1`: every "Advanced" open |
//! | `SIZE=<w>x<h>` | the window's size in logical pixels (default 1200x700) |
//! | `SHORTCUT=<command>` | in Settings → Shortcuts, that command opened (`SHORTCUT_KEYS=<keys>`: those keys recorded for it) |
//! | `FOLD=1` | all regions collapsed |
//! | `FIND=<text>` | the find bar is open with the text |
//! | `FILES=<text>` | the project panel's file search has the text |
//! | `TERMINAL=<command>` | the terminal is open and runs the command (`BOTTOM=half\|full`: the bottom window that tall, as its buttons make it) |
//! | `SESSION_TAB=<title>` | a program run for a module (as an agent's session) in a terminal tab of the editor, a few lines of made-up output on its screen (`VIEW_COMMAND=snapshot:asleep`: then put to sleep by hand, its card saying it freed 412 MB); the hint it asks for closed unless `HINT` says |
//! | `HINT=<id>` | after the window's other steps, the contributed hint shown by its anchor (`terminal.sleep`, by the active tab; `HINT_FOCUS=1`: given the keyboard, its focus ring on the main button) |
//! | `TYPE=<text>` | the text is typed at the end of line `LINE` (default: the last), after indexing (`\\n` Enter, `{sc}` a semicolon) |
//! | `SEARCH=<text>` | "Search in Project" is open with the text (`⇥`: Tab, to the folder's field: `Order⇥src/`) |
//! | `SELECT=<text>` | the first occurrence is selected |
//! | `COLOR=<line>` | the color picker is open on that line's swatch (0-based) |
//! | `SPLIT=right,down,move-right,move-down` | editor splits, in order |
//! | `CANVAS=1\|2\|3` | the canvas (functions / files / full code); `CANVAS_EDIT`, `CANVAS_QUERY` |
//! | `TABDRAG=x1,y1,x2,y2[!]` | a tab dragged (and dropped with `!`) |
//! | `TREEDRAG=x1,y1,x2,y2[!]` | the active file revealed in the tree, a tree entry dragged (dropped with `!`) |
//! | `COMPARE=left,right[,merge]` | the compare view (merge mode) |
//! | `PIN=<dx>` | the first tab pinned, the tab bar scrolled by `dx` |
//! | `LOCK=1` | the active tab locked (read-only): the padlock closed in the status bar, on the tab |
//! | `ZOOM=<factor>` | the editor zoomed |
//! | `LEVEL=1` | the language level picker |
//! | `ABOUT=1` | the About window |
//! | `ENDING=some\|many` | The app ending: what it started being stopped, made up (`many`: more than the list shows) |
//! | `UPDATES=notes\|long` | What's New of a made-up version found (nothing downloaded): its notes short, or `long`, more than the window holds (its scrollbar; `HOVER` over it widens it) |
//! | `COMMAND=<id>,<id>…` | those commands run, in order, after the window's other steps (`focus.toggle`: Focus mode on; with `HOVER`, the region under the mouse in color) |
//! | `PROJECT_WINDOWS=<name>,<name>…` | other windows open with these projects (made up; the first is this window's), numbered 1… as the app does (`COMMAND=projects.switcher`: their list) |
//! | `VIEW_COMMAND=<id>,<id>…` | then those sent to the view in front (a modal's or an overlay's first): a module's own snapshot steps (`snapshot:dimmed`: Keep awake's screen as after its fade) |
//! | `FILEDROP=<path>,<path>…` | those files / folders (relative: in the project) dragged from the file manager over the window, at `HOVER` (else its middle): the drop overlay |
//! | `COPY=1` | the "Copy" popover for the active file |
//! | `MENU=1` | the tab's context menu |
//! | `GIT=head\|history\|branch\|blame\|annotated\|commit\|log\|diff\|changes\|menu\|file-history…` | compared with the last commit / the version lists; `file-history-scrolled` — the active file's history in the Git window, its newest commit chosen, every file of it shown and the details scrolled to their end; `commit-message` — the commit view with a message of several lines; `new-branch` / `new-tag` — those dialogs, a name typed; `file-history-commit-menu` / `-file-menu` — a commit's / a file's right-click menu; `log-filter:<command>,…` — the log, its view's commands run (`filter-text:<text>`: the text filter; `filter:branch\|user\|date\|paths\|order`: that filter's list open) |
//! | `RENAME=<new name>` | the rename preview for the symbol at the cursor (`SELECT`), the name typed |
//! | `MOVE=<folder>[,<typed>][!]` | the Move preview: the active file into the project's folder, the field retyped (applied with `!`) |
//! | `USAGES=1` | Find Usages for the symbol at the cursor (`SELECT`) |
//! | `DIAGRAM=1` | the diagram of the type at the cursor (`SELECT`) |
//! | `HISTORY=<dir>` | the active file's local history, its versions read from `<dir>` |
//! | `CLIPBOARD=<text>` | Compare with Clipboard, this text on the clipboard (`\n`: a new line) |
//! | `SIGNATURE=1` | the Change Signature preview for the function at the cursor (`SELECT`) |
//! | `PANEL=structure` | the side panel is open |
//! | `DIALOG=1\|rename\|delete` | the "Save changes?" (or rename) dialog; `delete`: a recent project's, its checkbox checked |
//! | `NEW=<name>` | a new-file row in the tree |
//! | `PROJECTS=1\|groups\|menu\|drag` | the projects menu; `groups`: with a sample list in groups (nothing kept is read or written); `menu`: and a project's right-click menu; `drag`: a project dragged above another |
//! | `SETTINGS_SCROLL=<px>` | the settings' content scrolled by the wheel, in steps (after it was painted) |
//! | `BACKDROP=<style>[:<blur>[:<tint>]]` | the backdrop under the windows over the editor chosen over the theme's: `simple` or `glass` (`theme`: the theme's), its blur in logical pixels, its tint in percent (`glass:6:20`) |
//! | `MENU_DRAWER=closed\|open\|keys:<keys>\|filter:<text>` | the window's bar with the menu button (as the macOS setting turns it on; always there on Windows and Linux): `closed` — only the button; `open` — the menu drawer open; `keys:` — and those keys pressed in it (`u` ↑, `d` ↓, `l` ←, `r` →, into a submenu or a row's own submenu: `ddrdd` shows File's submenu, its third row chosen; `ddddrr` View › Tool Windows as the third panel); `filter:` — the text typed in its filter |
//! | `WINDOW_CONTROLS=1\|max\|full\|full-shown` | the window's own buttons in the top bar (as on Windows, its corners cut; `max`: maximized; `full`: full screen, the bar hidden; `full-shown`: come down over the content), the mouse at `HOVER` |
//! | `MEMORY=1` | prints the memory once everything settled (measurements) |
//! | `PARTIAL=1` | checks the partial redraw: the picture drawn over an earlier frame (only where it changed) must equal the one drawn whole, or the snapshot fails |
//! | `TASKS=1\|wide\|split\|compact\|minimal\|page\|columns\|all` | the Task Manager's tab: `1` after two looks (a second of CPU measured); the others made up, the same every run (fixed parts, programs, operations and history, its clock stopped) — `wide`: the tab alone (the project's tree closed), a terminal selected with its programs and its detail; `split` / `page`: the same in a split; `compact` / `minimal`: a narrow split; `columns`: the columns' chooser open; `all`: every project's view grouped by project; `menu`: a row's right-click menu; `stop`: stopping a formatter asked first (the module's modal over the window) |
//! | `SPLIT_AT=<px>` | the first split's separator at this many logical pixels from the window's left edge |
//! | `REMOTE=1\|2\|3` | the remote project wizard at that step, against the demo SFTP server (`127.0.0.1:2222`, `demo`; run with `HOME` in a scratch folder: trusting it writes there) |
//! | `REMOTE_PROPS=file\|several\|folder\|list\|confirm\|failed\|checksum\|algos` | a remote file's "Properties…" (made up: no server): one file, several mixed (a folder among them), a folder measured, the groups' list open, a recursive change asked, what the server refused, the Checksum tab (its algorithms' list open) |
//! | `AGENT_CHAT=1` | an agent's session as a conversation (made up: no agent): messages, thinking, a plan, tools (a change to compare), a permission waited for |
//! | `REMOTE_CHANGED=1\|search\|scrolled[<px>]` | a remote project's "Changed" tab (made up: no server): files over the last days on their timeline; `search`: a search typed; `scrolled`: more files, scrolled by 900 physical pixels (or as given): a day pinned at the top |
//! | `REMOTE_EDIT=1\|invalid` | a remote project's connection edited (made up: no server, no secret): its form prefilled; `invalid`: a port out of range tried (the error inline) |
//! | `LINK=x,y` | ⌘ held over a point (link to a declaration) |
//! | `HOVER=x,y` | the mouse over a point (physical pixels) |
//! | `HOVER_WORD=<text>` | the mouse over the first place of the text in the active file (its card) |
//! | `SPLASH=1` | the splash screen |
//! | `DATABASE=tree\|databases\|mysql\|add\|menu\|folders\|folder-grid\|folder-grid-more\|folder-grid-tip\|sources\|sources-ssl\|sources-schemas\|sources-ok\|sources-failed\|sources-folders\|sources-add` | the "Database" window with the sample SQLite database and CSV file as sources (kept only in memory), the database opened down to a table's columns; `databases`: a PostgreSQL-like server's source naming its empty `postgres` database too (a made-up driver in memory), its databases listed under it — its own marked —, `shop` opened down to its `sales` tables; `mysql`: a MySQL-like server's source (made up, in memory) naming `shop`, its databases listed as its schemas (`analytics`, `shop`: one session reads them all), `shop` opened down to its tables; `add`: its `+` menu open on Data Source; `menu`: a table's right-click menu; `folders`: the sources in folders (prod in red with a subfolder, dev in green); `folder-grid`: those folders, the red folder's database opened in a tab (its color on the tab; `-more`: its grid's "…" menu open; `-tip`: the tooltip of its Delete button); `sources…`: the data sources' modal with PostgreSQL sources too, a server's General tab (its password typed, masked), its SSH/SSL tab, its schemas, a test's success or failure (shown, nothing connects); `sources-folders`: the modal with the folders, the prod folder selected; `sources-add`: its `+`'s drivers open |
//! | `RELATIONS=all\|focus\|inferred\|decided\|edge` | a sample store's SQLite database (made in a temporary folder) opened as data, its relations diagram shown from the tables' list: every table, `orders` and its neighbors, the relations found from names with their list open, one accepted and one set aside (kept in memory only: the file isn't a project's), the popover of a line found from names (`reviews.userid`) |
//! | `TOASTS=all\|modal` | notifications shown, their time stopped part way: `all` — an information, a warning with two actions, an error with its detail, work in progress; `modal` — over a modal (with `DATABASE=sources`), what Apply tells: saved (twice, merged) and where a password went |
//! | `CONSOLE=run\|jump\|jump-filter\|history\|history-clear\|output\|result\|results-many\|error\|tx-manual\|danger\|prod-confirm\|menu\|params\|plan\|profile\|profile-narrow\|profile-tab\|profile-finding\|profile-graph\|profile-flame\|profile-compare\|profile-tables\|profile-sqlite\|completion\|signature\|join\|hover\|unknown-table\|file\|file-attached\|result-extractors` | a query console of the sample SQLite database (kept as a source in memory, its console's file in the project's own folder, its clock stopped): `run` — a statement run, its mark in the gutter and what it did in the bar, its result in the Services window; `jump` — "Jump to Query Console…" with two consoles of the source, its list shown; `output` — a script run, its Output shown (timestamps, each statement, rows retrieved with execution and fetching); `result` — a table's rows in the Services window (`main.customers`); `results-many` — a script's results as tabs, the first pinned; `error` — a statement the engine refuses: underlined in the console, red in the Output; `tx-manual` — two writes in Tx: Manual waiting for Commit (the pill's count, Commit / Rollback, `TX` on the tab and in the Services tree); `danger` — a `DELETE` without `WHERE` and a `DROP` confirmed before they're sent; `prod-confirm` — a write on a production source in Tx: Auto, "I understand" asked first; `menu` — its right-click menu (its own rows first, Comment, the clipboard), nothing run; `params` — a statement's parameters asked before it runs (values typed, an expression, one not remembered, the one edited lit in the statement); `plan` — Explain Plan of a query with a CTE and a subquery, its profile in the Services window; `profile` / `profile-narrow` — a PostgreSQL profile made by hand (parallel, a spill, findings) in the Services window, wide and narrow; `profile-tab` — the same in an editor tab of its own; `profile-finding` — its first problem chosen, its place lit in the console; `profile-graph` — the same as a diagram, its first problem chosen; `profile-flame` — as a flame graph, its hottest step chosen; `profile-compare` — the same query after an index, compared with it before (the steps aligned, their deltas, the findings resolved and new); `profile-tables` — its tables with the server's statistics of them (made by hand), the first problem chosen; `profile-sqlite` — Explain Analyze measured on the sample database (the engine's times frozen); `history` — a console's history window, searched for `orders` (what matched lit, the statement chosen whole); `history-clear` — the same window asking whether to clear the whole history (in its footer, "Cancel" ringed); `jump-filter` — the source's consoles' popover filtered by `ord` (names and texts matched, the line that matched under the name); `signature` — `substr(name,` typed: the function's parameter help, its second parameter lit; `completion` — completion after `FROM` from the source's schema (the tables with their schema and source, as a list); `join` — completion after `JOIN` (the related table with its whole `ON` condition first); `hover` — a table's hover card from the schema (its columns, types, keys and what they point to); `unknown-table` — a text checked against the schema: an unknown table underlined with its message, the CTE beside it not; `file` — a SQL file of the project attached to nothing ("No data source — Attach…"); `file-attached` — the same file attached to the source, a statement run (the source beside the schema, its result in the Services window); `result-extractors` — a table's rows in the Services window, its grid's data extractors' popover open |
//! | `EXTRACTOR=template` | the data extractors' templates: "Go to Extractors Directory" makes the project's folder with its five examples (removed by the script after), `Python-DataFrame.extractor` open — its `{{ }}` tags, sections and filters colored |
//! | `LSP_PHP=missing` | the settings' language servers block shows PHP, intelephense not installed (`installed`: installed in a temporary folder, a licence file found; `licensed`: a key kept, in memory) |
//! | `CURSOR=text` | the cursor right after the first occurrence of `text` |
//! | `RELATED=implementations\|hierarchy\|actions\|definition` | "Go to Implementation" / "Type Hierarchy" / "Show Context Actions" / "Go to Definition" at the cursor: its list (a definition's, when it has several places) |
//! | `INSERT=text` | `text` put at the end of line `LINE` as one edit (`\\n` a line break, `{sc}` a semicolon): the inlay hints and lenses waited for |
//! | `ENGINES=1` | "Choose the File's Language Engine…" (the status bar's engine segment): its list |
//! | `LSP_OFFER=offer\|downloading\|parts` | the language servers downloaded on demand, in a folder of the temporary folder from files in memory (never the network nor the user's data folder): `offer` — the bar over a TypeScript file whose server isn't downloaded (vtsls + Node.js, their size); `downloading` — the same while Node downloads (stalled part way: its progress, Stop); `parts` — Node, PHP, phpactor and vtsls downloaded (their markers only), the settings' list of the downloaded servers. Without it nothing is downloadable (no bar) |
//! | `LSP=completion` | a TypeScript file (`src/cart.ts`, made for it: the script removes it) answered by the fake language server (this binary's `--fake-language-server`, a process): its diagnostics (a `TODO`'s warning, a `FIXME`'s error) and its completion after `console.` |
//! | `LSP=cpp` | a C++ file (`src/shapes.cpp`) answered by the fake server: with `PANEL=structure`, the Structure panel from its declarations (C++ has no structure of ours) |
//! | `LSP=usages` | the TypeScript file, with a page calling its function (`src/cart.html`): with `USAGES=1`, Find Usages joining the server's references and the built-in engine's in the page |
//! | `LSP=css` | the same for a CSS file (`src/card.css`), the fake described as the CSS server is: its completion of `col`, its `TODO` warning and the built-in engine's deprecated `clip` kept beside them |
//! | `TOAST_POSITION=bottom-right\|bottom-center\|top-center\|top-right` | where the notifications show (as the setting) |
//! | `DATA=sqlite\|structure\|query\|find\|transpose\|menu\|csv\|split\|split-keys\|edit\|preview\|json\|where-error\|extractors\|export\|exported\|create-table\|create-table-types\|alter-column\|drop-column\|create-index` | a sample SQLite database (or CSV file) made in a temporary folder and opened as data: its first table, its structure, a table filtered, sorted and a column selected, its rows searched (Find in the Rows), a row transposed, a cell's menu, the CSV alone or beside its text (its grid clicked and moved in by keys), changes pending, their preview before they're submitted, a JSON value in the large editor, a `WHERE` not understood, the grid's data extractors' popover open, every row being exported and once written, the structure's modals (a new table — `-types`: its new column's type suggestions open —, a column changed, a column dropped with what it loses, a partial index) |

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use kernel::Point;
use render_api::{PhysicalSize, Renderer, Scene};
use render_software::SoftwareRenderer;
use text_layout::{FontFamily, TextSystem};

use crate::resources;
use crate::split_tree::Split;
use crate::themes::Themes;
use crate::workbench::{EditKey, FoldCommand, Workbench};
use indexing::semantic;

use self::fixtures::*;
use self::options::*;
use self::setup::*;

pub use self::capture::render;
pub use self::setup::{before_open, prepare};

mod actions;
mod capture;
mod cluster;
mod console_steps;
mod data_fixtures;
mod data_steps;
mod fixtures;
mod git_steps;
mod mysql_like;
mod options;
mod overlay_steps;
mod setup;

/// The snapshot size: 1200x700 logical pixels at 2x density (`EDITOR_SNAPSHOT_SIZE=WxH`:
/// another, in logical pixels — a narrow window).
static SIZE: std::sync::LazyLock<PhysicalSize> = std::sync::LazyLock::new(|| {
    var("SIZE")
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some(PhysicalSize::new(w.parse::<u32>().ok()? * 2, h.parse::<u32>().ok()? * 2))
        })
        .unwrap_or(PhysicalSize::new(2400, 1400))
});
/// Where `TASKS=menu` right-clicks (physical pixels): the terminal's row of the made-up list.
const TASKS_MENU_AT: (f32, f32) = (360.0, 986.0);

/// Everything the `EDITOR_SNAPSHOT_*` variables ask for, read once.
#[derive(Debug, Default)]
pub struct Options {
    theme: Option<String>,
    /// A family of themes (`christmas`): its variant of the theme's appearance
    /// (`christmas-dark` with a dark theme, `christmas-light` with a light one).
    themed: Option<String>,
    /// The user's own decorations: a preset's id.
    my_decorations: Option<String>,
    studio: bool,
    studio_scroll: Option<f32>,
    /// The studio's commands, run in order once it's drawn.
    studio_command: Option<String>,
    /// The studio's work-in-progress theme applied to the window, as the app does.
    studio_live: bool,
    /// `Some(steps)`: settings open, moved `steps` sections down.
    settings: Option<isize>,
    settings_query: Option<String>,
    /// The settings' page opened by its id (`SETTINGS_PAGE`), the Languages page's language
    /// (`SETTINGS_LANGUAGE`), every "Advanced" opened (`SETTINGS_ADVANCED=1`).
    settings_page: Option<String>,
    settings_language: Option<String>,
    settings_advanced: bool,
    shortcut: Option<String>,
    shortcut_keys: Option<String>,
    fold: bool,
    find: Option<String>,
    files: Option<String>,
    terminal: Option<String>,
    /// `SESSION_TAB`: a keyed terminal tab in the editor, by its title.
    session_tab: Option<String>,
    /// `HINT`: a hint shown at the end; `HINT_FOCUS`: with the keyboard.
    hint: Option<String>,
    hint_focus: bool,
    /// The terminal's first shell split (`right` / `down`: another shell beside / below).
    terminal_split: Option<String>,
    /// The bottom window's size: `half`, `full`.
    bottom: Option<String>,
    typed: Option<String>,
    line: Option<usize>,
    search: Option<String>,
    select: Option<String>,
    color: Option<u32>,
    /// (direction, move the tab instead of copying it).
    splits: Vec<(Split, bool)>,
    /// The canvas mode key (`1`, `2`, `3`).
    canvas: Option<char>,
    canvas_edit: Option<String>,
    canvas_query: Option<String>,
    tab_drag: Option<TabDrag>,
    tree_drag: Option<TabDrag>,
    compare: Option<(PathBuf, PathBuf, bool)>,
    pin: Option<f32>,
    lock: bool,
    zoom: Option<f32>,
    level: bool,
    about: bool,
    /// `UPDATES`: What's New of a made-up version (`long`: notes that scroll).
    updates: Option<String>,
    /// `ENDING`: the app ending, what it started being stopped (`many`: a list that scrolls).
    ending: Option<String>,
    /// `RESOURCES=1`: the bar's CPU, memory and GPU, made-up values (no sampling here).
    resources: bool,
    /// `STATUS_MORE=1`: the list of the bar's items that didn't fit (its `…` clicked).
    status_more: bool,
    /// `STATUS_POPOVER=file`: a bar item's window open (`file`: the file's details).
    status_popover: Option<String>,
    commands: Vec<String>,
    view_commands: Vec<String>,
    project_windows: Vec<String>,
    /// Items dragged from the file manager over the window.
    file_drop: Option<Vec<PathBuf>>,
    copy_menu: bool,
    /// The tab's context menu (our own), at the first tab.
    menu: bool,
    /// `head`: compared with the last commit; `history` / `branch`: the version lists.
    git: Option<String>,
    /// The rename preview, with this name typed (empty: the old name).
    rename: Option<String>,
    /// The Move preview: the active file into this folder (relative), the field retyped.
    moving: Option<String>,
    signature: bool,
    usages: bool,
    clipboard: Option<String>,
    diagram: bool,
    history: Option<PathBuf>,
    panel: Option<String>,
    /// `Some(true)`: the dialog has a text field (rename).
    dialog: Option<String>,
    new_entry: Option<String>,
    projects: Option<String>,
    remote: Option<u8>,
    remote_edit: Option<String>,
    remote_props: Option<String>,
    remote_changed: Option<String>,
    agent_chat: bool,
    /// `TASKS`: the Task Manager's tab (`1`: measured; else a made-up scenario).
    tasks: Option<String>,
    /// `SPLIT_AT`: the first split's separator at this many logical pixels from the left.
    split_at: Option<f32>,
    window_controls: Option<String>,
    menu_drawer: Option<String>,
    settings_open: bool,
    link: Option<Point>,
    hover: Option<Point>,
    /// `HOVER_WORD`: the mouse over the first place of this text in the active file.
    hover_word: Option<String>,
    splash: bool,
    /// A sample data file opened as data (`sqlite`, `structure`, `query`, `transpose`,
    /// `menu`, `csv`, `split`).
    data: Option<String>,
    /// The "Database" window (`tree`, `add`, `menu`).
    database: Option<String>,
    /// `RELATIONS`: the relations diagram of a sample database.
    relations: Option<String>,
    /// `TOASTS`: notifications shown (`all`, `modal`).
    toasts: Option<String>,
    /// `TOAST_POSITION`: where they show.
    toast_position: Option<String>,
    /// `SERVICES`: the Services window (`empty`: nothing contributed; `fake`: a sample
    /// contributor's tree, a console's Output shown).
    services: Option<String>,
    /// `CONSOLE`: a query console (`run`: a statement run; `jump`: the source's consoles).
    console: Option<String>,
    /// `LSP`: a TypeScript file answered by the fake language server (`completion`).
    lsp: Option<String>,
    /// `INSERT`: this text put at the end of line `LINE` as one edit (`\n` a line break,
    /// `{sc}` a semicolon — the cases' separator), nothing typed.
    insert: Option<String>,
    /// `ENGINES`: "Choose the File's Language Engine…" run (the status bar's engine segment).
    engines: bool,
    /// `CURSOR`: the cursor put right after the first occurrence of this text.
    cursor: Option<String>,
    /// `RELATED`: "Go to Implementation" (`implementations`), "Type Hierarchy"
    /// (`hierarchy`), "Show Context Actions" (`actions`) or "Go to Definition"
    /// (`definition`) run at the cursor, its list shown.
    related: Option<String>,
    /// `TYPING`: this many keys typed at the end of the active file, the time each costs
    /// the UI thread printed (measurements; `REAL_LSP=1`: the bundled servers answer).
    typing: Option<usize>,
    /// `IDLE`: the servers stop after this many seconds without documents; the tabs are
    /// closed after the other steps and the process stays this long and 25 s more
    /// (measurements: a server's idle stop, the memory given back).
    idle: Option<u64>,
    /// `SOAK`: after the commands, this many seconds of frames (30 a second, as a screen
    /// left on), the process's memory and the heap's live bytes printed every 10 s
    /// (measurements: what grows while nothing is done).
    soak: Option<u64>,
    /// `EXTRACTOR`: the data extractors' templates (`template`: their folder made with the
    /// examples, one of them open).
    extractor: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct TabDrag {
    from: Point,
    to: Point,
    drop: bool,
}

/// The steps that bring the workbench to the requested state.
struct Steps<'a> {
    wb: &'a mut Workbench,
    text: &'a mut TextSystem,
}

impl Steps<'_> {
    /// Every step asked for, in a fixed order (it decides what the snapshot shows).
    fn run(&mut self, o: &Options) {
        // The committed texts (git) are read in the background: the gutter bars need them;
        // the modules' counts (the changed files on the Git button) too.
        self.wait(Duration::from_secs(5), Workbench::decorations_settled);
        self.wait(Duration::from_secs(5), Workbench::services_settled);
        self.editor_steps(o);
        // The inlay hints and code lenses worked out (in the background, after a pause).
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            self.frame();
            self.wb.poll_servers();
            if self.wb.insight_settled() || Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // What the services check in the documents announced by those frames (a `.env`
        // module's undefined variables).
        self.wait(Duration::from_secs(5), Workbench::services_settled);
        self.wb.poll_diagnostics();
        self.frame();
        self.view_steps(o);
        self.window_steps(o);
        // The active file's analysis (level issues, deprecated names) from its thread.
        self.wb.sync_syntax();
        self.frame();
        self.wait(Duration::from_millis(1500), Workbench::poll_symbols);
        self.frame();
        if let Some(position) = &o.toast_position {
            match workbench_model::ToastPosition::parse(position) {
                Some(position) => crate::toasts::set_position(position),
                None => tracing::error!("no notification position `{position}`"),
            }
        }
        if let Some(kind) = o.toasts.as_deref() {
            self.toasts(kind);
        }
        if !o.project_windows.is_empty() {
            self.project_windows(&o.project_windows);
        }
        for command in &o.commands {
            let id = command_model::CommandId(Box::leak(command.clone().into_boxed_str()));
            // The app's own lists of the file's format (the app runs them, not a module).
            let own = match id {
                crate::commands::CHOOSE_LANGUAGE => Some(self.wb.choose_language()),
                crate::commands::CHOOSE_ENCODING => Some(self.wb.choose_encoding()),
                crate::commands::CHOOSE_LINE_ENDING => Some(self.wb.choose_line_ending()),
                crate::commands::CHOOSE_INDENT => Some(self.wb.choose_indent()),
                _ => None,
            };
            if own.is_none() && self.wb.run_command(id).is_none() {
                tracing::error!("no command `{command}`");
            }
            self.frame();
        }
        for command in &o.view_commands {
            // What the views make in the background first (a diagram laid out).
            self.wait_for_viewers(Duration::from_secs(10));
            if !self.wb.viewer_command(command) {
                tracing::error!("the view didn't take `{command}`");
            }
            // What it asked for (a window opened) taken, as the app does after a key.
            self.wb.take_viewer_action(self.text);
            self.frame();
            settle(200);
            self.frame();
        }
        if let Some(id) = &o.hint {
            self.hint(id, o.hint_focus);
        }
        if let Some(seconds) = o.soak {
            self.soak(seconds);
        }
        if let Some(p) = o.hover {
            self.hover(p);
        }
        if let Some(word) = &o.hover_word {
            self.hover_word(word);
        }
        if let Some(paths) = &o.file_drop {
            let root = self.wb.project_root().unwrap_or_default();
            let paths: Vec<PathBuf> = paths.iter().map(|p| root.join(p)).collect();
            let (w, h) = (SIZE.width as f32, SIZE.height as f32);
            let at = o.hover.unwrap_or(Point::new(w / 2.0, h / 2.0));
            self.wb.drag_files_over(&paths, at);
            self.frame();
        }
    }

    /// The editor's own state: folds, find, typing, selections, splits, tabs, zoom.
    fn editor_steps(&mut self, o: &Options) {
        if let Some(case) = &o.lsp {
            self.language_server(case);
        }
        if let Some(keys) = o.typing {
            self.typing(keys);
        }
        if let Some(seconds) = o.idle {
            self.idle(seconds);
        }
        if o.fold {
            self.wb.sync_syntax();
            self.wb.fold(FoldCommand::FoldAll);
        }
        if let Some(query) = &o.find {
            self.wb.open_find(self.text, true);
            self.wb.type_text(self.text, query);
        }
        if let Some(query) = &o.files {
            self.file_search(query);
        }
        if let Some(command) = &o.terminal {
            self.terminal(command);
        }
        if let Some(title) = &o.session_tab {
            self.session_tab(title, o.hint.is_some());
        }
        if let Some(split) = &o.terminal_split {
            use crate::commands::ToolTabAction as A;
            let action = if split == "down" { A::SplitDown } else { A::SplitRight };
            self.wb.tool_tab_action(action, 0);
            self.frame();
            settle(300);
            self.wb.poll_viewers();
        }
        if let Some(size) = &o.bottom {
            self.wb.bottom_size_for_snapshot(size == "full");
            self.frame();
        }
        if let Some(typed) = &o.typed {
            self.type_at_line(typed, o.line);
            // The changed lines (gutter bars) are found in the background.
            self.wait(Duration::from_secs(5), Workbench::decorations_settled);
        }
        if let Some(inserted) = &o.insert {
            self.wait_for_indexing(Duration::from_secs(20));
            self.wb.place_cursor_at_line_end(self.text, o.line);
            self.wb.insert_at_cursor(self.text, inserted);
        }
        if let Some(query) = &o.search {
            self.project_search(query);
        }
        if let Some(needle) = &o.select {
            self.frame();
            self.wb.select_first(self.text, needle);
        }
        if let Some(line) = o.color {
            self.color_picker(line);
        }
        if !o.splits.is_empty() {
            self.split(&o.splits);
            if let Some(at) = o.split_at {
                self.wb.place_first_divider(at);
                self.frame();
            }
        }
        if let Some(mode) = o.canvas {
            self.canvas(mode, o);
        }
        if let Some(drag) = o.tree_drag {
            self.tree_drag(drag);
        }
        if let Some(drag) = o.tab_drag {
            self.tab_drag(drag);
        }
        if let Some((left, right, merge)) = &o.compare {
            self.compare(left, right, *merge);
        }
        if let Some(dx) = o.pin {
            self.pin(dx);
        }
        if o.lock {
            self.wb.toggle_active_lock();
            self.frame();
        }
        if let Some(factor) = o.zoom {
            self.frame();
            self.wb.zoom_by(factor, Point::new(1400.0, 600.0));
        }
    }

    /// Popovers, menus and the modules' views (git, refactorings, history, diagrams).
    fn view_steps(&mut self, o: &Options) {
        if o.level {
            self.level_picker();
        }
        if o.copy_menu {
            self.copy_menu();
        }
        if o.menu {
            self.tab_menu();
        }
        if let Some(kind) = o.git.as_deref() {
            self.git(kind);
        }
        if let Some(name) = &o.rename {
            self.rename(name);
        }
        if let Some(spec) = &o.moving {
            self.move_entry(spec);
        }
        if let Some(clipboard) = o.clipboard.clone() {
            self.wb.set_clipboard_reader(std::rc::Rc::new(move || Some(clipboard.clone())));
            self.wb.run_command(command_model::CommandId("compare.clipboard"));
        }
        if let Some(dir) = o.history.clone() {
            self.local_history(dir);
        }
        if o.diagram {
            self.indexed_view(command_model::CommandId("diagram.show"));
        }
        if o.usages {
            self.indexed_view(command_model::CommandId("refactor.find_usages"));
        }
        if o.signature {
            self.indexed_view(command_model::CommandId("refactor.change_signature"));
        }
        if let Some(kind) = o.data.as_deref() {
            self.data_view(kind);
        }
        if let Some(kind) = o.database.as_deref() {
            self.database_window(kind);
        }
        if let Some(kind) = o.relations.as_deref() {
            self.relations(kind);
        }
        if let Some(kind) = o.console.as_deref() {
            self.console(kind);
        }
        if let Some(kind) = o.extractor.as_deref() {
            self.extractor_template(kind);
        }
        if let Some(needle) = &o.cursor {
            self.frame();
            self.wb.cursor_after_first(self.text, needle);
        }
        if let Some(kind) = o.related.as_deref() {
            self.related(kind);
        }
        if o.engines {
            self.frame();
            self.wb.run_command(workbench_model::CHOOSE_ENGINE);
            self.frame();
            settle(250);
        }
    }

    /// The windows over the editor and the window's own state: About, panels, dialogs,
    /// the wizard, the top bar, the Task Manager, a link under ⌘.
    fn window_steps(&mut self, o: &Options) {
        if o.resources {
            // A minute of made-up samples: the CPU with two spikes, the memory creeping up.
            for i in 0..60u64 {
                let spike = |at: u64| 6.0 * (-(((i as f32) - at as f32) / 2.0).powi(2)).exp();
                let cpu = 0.6 + spike(18) + spike(41);
                let ram = (170 + i / 6) * 1024 * 1024;
                let sample = crate::resources::Sample {
                    cpu,
                    ram_bytes: ram,
                    programs: 4,
                    programs_bytes: 169 * 1024 * 1024,
                };
                let gpu =
                    render_api::GpuMemory { shared: 15 * 1024 * 1024, window: 8 * 1024 * 1024 };
                self.wb.set_resources(sample, Some(gpu));
            }
            self.frame();
        }
        if let Some(which) = &o.status_popover {
            self.frame();
            self.wb.open_status_popover(which);
            self.frame();
            settle(150);
        }
        if o.status_more {
            self.frame();
            self.wb.open_status_more();
            self.frame();
            settle(150);
        }
        if o.about {
            self.wb.open_about();
            self.frame();
            settle(220);
        }
        if let Some(kind) = o.updates.as_deref() {
            updates::snapshot::found(kind == "long");
            if self.wb.run_command(updates::WHATS_NEW).is_none() {
                tracing::error!("What's New didn't open");
            }
            self.frame();
            settle(300);
            self.frame();
        }
        if let Some(kind) = o.ending.as_deref() {
            self.ending(kind == "many");
        }
        if let Some(panel) = &o.panel {
            self.side_panel(panel);
        }
        if let Some(kind) = o.services.as_deref() {
            self.services(kind);
        }
        match o.dialog.as_deref() {
            Some("delete") => self.delete_dialog(),
            Some("delete-many") => self.delete_many_dialog(),
            Some(kind @ ("running" | "running-wait")) => {
                self.running_prompt(kind == "running-wait")
            }
            Some(kind) => self.dialog(kind == "rename"),
            None => {}
        }
        if let Some(name) = &o.new_entry {
            self.frame();
            self.wb.new_entry(false);
            self.wb.type_text(self.text, name);
        }
        if let Some(kind) = &o.projects {
            self.wb.open_projects_menu();
            if kind != "1" {
                self.project_groups(kind);
            }
        }
        if let Some(step) = o.remote {
            self.remote_wizard(step);
        }
        if let Some(state) = &o.remote_edit {
            self.remote_edit(state);
        }
        if o.agent_chat {
            agent_chat::snapshot(self.wb);
            self.frame();
        }
        if let Some(which) = &o.remote_changed {
            remote_projects::snapshot_changed(self.wb, which);
            self.frame();
        }
        if let Some(which) = &o.remote_props {
            remote_projects::snapshot_properties(self.wb, which);
            self.frame();
        }
        if let Some(controls) = &o.window_controls {
            self.window_controls(controls);
        }
        if let Some(drawer) = &o.menu_drawer {
            self.menu_drawer(drawer);
        }
        // The studio's list (on the left of its tab), scrolled: once drawn, it knows its height.
        if let Some(dy) = o.studio_scroll {
            self.frame();
            self.wb.scroll(Point::new(SIZE.width as f32 * 0.35, SIZE.height as f32 * 0.5), 0.0, dy);
        }
        if let Some(commands) = &o.studio_command {
            for command in commands.split(',') {
                // Drawn first: a section's place is known once it's drawn.
                self.frame();
                if !self.wb.viewer_command(command.trim()) {
                    tracing::error!("the studio didn't take `{command}`");
                }
            }
        }
        // A command opened in Settings → Shortcuts, its keys recorded (a conflict asked).
        if let Some(command) = &o.shortcut {
            self.frame();
            self.wb.open_shortcut(command);
            if let Some(keys) = &o.shortcut_keys
                && let Some(ui_workbench::SettingsAction::Shortcuts(changes)) =
                    self.wb.record_shortcut(keys, &crate::commands::display_shortcut(keys))
            {
                tracing::info!(?changes, "snapshot: shortcut recorded without a conflict");
            }
        }
        if o.settings_open {
            self.frame();
            self.wb.settings_open_result();
            self.frame();
            self.frame();
        }
        // The settings' slow figures (what the local history holds), counted in the
        // background: the blocks drawn with them.
        if o.settings.is_some() {
            self.frame();
            settle(150);
            self.wb.poll_viewers();
        }
        if let Some(scenario) = &o.tasks {
            self.task_manager(scenario);
        }
        if let Some(p) = o.link {
            self.wait_for_indexing(Duration::from_secs(20));
            self.wb.sync_syntax();
            self.frame();
            self.wb.set_link_mode(self.text, true, p);
        }
    }
}

/// How long a screen still moving after its transitions is watched before what moves is
/// named (longer than any transition).
const SETTLE_MOVING: Duration = Duration::from_millis(1500);

/// Lets animations and background work progress.
fn settle(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(test)]
#[path = "../../tests/unit/snapshot.rs"]
mod tests;
