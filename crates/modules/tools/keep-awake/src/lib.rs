//! Keep awake as a module: a button on the right strip, just above Focus mode's, that keeps
//! the computer from going to sleep while it's on (lit then; also the `keep_awake.toggle`
//! command, its shortcut chosen in the keymap) — an AI working in a terminal, transfers, a
//! long indexing go on while nobody touches anything. Only the system's sleep is held off:
//! the screen may still turn off or lock. While it's kept awake and untouched for a while,
//! the window dims gently and a small mark drifts over it ([`screen`]: nothing stays hours
//! in one place); any input brings it back. Its parts: [`keeper`] (the system's hold, one
//! for the whole app), [`settings`] (`[keep_awake]`, its page), [`button`] (its button and
//! its menu), [`screen`] (the dimmed screen), [`watcher`] (only while work runs, if the user
//! chose so). Off by default: nothing is held until the user turns it on; while it holds,
//! the status bar says so (a click turns it off).

use power::Mode;

pub mod button;
pub mod keeper;
pub mod screen;
pub mod settings;
pub mod tasks;
pub mod watcher;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use command_model::{CommandId, CommandSpec};
use contracts::scenes::ShowCalmScene;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{ToolWindowProvider, Viewer, ViewerKind, ViewerProvider};
use workbench_model::{
    CommandContribution, CommandHandler, IdleAction, Indicator, MenuContribution, MenuPlace,
    MenuRow, ModuleThreads, Notification, StatusIndicator, SystemIndicator, SystemSign,
    WorkbenchHost,
};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tools.keep_awake"),
    name: "Keep awake",
    description: "The computer doesn't go to sleep while it's on (work running in a terminal, transfers)",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "sun", ..extensions::ModuleInfo::BUILT_IN },
};

/// Turns it on or off.
pub const TOGGLE: CommandId = CommandId("keep_awake.toggle");
/// Dims the screen now (while kept awake): what a while without input does.
pub const DIM_NOW: CommandId = CommandId("keep_awake.dim");
/// Only while work runs, or always while on (switches between the two).
pub const WHILE_WORKING: CommandId = CommandId("keep_awake.while_working");
/// Its page in the settings.
pub const SETTINGS: CommandId = CommandId("keep_awake.settings");
/// The screen kept on too, or no longer.
pub const SCREEN: CommandId = CommandId("keep_awake.screen");
/// A calm scene over the whole screen instead of dimming it, or no longer.
pub const SCENE: CommandId = CommandId("keep_awake.scene");
/// The extension the dimmed screen's view is opened by.
pub const EXTENSION: &str = "view-keep-awake-screen";
/// Its button's icon (contributed: Lucide's `coffee`).
pub const ICON: &str = "coffee";
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 2v2"/><path d="M14 2v2"/><path d="M16 8a1 1 0 0 1 1 1v8a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4V9a1 1 0 0 1 1-1h14a4 4 0 1 1 0 8h-1"/><path d="M6 2v2"/></svg>"##;

#[derive(Debug, Default)]
pub struct KeepAwakeModule;

impl Module for KeepAwakeModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(button::KeepAwakeButton));
        ctx.contribute(CommandContribution {
            // No shortcut of its own: one is chosen in the keymap.
            spec: CommandSpec { id: TOGGLE, label: "Keep Awake", menu: None, shortcuts: &[] },
            icon: Some(ICON),
            handler: Arc::new(Toggle),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec { id: DIM_NOW, label: "Dim the Screen", menu: None, shortcuts: &[] },
            icon: Some(ICON),
            handler: Arc::new(DimNow),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: WHILE_WORKING,
                label: "Keep Awake Only While Work Runs",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("activity"),
            handler: Arc::new(WhileWorking),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SCREEN,
                label: "Keep the Screen On Too",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("monitor"),
            handler: Arc::new(KeepScreen),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SCENE,
                label: "Full-Screen Scene Instead of Dimming",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("moon"),
            handler: Arc::new(ShowScene),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SETTINGS,
                label: "Keep Awake Settings…",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("settings"),
            handler: Arc::new(OpenSettings),
        });
        // Its button's right-click menu: the same commands.
        const STRIP: &[MenuPlace] = &[MenuPlace::ToolStrip];
        ctx.contribute(MenuContribution {
            id: button::TOOL,
            label_key: "keep_awake.title",
            icon: ICON,
            places: STRIP,
            rows: vec![
                MenuRow::Command(TOGGLE, STRIP),
                MenuRow::Command(WHILE_WORKING, STRIP),
                MenuRow::Command(SCREEN, STRIP),
                MenuRow::Command(SCENE, STRIP),
                MenuRow::Separator,
                MenuRow::Command(DIM_NOW, STRIP),
                MenuRow::Separator,
                MenuRow::Command(SETTINGS, STRIP),
            ],
        });
        // While it holds: said in the status bar, in every window, and outside them (by the
        // system's clock, on the app's icon).
        ctx.contribute::<Arc<dyn StatusIndicator>>(Arc::new(Awake));
        ctx.contribute::<Arc<dyn SystemIndicator>>(Arc::new(Awake));
        ctx.contribute(ModuleThreads(&[watcher::THREAD]));
        // Its row in the Task Manager while it's on.
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(tasks::Reporter));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::KeepAwakeSettings,
        ));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(ScreenProvider));
        // The screen dims after the minutes without input its settings say, while kept awake.
        ctx.contribute::<Arc<dyn IdleAction>>(Arc::new(IdleDim));
        Ok(())
    }
}

