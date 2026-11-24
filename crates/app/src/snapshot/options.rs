//! The `EDITOR_SNAPSHOT_*` variables read into [`Options`]: names, flags, points, splits,
//! drags.

use super::*;

pub(super) fn var(name: &str) -> Option<String> {
    std::env::var(format!("EDITOR_SNAPSHOT_{name}")).ok()
}

pub(super) fn flag(name: &str) -> bool {
    var(name).is_some()
}

pub(super) fn parsed<T: FromStr>(name: &str) -> Option<T> {
    var(name)?.parse().ok()
}

/// `x,y`.
pub(super) fn point(name: &str) -> Option<Point> {
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
pub(super) fn parse_splits(value: &str) -> Vec<(Split, bool)> {
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
pub(super) fn parse_tab_drag(value: &str) -> Option<TabDrag> {
    let drop = value.ends_with('!');
    let v: Vec<f32> =
        value.trim_end_matches('!').split(',').filter_map(|n| n.trim().parse().ok()).collect();
    let [x1, y1, x2, y2] = v[..] else { return None };
    Some(TabDrag { from: Point::new(x1, y1), to: Point::new(x2, y2), drop })
}

/// `left,right[,merge]`.
pub(super) fn parse_compare(value: &str) -> Option<(PathBuf, PathBuf, bool)> {
    let parts: Vec<&str> = value.split(',').collect();
    let [left, right, rest @ ..] = &parts[..] else { return None };
    Some((PathBuf::from(left), PathBuf::from(right), rest.first() == Some(&"merge")))
}
