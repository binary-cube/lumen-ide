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

mod cluster;
mod console_steps;
mod data_fixtures;
mod data_steps;
mod fixtures;
mod git_steps;
mod mysql_like;
mod overlay_steps;

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

fn var(name: &str) -> Option<String> {
    std::env::var(format!("EDITOR_SNAPSHOT_{name}")).ok()
}

fn flag(name: &str) -> bool {
    var(name).is_some()
}

fn parsed<T: FromStr>(name: &str) -> Option<T> {
    var(name)?.parse().ok()
}

/// `x,y`.
fn point(name: &str) -> Option<Point> {
    let value = var(name)?;
    let (x, y) = value.split_once(',')?;
    Some(Point::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

impl Options {
    pub fn from_env() -> Self {
        Self {
            theme: var("THEME"),
            themed: var("THEMED"),
            my_decorations: var("MY_DECORATIONS"),
            studio: flag("STUDIO"),
            studio_scroll: var("STUDIO_SCROLL").and_then(|v| v.parse().ok()),
            studio_command: var("STUDIO_COMMAND"),
            studio_live: flag("STUDIO_LIVE"),
            settings: var("SETTINGS").map(|v| v.parse().unwrap_or(0)),
            settings_query: var("SETTINGS_QUERY"),
            settings_page: var("SETTINGS_PAGE"),
            settings_language: var("SETTINGS_LANGUAGE"),
            settings_advanced: flag("SETTINGS_ADVANCED"),
            shortcut: var("SHORTCUT"),
            shortcut_keys: var("SHORTCUT_KEYS"),
            fold: flag("FOLD"),
            find: var("FIND"),
            files: var("FILES"),
            terminal: var("TERMINAL"),
            terminal_split: var("TERMINAL_SPLIT"),
            bottom: var("BOTTOM"),
            typed: var("TYPE").map(|v| v.replace("\\n", "\n").replace("{sc}", ";")),
            line: parsed("LINE"),
            search: var("SEARCH"),
            select: var("SELECT"),
            color: parsed("COLOR"),
            splits: var("SPLIT").map(|v| parse_splits(&v)).unwrap_or_default(),
            canvas: var("CANVAS").map(|v| v.chars().next().unwrap_or('1')),
            canvas_edit: var("CANVAS_EDIT"),
            canvas_query: var("CANVAS_QUERY"),
            tab_drag: var("TABDRAG").and_then(|v| parse_tab_drag(&v)),
            tree_drag: var("TREEDRAG").and_then(|v| parse_tab_drag(&v)),
            compare: var("COMPARE").and_then(|v| parse_compare(&v)),
            pin: parsed("PIN"),
            lock: flag("LOCK"),
            zoom: parsed("ZOOM"),
            level: flag("LEVEL"),
            about: flag("ABOUT"),
            updates: var("UPDATES"),
            ending: var("ENDING"),
            resources: flag("RESOURCES"),
            status_more: flag("STATUS_MORE"),
            status_popover: var("STATUS_POPOVER"),
            commands: var("COMMAND")
                .map(|v| v.split(',').filter(|c| !c.is_empty()).map(str::to_owned).collect())
                .unwrap_or_default(),
            view_commands: var("VIEW_COMMAND")
                .map(|v| v.split(',').filter(|c| !c.is_empty()).map(str::to_owned).collect())
                .unwrap_or_default(),
            project_windows: var("PROJECT_WINDOWS")
                .map(|v| v.split(',').filter(|c| !c.is_empty()).map(str::to_owned).collect())
                .unwrap_or_default(),
            file_drop: var("FILEDROP")
                .map(|v| v.split(',').filter(|p| !p.is_empty()).map(PathBuf::from).collect()),
            copy_menu: flag("COPY"),
            menu: flag("MENU"),
            git: var("GIT"),
            rename: var("RENAME"),
            moving: var("MOVE"),
            signature: flag("SIGNATURE"),
            usages: flag("USAGES"),
            diagram: flag("DIAGRAM"),
            history: var("HISTORY").map(PathBuf::from),
            clipboard: var("CLIPBOARD").map(|v| v.replace("\\n", "\n")),
            panel: var("PANEL"),
            dialog: var("DIALOG"),
            new_entry: var("NEW"),
            projects: var("PROJECTS").filter(|v| v != "0"),
            remote: parsed("REMOTE"),
            remote_edit: var("REMOTE_EDIT"),
            remote_props: var("REMOTE_PROPS"),
            remote_changed: var("REMOTE_CHANGED"),
            agent_chat: var("AGENT_CHAT").is_some(),
            tasks: var("TASKS"),
            split_at: parsed("SPLIT_AT"),
            window_controls: var("WINDOW_CONTROLS"),
            menu_drawer: var("MENU_DRAWER"),
            settings_open: flag("SETTINGS_OPEN"),
            link: point("LINK"),
            hover: point("HOVER"),
            hover_word: var("HOVER_WORD"),
            splash: flag("SPLASH"),
            data: var("DATA"),
            database: var("DATABASE"),
            relations: var("RELATIONS"),
            toasts: var("TOASTS"),
            toast_position: var("TOAST_POSITION"),
            services: var("SERVICES"),
            console: var("CONSOLE"),
            extractor: var("EXTRACTOR"),
            lsp: var("LSP"),
            cursor: var("CURSOR"),
            engines: flag("ENGINES"),
            insert: var("INSERT").map(|v| v.replace("\\n", "\n").replace("{sc}", ";")),
            related: var("RELATED"),
            typing: parsed("TYPING"),
            idle: parsed("IDLE"),
            soak: parsed("SOAK"),
        }
    }
}

/// `right,down,move-right,move-down`.
fn parse_splits(value: &str) -> Vec<(Split, bool)> {
    value
        .split(',')
        .map(|s| match s.trim() {
            "down" => (Split::Down, false),
            "move-right" => (Split::Right, true),
            "move-down" => (Split::Down, true),
            _ => (Split::Right, false),
        })
        .collect()
}

/// `x1,y1,x2,y2`, with `!` at the end to drop.
fn parse_tab_drag(value: &str) -> Option<TabDrag> {
    let drop = value.ends_with('!');
    let v: Vec<f32> =
        value.trim_end_matches('!').split(',').filter_map(|n| n.trim().parse().ok()).collect();
    let [x1, y1, x2, y2] = v[..] else { return None };
    Some(TabDrag { from: Point::new(x1, y1), to: Point::new(x2, y2), drop })
}

/// `left,right[,merge]`.
fn parse_compare(value: &str) -> Option<(PathBuf, PathBuf, bool)> {
    let parts: Vec<&str> = value.split(',').collect();
    let [left, right, rest @ ..] = &parts[..] else { return None };
    Some((PathBuf::from(left), PathBuf::from(right), rest.first() == Some(&"merge")))
}

/// Before anything opens: the language servers are never the real bundled ones (what they
/// say varies); `LSP` describes the fake one (this binary, `--fake-language-server`) for
/// TypeScript, and for CSS / SCSS as the CSS server is described (the built-in engine
/// keeping the deprecations), in a file of the temporary folder (never the user's).
pub fn before_open() {
    // The MCP server's token in memory (never the user's keychain).
    mcp_server::secret::use_secrets(std::sync::Arc::new(SnapshotSecrets::default()));
    // Measurements (`REAL_LSP=1`): the servers bundled beside the binary, as the app has them.
    if let Some(seconds) = parsed::<u64>("IDLE") {
        language_servers::idle_after(Duration::from_secs(seconds));
    }
    if flag("REAL_LSP") {
        return;
    }
    language_servers::use_bundle(None);
    php_engines();
    downloads();
    if var("LSP").is_none() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let file =
        std::env::temp_dir().join(format!("snapshot-language-servers-{}.toml", std::process::id()));
    let text = format!(
        "[[language-servers]]\nname = \"fake\"\nlanguages = [\"typescript\"]\ncommand = {exe:?}\nargs = [\"--fake-language-server\"]\n\n[[language-servers]]\nname = \"fake-css\"\nlanguages = [\"css\", \"scss\"]\ncommand = {exe:?}\nargs = [\"--fake-language-server\"]\nbuilt-in = [\"deprecated\", \"rename\"]\ncompletion-kinds = {{ property = \"attribute\" }}\n\n[[language-servers]]\nname = \"fake-cpp\"\nlanguages = [\"c\", \"cpp\"]\ncommand = {exe:?}\nargs = [\"--fake-language-server\"]\n",
        exe = exe.to_string_lossy()
    );
    if std::fs::write(&file, text).is_ok() {
        language_servers::use_user_file(file);
    }
}

/// The servers downloaded on demand, never in the user's data folder nor from the network:
/// nothing downloadable, or (`LSP_OFFER`) a folder of the temporary folder, files in memory.
fn downloads() {
    let Some(case) = var("LSP_OFFER") else {
        language_servers::use_downloads(None);
        return;
    };
    let dir = std::env::temp_dir().join(format!("snapshot-lsp-downloads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    language_servers::use_downloads(Some(dir.clone()));
    let memory = std::sync::Arc::new(lsp_client::fetch::Memory::default());
    language_servers::use_fetch(
        std::sync::Arc::clone(&memory) as std::sync::Arc<dyn lsp_client::fetch::Fetch>
    );
    let manifest = language_servers::downloads::manifest();
    let platform = language_servers::downloads::platform();
    match case.as_str() {
        "downloading" => {
            // Node's archive stalls at 40%: its progress shown, still going.
            let Some(node) = manifest.named("node") else { return };
            let Some(archive) = node.platforms.get(platform) else { return };
            let at = archive.size * 2 / 5;
            memory.serve(&archive.url, vec![0u8; at as usize + 1]);
            memory.stall_after(Some(at));
            language_servers::downloads::start(vec!["node".into(), "vtsls".into()], None);
            let until = Instant::now() + Duration::from_secs(10);
            while Instant::now() < until
                && !matches!(language_servers::downloads::status("node"), language_servers::downloads::Status::Downloading(p) if p.received >= at)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        "parts" => {
            // Downloaded already (their markers: what the list reads).
            for name in ["node", "php", "phpactor", "vtsls"] {
                let Some(c) = manifest.named(name) else { continue };
                let folder = lsp_client::store::path_in(&dir, &c.folder());
                let _ = std::fs::create_dir_all(&folder);
                let marker = lsp_client::store::Marker {
                    name: c.name.clone(),
                    version: c.version.clone(),
                    fingerprint: c.fingerprint(platform),
                    size: c.size(platform) * 3,
                    platform: platform.to_owned(),
                };
                let _ = lsp_client::store::write_marker(&folder, &marker);
                if let Some(bin) = c.bin(platform) {
                    let program = lsp_client::store::path_in(&folder, bin);
                    let _ = std::fs::create_dir_all(program.parent().unwrap_or(&folder));
                    let _ = std::fs::write(program, "");
                }
            }
            language_servers::use_downloads(Some(dir));
        }
        _ => {}
    }
}

/// The servers the user installs (intelephense), never the user's: installed in a folder
/// of the temporary folder or not, secrets in memory, a licence file there or not; with
/// `LSP_PHP` (`missing`, `installed`: a licence file found, `licensed`: a key kept) the
/// settings' language servers block shows PHP.
fn php_engines() {
    let dir = std::env::temp_dir().join("snapshot-lsp-installs");
    let _ = std::fs::remove_dir_all(&dir);
    let case = var("LSP_PHP");
    let secrets = std::sync::Arc::new(SnapshotSecrets::default());
    let licence = dir.join("intelephense").join("licence.txt");
    if matches!(case.as_deref(), Some("installed" | "licensed")) {
        let package = dir.join("installed").join("intelephense").join("1.18.5");
        let _ = std::fs::create_dir_all(package.join("lib"));
        let _ = std::fs::write(
            package.join("package.json"),
            r#"{"name": "intelephense", "version": "1.18.5"}"#,
        );
        let _ = std::fs::write(package.join("lib").join("intelephense.js"), "");
        let _ = std::fs::create_dir_all(dir.join("intelephense"));
        let _ = std::fs::write(&licence, "SNAPSHOT-KEY\n");
    }
    if case.as_deref() == Some("licensed") {
        let _ = kernel::SecretStore::set(&*secrets, "intelephense-licence", "SNAPSHOT-KEY");
    }
    language_servers::use_install_dir(dir.join("installed"));
    language_servers::use_secrets(secrets);
    language_servers::use_licence_files(vec![licence]);
    if case.is_some() {
        language_servers::wait_known(Duration::from_secs(5));
    }
}

/// Secrets in memory (the snapshots never touch the keychain).
#[derive(Default)]
struct SnapshotSecrets(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl kernel::SecretStore for SnapshotSecrets {
    fn get(&self, id: &str) -> Option<kernel::Secret> {
        self.0.lock().ok()?.get(id).map(|s| kernel::Secret::new(s.as_str()))
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert(id.into(), secret.into());
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(id);
        Ok(())
    }
}

/// The app-level state of the snapshot (theme, theme studio, settings), before rendering.
pub fn prepare(
    options: &Options,
    workbench: &mut Workbench,
    themes: &mut Themes,
) -> anyhow::Result<()> {
    if let Some(theme) = &options.theme
        && !themes.select(theme)
    {
        anyhow::bail!("theme `{theme}` does not exist");
    }
    if let Some(family) = &options.themed {
        let dark = themes.resolved().is_some_and(|t| t.appearance == theming::Appearance::Dark);
        let variant = format!("{family}-{}", if dark { "dark" } else { "light" });
        if !themes.select(&variant) {
            anyhow::bail!("theme `{variant}` does not exist");
        }
    }
    if let Some(preset) = &options.my_decorations {
        use theme_decorations::settings::{Mine, Overlay, set_overlay};
        use theming::decorations::{FieldValue, LayerKind, LayerSpec};
        // A preset's id, or one effect of a kind (`matrix`), its style (`synthwave:city`),
        // where it shows (`@panels`).
        let (name, zone) = preset.split_once('@').unwrap_or((preset.as_str(), ""));
        let (name, style) = name.split_once(':').unwrap_or((name, ""));
        // With a zone it's always an effect (`synthwave@panels`); without, a preset first.
        let mine = if zone.is_empty() && style.is_empty() { Mine::parse(name) } else { None };
        let overlay = match (mine, LayerKind::parse(name)) {
            (Some(mine), _) => Overlay { mine, ..Overlay::default() },
            (None, Some(kind)) => {
                let mut layer = LayerSpec::new(kind);
                if !zone.is_empty()
                    && layer.set("zone", &FieldValue::Text(zone.to_owned())).is_err()
                {
                    anyhow::bail!("no zone `{zone}` for `{name}`");
                }
                if !style.is_empty()
                    && layer.set("style", &FieldValue::Text(style.to_owned())).is_err()
                {
                    anyhow::bail!("no style `{style}` for `{name}`");
                }
                Overlay { mine: Mine::Custom, custom: vec![layer], ..Overlay::default() }
            }
            (None, None) => anyhow::bail!("no decorations preset or effect `{preset}`"),
        };
        set_overlay(overlay);
    }
    // The theme's decorations as a still frame (the same every run).
    workbench.set_decorations_window(false, true, true);
    // Word wrap on (`WRAP=1`): every view told so at its first frame, as the setting would.
    if flag("WRAP") {
        crate::text_options::set(ui_workbench::TextOptions { word_wrap: true });
    }
    // The user's backdrop over the theme's (`BACKDROP=glass:6:20`: the style, the blur in
    // pixels, the tint in percent; each optional).
    if let Some(backdrop) = var("BACKDROP") {
        crate::backdrop::set_own(crate::backdrop::parse_own(&backdrop));
    }
    workbench.set_palette(themes.palette());
    if let Some(theme) = themes.resolved() {
        crate::theme_options::set_active(&theme);
        workbench.set_theme(theme);
    }
    if options.studio {
        let mut input = themes.studio_input(themes.current_id());
        // The names as their ids (what the snapshots always showed).
        for theme in &mut input.themes {
            theme.name = theme.id.clone();
        }
        workbench.set_theme_source(std::rc::Rc::new(move |_| input.clone()));
        workbench.run_command(workbench_model::THEME_STUDIO);
    }
    if let Some(steps) = options.settings {
        workbench.open_settings(crate::app::settings_model(themes));
        if let Some(query) = &options.settings_query {
            wait_for_semantic_model(Duration::from_secs(10));
            workbench.take_settings_embedder();
            workbench.settings_query(query);
        }
        workbench.settings_step(steps);
        if let Some(page) = &options.settings_page {
            workbench.settings_show_page(page);
        }
        if let Some(language) = &options.settings_language {
            workbench.select_settings_language(language);
        }
        if options.settings_advanced {
            workbench.settings_open_advanced();
        }
        // The modal's appear animation finishes before the capture.
        std::thread::sleep(Duration::from_millis(220));
    }
    Ok(())
}

/// Renders the screen into `out`.
pub fn render(
    out: &Path,
    options: &Options,
    mut workbench: Workbench,
    themes: &mut Themes,
    fonts: JoinHandle<(TextSystem, FontFamily)>,
) -> anyhow::Result<()> {
    let (mut text, family) =
        fonts.join().map_err(|_| anyhow::anyhow!("loading the fonts failed"))?;
    workbench.set_font_family(family);
    workbench.resize(*SIZE, 2.0);
    Steps { wb: &mut workbench, text: &mut text }.run(options);
    // `EDITOR_SNAPSHOT_TOOL_BADGES=<tool>:<count>[:warning|error],…`: counts on tool windows'
    // buttons, as their modules put them.
    if let Ok(badges) = std::env::var("EDITOR_SNAPSHOT_TOOL_BADGES") {
        use workbench_model::ViewHost as _;
        for spec in badges.split(',') {
            let mut parts = spec.split(':');
            let (Some(tool), Some(count)) = (parts.next(), parts.next()) else { continue };
            let kind = match parts.next() {
                Some("warning") => workbench_model::BadgeKind::Warning,
                Some("error") => workbench_model::BadgeKind::Error,
                _ => workbench_model::BadgeKind::Count,
            };
            let count = count.parse().unwrap_or(0);
            let badge = workbench_model::ToolBadge { count, kind, label: String::new() };
            let tool: &'static str = Box::leak(tool.to_owned().into_boxed_str());
            workbench.set_tool_badge(tool, Some(badge));
        }
    }
    // The studio's work-in-progress theme on the window (as `AppRequest::PreviewTheme`).
    if options.studio_live {
        workbench.paint(&mut text, &mut Scene::default());
        let src = workbench.take_theme_preview();
        let id = src.as_deref().map(|src| themes.register_source(src));
        match id {
            Some(Ok(id)) => {
                if let Some((palette, theme)) = themes.palette_of(&id) {
                    workbench.set_palette(palette);
                    workbench.set_theme(theme);
                }
            }
            Some(Err(e)) => anyhow::bail!("the studio's theme is invalid: {e}"),
            None => anyhow::bail!("the studio asked for no preview"),
        }
    }
    // Appear animations (menus, modals) end and views finish loading (a document's images)
    // before the capture: the same picture every run.
    // What appears once loaded (a chart's slices fading in) has its second from then.
    let (mut animations, loads) =
        (Instant::now() + Duration::from_secs(1), Instant::now() + Duration::from_secs(3));
    // The first frame painted, kept for `EDITOR_SNAPSHOT_PARTIAL` (a frame before this one).
    let mut previous = Scene::default();
    workbench.paint(&mut text, &mut previous);
    while (workbench.animating() && Instant::now() < animations)
        || (workbench.loading() && Instant::now() < loads)
    {
        if workbench.loading() {
            animations = animations.max(Instant::now() + Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_millis(16));
        // What the views' work brought meanwhile taken, as the app does every frame.
        workbench.poll_viewers();
        workbench.paint(&mut text, &mut Scene::default());
    }
    // Still moving past every transition's end: given a little longer, then whatever still
    // says it moves is named (`moving: …`): the snapshots list the screens where a part
    // keeps asking for frames (on purpose: a scene that moves; else a part to mend).
    if workbench.animating() {
        let until = Instant::now() + SETTLE_MOVING;
        while workbench.animating() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(16));
            workbench.poll_viewers();
            workbench.paint(&mut text, &mut Scene::default());
        }
        let parts = workbench.moving_parts();
        if !parts.is_empty() {
            eprintln!("moving: {}", parts.join(", "));
        }
    }
    // The settings' content scrolled by the wheel (`SETTINGS_SCROLL=<px>`, in steps of
    // 120 px, a frame painted after each).
    if let Some(total) = parsed::<f32>("SETTINGS_SCROLL") {
        let mut left = total;
        while left.abs() > 0.5 {
            let step = left.clamp(-120.0, 120.0);
            workbench.settings_wheel(step);
            workbench.paint(&mut text, &mut Scene::default());
            left -= step;
        }
    }
    // The memory once everything settled (`EDITOR_SNAPSHOT_MEMORY=1`): measurements.
    if flag("MEMORY") {
        kernel::memory::release_free();
        let sample = crate::resources::measure_now();
        eprintln!("settled memory: {} MB", sample.ram_bytes / (1024 * 1024));
    }

    // What the views and modules told meanwhile, its time stopped (a ring caught part way).
    workbench.settle_notifications();
    // Carets (the editor's, the terminal's) caught lit, whatever the time it took.
    ui_kit::caret::reset();
    let mut scene = Scene::default();
    scene.reset(workbench.background());
    // The window's own buttons (as on Windows), not maximized: its corners cut.
    if options.window_controls.as_deref() == Some("1") {
        scene.window_corners = 16.0;
    }
    if options.splash {
        let frame = ui_workbench::SplashFrame {
            bounds: kernel::Rect::from_size(SIZE.to_size()),
            scale: 2.0,
            phase: 0.45,
            opacity: 1.0,
        };
        ui_workbench::Splash.paint(
            frame,
            concat!("v", env!("CARGO_PKG_VERSION")),
            &i18n::t("splash.preparing"),
            resources::splash_art().as_ref(),
            ui_workbench::Painter {
                text: &mut text,
                scene: &mut scene,
                palette: workbench.palette(),
                icons: workbench.icons(),
            },
        );
    } else {
        // `EDITOR_SNAPSHOT_ACCESS=1`: the screen's accessibility tree, made as it's painted,
        // written beside the picture (`<out>.access.txt`).
        let access = flag("ACCESS").then(ui_kit::access::begin);
        workbench.paint(&mut text, &mut scene);
        if let Some(mut frame) = access.map(ui_kit::access::Recording::take) {
            // As the app tells it: a control without a text of its own named by its tooltip.
            frame.name_from_tips(|bounds| workbench.access_tip(bounds.center()));
            let path = out.with_extension("access.txt");
            std::fs::write(&path, crate::snapshot_gpu::access_text(&frame))
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
    }
    // `EDITOR_SNAPSHOT_DEBUG_OVERLAY=1`: the diagnostics overlay (F12) over it, with made-up
    // frames (one over its budget).
    if flag("DEBUG_OVERLAY") {
        let mut overlay = ui_workbench::DebugOverlay::new("Metal (Apple M2)".into());
        overlay.toggle();
        for i in 0..60u64 {
            let stats = render_api::FrameStats {
                quads: 812,
                glyphs: 2_431,
                draw_calls: 16,
                cached_glyphs: 965,
                drawn_pixels: 120_000,
                ..render_api::FrameStats::default()
            };
            overlay.record(std::time::Duration::from_micros(2_400 + (i % 7) * 900), stats);
            overlay.record_latency(std::time::Duration::from_millis(9 + i % 5));
        }
        overlay.record(std::time::Duration::from_millis(19), render_api::FrameStats::default());
        let area = workbench.overlay_area();
        let viewport = kernel::Size::new(SIZE.width as f32, SIZE.height as f32);
        let painter = ui_workbench::Painter {
            text: &mut text,
            scene: &mut scene,
            palette: workbench.palette(),
            icons: workbench.icons(),
        };
        overlay.paint(painter, area, viewport, 2.0);
    }
    let mut renderer =
        SoftwareRenderer::new(*SIZE).context("cannot create the snapshot surface")?;
    let icons = std::sync::Arc::clone(workbench.icons());
    // `EDITOR_SNAPSHOT_PARTIAL=1`: the picture drawn after an earlier frame, only where it
    // changed (the partial redraw), must be the one drawn whole; it fails otherwise.
    if flag("PARTIAL") {
        let mut glyphs = icons::WithIcons { text: &mut text, icons: &icons };
        renderer.render(&previous, &mut glyphs)?;
        let stats = renderer.render(&scene, &mut glyphs)?;
        let mut whole =
            SoftwareRenderer::new(*SIZE).context("cannot create the snapshot surface")?;
        whole.render(&scene, &mut glyphs)?;
        anyhow::ensure!(
            renderer.encode_png()? == whole.encode_png()?,
            "the partial redraw ({} pixels) differs from the whole one",
            stats.drawn_pixels
        );
        tracing::info!(drawn = stats.drawn_pixels, "partial redraw checked");
    }
    renderer.render(&scene, &mut icons::WithIcons { text: &mut text, icons: &icons })?;
    std::fs::write(out, renderer.encode_png()?)
        .with_context(|| format!("cannot write {}", out.display()))?;
    tracing::info!(path = %out.display(), "snapshot saved");
    // On the GPU too, when asked: after the picture, so measuring never changes it.
    crate::snapshot_gpu::measure(&mut workbench, &mut text, *SIZE, &scene)?;
    Ok(())
}

/// Waits (up to `timeout`) for the semantic model, if it is installed.
fn wait_for_semantic_model(timeout: Duration) {
    semantic::load(None);
    let deadline = Instant::now() + timeout;
    while semantic::is_installed() && semantic::model().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
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

    /// Polls the views until none is busy, or `timeout` passes.
    fn wait_for_viewers(&mut self, timeout: Duration) {
        self.wait(timeout, |wb| {
            wb.poll_viewers();
            !wb.viewer_busy() && !wb.viewer_working()
        });
    }

    /// The active file revealed in the tree, then a tree entry dragged (and dropped).
    fn tree_drag(&mut self, drag: TabDrag) {
        if let Some(i) = self.wb.active_tab() {
            self.wb.reveal_in_project(i);
        }
        self.tab_drag(drag);
        if drag.drop {
            self.wait_for_viewers(Duration::from_secs(20));
        }
    }

    /// The first tab pinned, then the tab bar scrolled by `dx` pixels.
    fn pin(&mut self, dx: f32) {
        self.wb.toggle_pin(0);
        self.frame();
        self.wb.scroll(Point::new(1400.0, 118.0), dx, 0.0);
    }

    /// The rename preview for the symbol at the cursor, `name` typed (empty: the old name).
    fn rename(&mut self, name: &str) {
        self.wait(Duration::from_secs(20), |wb| !wb.indexing());
        self.wb.run_command(command_model::CommandId("refactor.rename"));
        self.wait_for_viewers(Duration::from_secs(20));
        if !name.is_empty() {
            self.wb.type_text(self.text, name);
        }
    }

    /// The Move preview: `folder[,typed][!]`.
    fn move_entry(&mut self, spec: &str) {
        self.wait(Duration::from_secs(20), |wb| !wb.indexing());
        let apply = spec.ends_with('!');
        let spec = spec.trim_end_matches('!');
        let (folder, typed) = spec.split_once(',').unwrap_or((spec, ""));
        let (Some(root), Some(from)) =
            (self.wb.project_root(), self.wb.active_tab().and_then(|i| self.wb.tab_path(i)))
        else {
            return;
        };
        self.wb.move_entry(&from, &root.join(folder));
        self.wait_for_viewers(Duration::from_secs(20));
        if !typed.is_empty() {
            self.wb.viewer_command("select_all");
            self.wb.type_text(self.text, typed);
        }
        if apply {
            self.wb.edit_key(self.text, EditKey::Enter { shift: false });
        }
    }

    /// The active file's local history, its versions read from `dir` (a fixture: the view
    /// opened with its own data, the module's type — the only one named here).
    fn local_history(&mut self, dir: PathBuf) {
        let Some(i) = self.wb.active_tab() else { return };
        let (Some(file), Some(path)) = (self.wb.tab_path(i), self.wb.tab_relative_path(i)) else {
            return;
        };
        let data = local_history::HistoryData {
            store: std::sync::Arc::new(history_fs::DiskHistory::new(dir)),
            path: path.to_string_lossy().replace('\\', "/"),
            file,
        };
        workbench_model::ViewHost::show_in_panel(
            self.wb,
            local_history::TOOL,
            "History",
            local_history::EXTENSION,
            &data,
        );
    }

    /// A view that needs the index (a diagram, the usages, Change Signature), once ready.
    fn indexed_view(&mut self, command: command_model::CommandId) {
        self.wait(Duration::from_secs(20), |wb| !wb.indexing());
        self.wb.run_command(command);
        self.wait_for_viewers(Duration::from_secs(20));
    }

    /// The window's own buttons (`1`, `max`), or full screen (`full`, `full-shown`).
    fn window_controls(&mut self, controls: &str) {
        self.wb.set_window_controls(Some(controls == "max"));
        // Full screen: the bar hidden, or come down (the mouse at the top edge).
        self.wb.set_full_screen(controls.starts_with("full"));
        self.frame();
        if controls == "full-shown" {
            self.wb.hover(Point::new(SIZE.width as f32 / 2.0, 0.0));
            settle(300);
            self.frame();
        }
    }

    /// A paint into a throwaway scene (layouts, hit areas and animations update).
    fn frame(&mut self) {
        self.wb.paint(self.text, &mut Scene::default());
    }

    /// Polls `ready` until it says yes or `timeout` passes.
    fn wait(&mut self, timeout: Duration, mut ready: impl FnMut(&mut Workbench) -> bool) {
        let deadline = Instant::now() + timeout;
        while !ready(self.wb) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `keys` keys typed at the end of the active file, about 12 a second (a fast typist),
    /// each followed by its frame; between keys, what the servers answered is polled and
    /// painted as the app does. Printed: what each key and each answer cost the UI thread
    /// (the edit, the servers' answers used, the frame's scene — the GPU's drawing aside,
    /// the same with or without a server), and how often the suggestions were shown.
    fn typing(&mut self, keys: usize) {
        // The file's server ready (none: the built-in engine), its project loaded. (The
        // document is announced to the servers by a frame.)
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.wb.served_at_cursor() && Instant::now() < deadline {
            self.wb.poll_servers();
            self.frame();
            settle(20);
        }
        let served = self.wb.served_at_cursor();
        let warm = Instant::now() + Duration::from_secs(if served { 5 } else { 1 });
        while Instant::now() < warm {
            self.wb.poll_servers();
            self.wb.poll_diagnostics();
            self.frame();
            settle(20);
        }
        self.wb.place_cursor_at_line_end(self.text, None);
        const TEXT: &str = "\nconst names = items.map((item) => item.name.trim());\nconsole.log(names.length, names.join(', '));\n";
        let (mut typed, mut answers, mut listed) = (Vec::new(), Vec::new(), 0usize);
        let started = Instant::now();
        for c in TEXT.chars().cycle().take(keys) {
            let key = Instant::now();
            match c {
                '\n' => {
                    // Enter would accept a suggestion: the list closed first.
                    self.wb.edit_key(self.text, EditKey::Escape);
                    self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                }
                _ => {
                    self.wb.type_text(self.text, c.encode_utf8(&mut [0; 4]));
                }
            }
            self.wb.poll_servers();
            self.wb.poll_diagnostics();
            self.wb.paint(self.text, &mut Scene::default());
            typed.push(key.elapsed());
            let next = key + Duration::from_millis(83);
            while Instant::now() < next {
                settle(4);
                let at = Instant::now();
                if self.wb.poll_servers() | self.wb.poll_diagnostics() {
                    self.wb.paint(self.text, &mut Scene::default());
                    answers.push(at.elapsed());
                }
            }
            listed += usize::from(self.wb.completion_open());
        }
        let stats = |mut v: Vec<Duration>| {
            v.sort();
            let at = |q: f64| v.get(((v.len() as f64 - 1.0) * q) as usize).copied();
            let ms = |d: Option<Duration>| d.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            format!(
                "n {} p50 {:.2} p95 {:.2} p99 {:.2} max {:.2} ms",
                v.len(),
                ms(at(0.5)),
                ms(at(0.95)),
                ms(at(0.99)),
                ms(v.last().copied())
            )
        };
        eprintln!(
            "typing: served {served}, {keys} keys in {:.1} s, list shown after {listed}",
            started.elapsed().as_secs_f64()
        );
        eprintln!("typing: keys: {}", stats(typed));
        eprintln!("typing: answers: {}", stats(answers));
    }

    /// Every tab closed, then `seconds` + 25 s of frames (10 a second): the servers left
    /// without documents stop meanwhile. The process's memory printed before and after.
    fn idle(&mut self, seconds: u64) {
        let mb = || crate::resources::measure_now().ram_bytes / (1024 * 1024);
        eprintln!("idle: app {} MB with the tabs open", mb());
        while self.wb.tab_count() > 0 {
            self.wb.close_tab(0);
        }
        self.frame();
        let closed = Instant::now();
        eprintln!("idle: tabs closed");
        while closed.elapsed() < Duration::from_secs(seconds + 25) {
            self.wb.poll_servers();
            self.frame();
            settle(100);
        }
        kernel::memory::release_free();
        eprintln!("idle: app {} MB after {} s", mb(), closed.elapsed().as_secs());
    }

    /// `seconds` of frames, 30 a second, nothing else done; the process's memory and the
    /// heap's live bytes printed every 10 s.
    fn soak(&mut self, seconds: u64) {
        let mb = |b: u64| b / (1024 * 1024);
        let started = Instant::now();
        let mut next = started;
        let mut frames = 0u64;
        while started.elapsed() < Duration::from_secs(seconds) {
            if Instant::now() >= next {
                let ram = mb(crate::resources::measure_now().ram_bytes);
                let heap = kernel::memory::live_heap().map_or(0, |b| mb(b as u64));
                eprintln!(
                    "soak: {:>4} s, {frames} frames, app {ram} MB, heap {heap} MB",
                    started.elapsed().as_secs()
                );
                next += Duration::from_secs(10);
            }
            self.frame();
            frames += 1;
            settle(33);
        }
    }

    fn wait_for_indexing(&mut self, timeout: Duration) {
        settle(50);
        self.wait(timeout, |wb| !wb.indexing());
    }

    fn file_search(&mut self, query: &str) {
        self.wb.focus_file_search();
        self.wb.type_text(self.text, query);
        // The file index is built in the background.
        self.wait(Duration::from_secs(5), Workbench::poll_index);
        // The panel's results, from its search's thread.
        self.frame();
        self.wait(Duration::from_secs(5), Workbench::poll_side_panel);
    }

    fn terminal(&mut self, command: &str) {
        self.wb.toggle_tool("terminal");
        // A first paint starts the shell (its size and the waker known then).
        self.frame();
        settle(300);
        self.wb.type_text(self.text, &format!("{command}\r"));
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            self.wb.poll_viewers();
            settle(20);
        }
    }

    fn type_at_line(&mut self, typed: &str, line: Option<usize>) {
        self.wait_for_indexing(Duration::from_secs(20));
        self.wb.place_cursor_at_line_end(self.text, line);
        // The language at the cursor (HTML in PHP) comes from parsing.
        self.wb.sync_syntax();
        for c in typed.chars() {
            match c {
                '\t' => {
                    self.wb.edit_key(self.text, EditKey::Tab { shift: false });
                }
                '\n' => {
                    self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                }
                _ => {
                    self.wb.type_text(self.text, c.encode_utf8(&mut [0; 4]));
                }
            }
        }
    }

    fn project_search(&mut self, query: &str) {
        self.wait_for_indexing(Duration::from_secs(20));
        // The semantic model, if installed (vectors are computed in the background).
        wait_for_semantic_model(Duration::from_secs(20));
        if self.wb.poll_indexer() || semantic::model().is_some() {
            settle(500);
        }
        // The folders section uses the file index.
        self.wb.start_file_index();
        self.wait(Duration::from_secs(10), Workbench::poll_index);
        self.wb.open_project_search();
        // `⇥`: Tab (from the search's field to the folder's, and back).
        for (i, part) in query.split('⇥').enumerate() {
            if i > 0 {
                self.wb.edit_key(self.text, EditKey::Tab { shift: false });
            }
            if !part.is_empty() {
                self.wb.type_text(self.text, part);
            }
        }
        settle(200);
    }

    /// Other windows with these projects (made up; this window's is the first), numbered
    /// as the app numbers them.
    fn project_windows(&mut self, names: &[String]) {
        let windows = names
            .iter()
            .enumerate()
            .map(|(i, name)| workbench_model::ProjectWindow {
                window: 100 + i as u64,
                name: name.clone(),
                path: PathBuf::from(format!("/Users/me/work/{name}")),
                number: u8::try_from(i + 1).ok().filter(|n| *n <= 9),
                current: i == 0,
            })
            .collect();
        self.wb.set_project_windows(windows);
        self.frame();
    }

    fn split(&mut self, splits: &[(Split, bool)]) {
        for &(direction, move_tab) in splits {
            let active = self.wb.active_tab().unwrap_or(0);
            self.wb.split_tab(active, direction, move_tab);
        }
        tracing::info!(groups = self.wb.group_count(), "editor split");
        self.wb.sync_syntax();
        self.frame();
    }

    fn canvas(&mut self, mode: char, o: &Options) {
        self.wait(Duration::from_secs(10), |wb| !wb.indexing());
        self.wb.start_file_index();
        self.wait(Duration::from_secs(10), Workbench::poll_index);
        if let Some(line) = o.line {
            self.wb.place_cursor_at_line_end(self.text, Some(line));
        }
        self.wb.run_command(command_model::CommandId("view.canvas"));
        // Its graph, built by the module in the background.
        self.wait(Duration::from_secs(10), Workbench::poll_viewers);
        // Its mode, as its keys (`2`, `3`) send it.
        match mode {
            '2' => self.wb.viewer_command("files"),
            '3' => self.wb.viewer_command("code"),
            _ => false,
        };
        // The selected declaration edited on its card, the text typed at the start.
        if let Some(typed) = &o.canvas_edit {
            self.wb.viewer_command("edit");
            self.wb.type_text(self.text, typed);
        }
        // The ⌘K search open, with the text.
        if let Some(query) = &o.canvas_query {
            self.wb.viewer_command("search");
            self.wb.type_text(self.text, query);
        }
        self.frame();
    }

    fn tab_drag(&mut self, drag: TabDrag) {
        let TabDrag { from, to, drop } = drag;
        self.frame();
        self.wb.mouse_down(self.text, from, false, false, false, 1);
        let middle = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
        self.wb.mouse_drag(self.text, middle);
        self.wb.mouse_drag(self.text, to);
        if drop {
            self.wb.mouse_up(self.text);
            tracing::info!(
                groups = self.wb.group_count(),
                tabs = self.wb.tab_count(),
                "tab dropped"
            );
        }
    }

    fn compare(&mut self, left: &Path, right: &Path, merge: bool) {
        compare::compare_files(self.wb, left, right);
        // Its differences, found on a thread.
        self.wait_for_viewers(Duration::from_secs(10));
        if merge {
            // Compose mode, the first difference taken from the right.
            self.wb.viewer_command("merge");
            self.wb.viewer_command("take_right");
        }
    }

    /// The mouse over the first place of `word` in the active file (its second character:
    /// inside the word, past a `$`).
    fn hover_word(&mut self, word: &str) {
        use workbench_model::WorkbenchHost;
        let Some(text) = self.wb.active_file().map(|f| f.text.to_string()) else { return };
        let Some(at) = text.find(word).map(|i| i + 1) else {
            tracing::error!("`{word}` isn't in the active file");
            return;
        };
        workbench_model::EditorHost::select_active(self.wb, at..at);
        self.frame();
        let Some(caret) = self.wb.caret_rect(self.text) else { return };
        let p = Point::new(caret.min_x() + 1.0, caret.min_y() + caret.size.height / 2.0);
        // The caret elsewhere (its line not lit under the card), the view where it is.
        let end = text.len();
        workbench_model::EditorHost::select_active(self.wb, end..end);
        self.hover(p);
    }

    fn hover(&mut self, p: Point) {
        // A paint first, so the hover areas (tabs etc.) exist.
        self.frame();
        self.wb.sync_syntax();
        self.frame();
        self.wb.hover(p);
        self.wb.update_error_tip(self.text, p);
        self.wb.update_code_hover(self.text, p);
        // Transitions (e.g. a preview appearing) finish before the capture.
        settle(600);
        self.frame();
        settle(200);
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
#[path = "../tests/unit/snapshot.rs"]
mod tests;