/// [`TOGGLE`]: on, or off — saved (it stays as chosen at the next start), the hold taken or
/// let go at once. The system refusing: said, and it stays off.
struct Toggle;

impl CommandHandler for Toggle {
    fn switched(&self, _host: &dyn WorkbenchHost) -> Option<bool> {
        Some(settings::config().on)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(user) = host.user_settings() else { return false };
        let on = !settings::config().on;
        settings::save_on(user, on);
        if let Err(why) = settings::apply(user) {
            settings::save_on(user, false);
            let _ = settings::apply(user);
            let text = i18n::tr!("keep_awake.failed", why = why.to_string().as_str());
            host.notify(Notification::warning(text));
        }
        true
    }
}

/// [`WHILE_WORKING`]: only while work runs, or always while on — saved, followed at once.
struct WhileWorking;

impl CommandHandler for WhileWorking {
    fn switched(&self, _host: &dyn WorkbenchHost) -> Option<bool> {
        Some(settings::config().when == Mode::WhileWorking)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(user) = host.user_settings() else { return false };
        let working = settings::config().when != Mode::WhileWorking;
        settings::save_while_working(user, working);
        if let Err(why) = settings::apply(user) {
            let text = i18n::tr!("keep_awake.failed", why = why.to_string().as_str());
            host.notify(Notification::warning(text));
        }
        let key = if working { "keep_awake.now_working" } else { "keep_awake.now_always" };
        host.notify(Notification::info(i18n::t(key)));
        true
    }
}

/// [`SCREEN`]: the screen kept on too, or no longer — saved, held again at once if held.
struct KeepScreen;

impl CommandHandler for KeepScreen {
    fn switched(&self, _host: &dyn WorkbenchHost) -> Option<bool> {
        Some(settings::config().screen)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(user) = host.user_settings() else { return false };
        let screen = !settings::config().screen;
        settings::save_screen(user, screen);
        if let Err(why) = settings::apply(user) {
            let text = i18n::tr!("keep_awake.failed", why = why.to_string().as_str());
            host.notify(Notification::warning(text));
        }
        let key = if screen { "keep_awake.now_screen" } else { "keep_awake.now_no_screen" };
        host.notify(Notification::info(i18n::t(key)));
        true
    }
}

/// [`SCENE`]: a calm scene instead of dimming, or no longer. Offered only while some module
/// answers [`ShowCalmScene`] (which one, it doesn't know).
struct ShowScene;

impl CommandHandler for ShowScene {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        workbench_model::presence::request::<ShowCalmScene>()
    }

    fn switched(&self, _host: &dyn WorkbenchHost) -> Option<bool> {
        Some(settings::config().scene)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(user) = host.user_settings() else { return false };
        settings::save_scene(user, !settings::config().scene);
        settings::apply(user).is_ok()
    }
}

/// [`SETTINGS`]: the settings, at its page.
struct OpenSettings;

impl CommandHandler for OpenSettings {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.open_settings_page("keep_awake")
    }
}

/// Its sign in the status bar while the computer is kept awake: why (turned on, or work
/// running), and a click turns it off.
#[derive(Debug)]
struct Awake;

impl StatusIndicator for Awake {
    fn indicator(&self) -> Option<Indicator> {
        if !keeper::keeper().is_held() {
            return None;
        }
        let why = match settings::config().mode() {
            Mode::WhileWorking => "keep_awake.indicator_working",
            _ => "keep_awake.indicator_on",
        };
        Some(Indicator {
            icon: ICON,
            label: i18n::t("keep_awake.indicator"),
            tooltip: i18n::t(why),
            command: Some(TOGGLE),
        })
    }
}

/// Its sign outside the windows while the computer is kept awake: seen with the app behind
/// others (its menu turns it off).
impl SystemIndicator for Awake {
    fn sign(&self) -> Option<SystemSign> {
        if !keeper::keeper().is_held() {
            return None;
        }
        let why = match settings::config().mode() {
            Mode::WhileWorking => "keep_awake.system_working",
            _ => "keep_awake.system_on",
        };
        Some(SystemSign {
            icon: ICON,
            tooltip: kernel::brand::named(&i18n::t(why)),
            action: Some((i18n::t("keep_awake.turn_off"), TOGGLE)),
        })
    }
}

/// [`DIM_NOW`]: the dimmed screen over every window, as a modal (nothing behind it gets
/// anything; any input, in any window, closes it everywhere) — or, if the user chose so and a module answers, a calm
/// scene asked for instead. Only while kept awake: otherwise the system's own screen saver
/// is the one to do it.
struct DimNow;

impl CommandHandler for DimNow {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        keeper::keeper().is_held()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if !keeper::keeper().is_held() {
            return false;
        }
        // Nobody answers, or it didn't open: the screen dims as usual.
        if settings::config().scene && host.ask(ShowCalmScene) == Some(true) {
            return true;
        }
        host.open_overlay_everywhere(EXTENSION, &())
    }
}

/// The dimmed screen's view, opened bare over the window.
#[derive(Debug)]
struct ScreenProvider;

impl ViewerProvider for ScreenProvider {
    fn id(&self) -> &'static str {
        "keep-awake-screen"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(screen::ScreenView::new(settings::config().care)))
    }
}

/// Dims the screen after the minutes without input the settings say — while kept awake
/// only (asked at each frame: cheap).
#[derive(Debug)]
pub struct IdleDim;

impl IdleAction for IdleDim {
    fn after(&self) -> Option<Duration> {
        if !keeper::keeper().is_held() {
            return None;
        }
        settings::config().care.dim_after
    }

    fn command(&self) -> CommandId {
        DIM_NOW
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
