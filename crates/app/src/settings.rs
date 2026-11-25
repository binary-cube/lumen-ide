//! Every user setting, declared once: its place in `settings.toml`, its type, its default
//! and which values are valid. Read and written through the one settings service
//! (`crate::config::settings()`), never by section / key strings elsewhere.

use settings_model::Setting;

/// How much the log writes (`[log] level`): `off`, `error`, `warn`, `info`, `debug`.
pub const LOG_LEVEL: Setting<String> = Setting::new("log", "level", || "info".to_owned())
    .valid(|v| crate::logs::LEVELS.iter().any(|(id, _)| id == v));

/// The log's folder (`[log] dir`); empty: `<config>/logs/`.
pub const LOG_DIR: Setting<String> = Setting::new("log", "dir", String::new);

/// Other levels for some modules (`[log] modules`: `database_explorer=debug, vcs_git=warn`).
pub const LOG_MODULES: Setting<String> = Setting::new("log", "modules", String::new);

/// Saving automatically once typing pauses (`[editor] autosave`).
pub const AUTOSAVE: Setting<bool> = Setting::new("editor", "autosave", || true);

/// How many of the app's past sessions are kept (the home screen lists them).
pub const SESSIONS_KEPT: Setting<i64> = Setting::new("sessions", "kept", || 10);
/// How many windows a session's "Reopen" opens at most (0: all), one after the other.
pub const SESSIONS_REOPEN: Setting<i64> = Setting::new("sessions", "reopen", || 10);

/// The status bar's resources window pinned open (`[status] resources_pinned`): it stays
/// over the bar, across restarts, until unpinned.
pub const RESOURCES_PINNED: Setting<bool> = Setting::new("status", "resources_pinned", || false);

/// A line break at the end of saved files (`[editor] final_newline`).
pub const FINAL_NEWLINE: Setting<bool> = Setting::new("editor", "final_newline", || true);

/// Lines longer than a view go on in rows under them, wherever text is shown (`[editor]
/// word_wrap`, off by default).
pub const WORD_WRAP: Setting<bool> = Setting::new("editor", "word_wrap", || false);

/// Parameter names shown inside the code at calls (inlay hints, `[editor] inlay_hints`).
pub const INLAY_HINTS: Setting<bool> = Setting::new("editor", "inlay_hints", || true);

/// The usages and implementations written after declarations (code lenses,
/// `[editor] code_lens`).
pub const CODE_LENS: Setting<bool> = Setting::new("editor", "code_lens", || true);

/// The bulb in the gutter when the cursor rests where there are actions
/// (`[editor] action_bulb`; off: ⌥⏎ still lists them).
pub const ACTION_BULB: Setting<bool> = Setting::new("editor", "action_bulb", || true);

/// ⌘-click follows a document's links — URLs, paths in quotes (`[editor] document_links`;
/// off: only declarations).
pub const DOCUMENT_LINKS: Setting<bool> = Setting::new("editor", "document_links", || true);

/// The app's menus in a drawer from the window's bar (`[appearance] window_menu`): on by
/// default; a choice on macOS only (beside its own menu bar), elsewhere always there (the
/// only menu).
pub const WINDOW_MENU: Setting<bool> = Setting::new("appearance", "window_menu", || true);

/// The theme chosen (`[appearance] theme`; empty: none, the system's look decides).
pub const THEME: Setting<String> = Setting::new("appearance", "theme", String::new);

/// The values of the themes' own settings (`[appearance] theme_options`): one list for
/// every theme, `"<theme>/<option>=<value>"` (plan 41e; what a theme doesn't take is ignored).
pub const THEME_OPTIONS: Setting<Vec<String>> =
    Setting::new("appearance", "theme_options", Vec::new)
        .valid(|list| list.iter().all(|e| theming::options::valid_entry(e)));

/// The theme follows the system's light / dark look (`[appearance] follow_system`).
pub const THEME_FOLLOWS_SYSTEM: Setting<bool> =
    Setting::new("appearance", "follow_system", || true);

/// "From the theme" for the backdrop's numbers below.
pub const FROM_THEME: i64 = -1;

/// The backdrop under the windows over the editor (`[appearance] backdrop`: theme, simple,
/// glass).
pub const BACKDROP_STYLE: Setting<String> =
    Setting::new("appearance", "backdrop", || "theme".to_owned())
        .valid(|s| s == "theme" || theming::backdrop::BackdropKind::parse(s).is_some());

/// How much of its color, percent (`[appearance] backdrop_opacity`; -1: the theme's).
pub const BACKDROP_OPACITY: Setting<i64> =
    Setting::new("appearance", "backdrop_opacity", || FROM_THEME)
        .valid(|o| *o == FROM_THEME || (0..=85).contains(o));

/// How far Glass blurs, logical pixels (`[appearance] backdrop_blur`; -1: the theme's).
pub const BACKDROP_BLUR: Setting<i64> = Setting::new("appearance", "backdrop_blur", || FROM_THEME)
    .valid(|b| *b == FROM_THEME || (0..=theming::backdrop::MAX_BLUR as i64).contains(b));

/// The floating panels' glass (`[appearance] floating`: theme, simple, glass) — the menu
/// drawer's.
pub const FLOATING_STYLE: Setting<String> =
    Setting::new("appearance", "floating", || "theme".to_owned())
        .valid(|s| s == "theme" || theming::backdrop::BackdropKind::parse(s).is_some());

/// A glass panel's opacity, percent (`[appearance] floating_opacity`; -1: the theme's).
pub const FLOATING_OPACITY: Setting<i64> =
    Setting::new("appearance", "floating_opacity", || FROM_THEME)
        .valid(|o| *o == FROM_THEME || (60..=100).contains(o));

/// How far a glass panel blurs, logical pixels (`[appearance] floating_blur`; -1: the
/// theme's).
pub const FLOATING_BLUR: Setting<i64> = Setting::new("appearance", "floating_blur", || FROM_THEME)
    .valid(|b| *b == FROM_THEME || (0..=theming::backdrop::MAX_BLUR as i64).contains(b));

/// The caret: the theme's (`false`) or the user's own below (`[caret] own`).
pub const CARET_OWN: Setting<bool> = Setting::new("caret", "own", || false);

/// The user's caret shape (`[caret] shape`: bar, block, underline).
pub const CARET_SHAPE: Setting<String> = Setting::new("caret", "shape", || "bar".to_owned())
    .valid(|s| theming::caret::CaretShape::parse(s).is_some());

/// The user's caret thickness, logical pixels (`[caret] width`).
pub const CARET_WIDTH: Setting<i64> =
    Setting::new("caret", "width", || 2).valid(|w| (1..=4).contains(w));

/// The user's caret blinking (`[caret] blink`: none, classic, smooth).
pub const CARET_BLINK: Setting<String> = Setting::new("caret", "blink", || "classic".to_owned())
    .valid(|s| theming::caret::Blink::parse(s).is_some());

/// The user's caret going to a new place (`[caret] motion`: jump, glide).
pub const CARET_MOTION: Setting<String> = Setting::new("caret", "motion", || "jump".to_owned())
    .valid(|s| theming::caret::Motion::parse(s).is_some());

/// The user's caret halo, percent (`[caret] glow`).
pub const CARET_GLOW: Setting<i64> =
    Setting::new("caret", "glow", || 0).valid(|g| (0..=100).contains(g));

/// The user's caret trail, milliseconds (`[caret] trail`).
pub const CARET_TRAIL: Setting<i64> =
    Setting::new("caret", "trail", || 0).valid(|t| (0..=1000).contains(t));

/// How a scrollbar answers the pointer (`[scrollbars] hover`: thumb — only the thumb thickens,
/// the default —, rail — it widens into its rail).
pub const SCROLLBAR_HOVER: Setting<String> =
    Setting::new("scrollbars", "hover", || "thumb".to_owned())
        .valid(|s| ui_workbench::ScrollbarHover::parse(s).is_some());

/// A scrollbar's thumb at rest, logical pixels (`[scrollbars] thickness`).
pub const SCROLLBAR_THICKNESS: Setting<i64> =
    Setting::new("scrollbars", "thickness", || 5).valid(|t| (3..=8).contains(t));

/// The thin line under a scrollbar's thumb (`[scrollbars] line`; off: the thumb alone).
pub const SCROLLBAR_LINE: Setting<bool> = Setting::new("scrollbars", "line", || true);

/// When an edge's scrollbar shows (`[scrollbars] visibility`: view — as each view chose —,
/// always, scrolling — only while it scrolls or is hovered).
pub const SCROLLBAR_VISIBILITY: Setting<String> =
    Setting::new("scrollbars", "visibility", || "view".to_owned())
        .valid(|s| ui_workbench::ScrollbarVisibility::parse(s).is_some());

/// The icon set's style (`[appearance] icons`): ours drawn as by hand (Sketch) by default;
/// a pack missing — its module off — falls back to ours.
pub const ICON_STYLE: Setting<String> =
    Setting::new("appearance", "icons", || icons::IconStyle::Sketch.id().to_owned())
        // A style's id or a pack's (packs are known once their modules load; one not there
        // any more falls back to ours).
        .valid(|id| {
            !id.is_empty()
                && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });

/// The file icons (`[appearance] file_icons`): `ours` (by language) or a file icon set's
/// id (one not there any more: ours); Symbols by default.
pub const FILE_ICONS: Setting<String> =
    Setting::new("appearance", "file_icons", || "symbols".to_owned()).valid(|id| {
        !id.is_empty()
            && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    });

/// The keyboard shortcuts preset (`[general] keymap`).
pub const KEYMAP: Setting<String> = Setting::new("general", "keymap", || "default".to_owned())
    .valid(|id| crate::keymaps::exists(id));

/// The user's own shortcuts (`[shortcuts]`: a command's id = its whole list, `[]` for none),
/// over the chosen preset's: only the commands they changed (written by hand for now; the
/// Shortcuts page writes them).
pub const SHORTCUTS: &str = "shortcuts";

/// The user's own shortcuts, as saved.
pub fn user_shortcuts() -> command_model::bindings::UserShortcuts {
    let section = crate::config::settings().section(SHORTCUTS).unwrap_or_default();
    let lists = section.iter().filter_map(|(id, value)| match value {
        settings_model::SettingValue::List(keys) => Some((id.clone(), keys.clone())),
        _ => None,
    });
    command_model::bindings::UserShortcuts(lists.collect())
}

/// Saves a command's own shortcuts (`None`: back to the preset's).
pub fn set_user_shortcuts(command: &str, shortcuts: Option<Vec<String>>) -> std::io::Result<()> {
    crate::config::settings().update(SHORTCUTS, &mut |section| match &shortcuts {
        Some(keys) => {
            section.insert(command.to_owned(), settings_model::SettingValue::List(keys.clone()));
        }
        None => {
            section.remove(command);
        }
    })
}

/// The UI language (`[general] language`).
pub const LANGUAGE: Setting<String> = Setting::new("general", "language", || "en".to_owned())
    .valid(|code| i18n::LANGUAGES.iter().any(|(c, _)| c == code));

/// How the modules' data loads (`[modules] loading`): `on-demand` — read on first use, let
/// go when unused for a while (saves memory) — or `startup` — everything read at startup in
/// the background and kept (first uses are instant).
pub const MODULE_LOADING: Setting<String> =
    Setting::new("modules", "loading", || ON_DEMAND.to_owned())
        .valid(|v| v == ON_DEMAND || v == STARTUP);
pub const ON_DEMAND: &str = "on-demand";
pub const STARTUP: &str = "startup";

/// The modules whose data is read at startup even `on-demand` (`[modules] preload`).
pub const MODULES_PRELOAD: Setting<Vec<String>> = Setting::new("modules", "preload", Vec::new);

/// The modules turned off (`[modules] disabled`); applied at the next start.
pub const MODULES_DISABLED: Setting<Vec<String>> = Setting::new("modules", "disabled", Vec::new);

/// The modules the user hid (`[modules] hidden`): they run, serve what others ask, but show
/// nothing of their own (no buttons, menus, commands; their viewers chosen last); applied at
/// the next start.
pub const MODULES_HIDDEN: Setting<Vec<String>> = Setting::new("modules", "hidden", Vec::new);

/// The modules hidden by default (their manifest says so) the user showed again
/// (`[modules] shown`).
pub const MODULES_SHOWN: Setting<Vec<String>> = Setting::new("modules", "shown", Vec::new);

/// Whether module `id` runs hidden, as the settings say: hidden by the user, or by default
/// and not shown again.
pub fn module_hidden(settings: &settings_model::SettingsService, id: &str) -> bool {
    settings.get(&MODULES_HIDDEN).iter().any(|h| h == id)
        || (crate::modules::hidden_by_default(id)
            && !settings.get(&MODULES_SHOWN).iter().any(|s| s == id))
}

/// `explicit`: only the modules in `[modules] enabled` start (`[modules] mode`; default
/// `auto`: all but the disabled ones).
pub const MODULES_MODE: Setting<String> =
    Setting::new("modules", "mode", || "auto".to_owned()).valid(|v| v == "auto" || v == "explicit");
pub const MODULES_ENABLED: Setting<Vec<String>> = Setting::new("modules", "enabled", Vec::new);

/// Where the notifications show (`[appearance] notifications`: `bottom-right`,
/// `bottom-center`, `top-center` — the default — or `top-right`).
pub const TOAST_POSITION: Setting<workbench_model::ToastPosition> =
    Setting::new("appearance", "notifications", workbench_model::ToastPosition::default);

/// The number a project prefers among the open ones (`[project] number` in its own settings:
/// 1–9, 0 none), chosen by the user; it takes it when it opens, if it's free.
pub const PROJECT_NUMBER: Setting<i64> =
    Setting::new("project", "number", || 0).valid(|n| (0..=9).contains(n));

/// The GPU that draws where the computer has several (`[graphics] gpu`): `system` (the
/// default: the system's choice — the user's setting for the app, its power policy),
/// `integrated` (saves the battery) or `dedicated` (stronger). Applied at the next start:
/// the GPU is chosen once, before the first window. The web engine draws on the same one.
pub const GPU: Setting<String> = Setting::new("graphics", "gpu", || "system".to_owned())
    .valid(|v| render_api::GpuPreference::parse(v).is_some());
