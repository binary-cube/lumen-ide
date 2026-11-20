//! Relax mode as a module: a button on the right strip, just above the Task Manager's,
//! that covers the whole window with a calm animated scene (the panels aren't drawn under
//! it, and are back as they were when it ends); `esc` — the key, or its cap in the top
//! right corner, shown while the mouse moves — leaves it. The scenes ([`scene::SCENES`])
//! are drawn with the renderer's quads and images (3D ones projected on the CPU), at the
//! pace chosen (30 frames a second at first, or 60). Nothing of it exists while it's closed but its
//! settings; it may start by itself after a while without input ([`IdleStart`]).

pub mod art;
pub mod preview;
pub mod scene;
pub mod settings;
mod view;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use command_model::{CommandId, CommandSpec};
use contracts::scenes::ShowCalmScene;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{
    Dock, ToolContent, ToolWindow, ToolWindowProvider, Viewer, ViewerKind, ViewerProvider,
};
use workbench_model::{CommandContribution, CommandHandler, IdleAction, WorkbenchHost};

pub use view::{RelaxView, SceneClock};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tools.relax"),
    name: "Relax mode",
    description: "A calm animated scene over the whole window (stars, a sea of light, a ribbon); Esc to leave",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "moon", ..extensions::ModuleInfo::BUILT_IN },
};

/// Starts Relax mode.
pub const START: CommandId = CommandId("relax.start");
/// Its button on the right strip.
pub const TOOL: &str = "relax";
/// The extension its view is opened by.
pub const EXTENSION: &str = "view-relax";
/// Its button's icon (contributed: Lucide's `moon-star`).
pub const ICON: &str = "moon-star";
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9"/><path d="M20 3v4"/><path d="M22 5h-4"/></svg>"##;

#[derive(Debug, Default)]
pub struct RelaxModule;

impl Module for RelaxModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        // Its button in the right strip's bottom group, right above the Task Manager's
        // (order 20): a command, no window.
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: TOOL,
            icon: ICON,
            title_key: "relax.title",
            dock: Dock::Right,
            order: 10,
            strip_end: true,
            group: "",
            toggle: Some(START.0),
            content: ToolContent::Command { command: START.0, tab: None },
        }));
        ctx.contribute(CommandContribution {
            spec: CommandSpec { id: START, label: "Relax Mode", menu: None, shortcuts: &[] },
            icon: Some(ICON),
            handler: Arc::new(Start),
        });
        // Its own page in the settings (Interface).
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::RelaxSettings,
        ));
        // Its scenes' pictures on its settings page.
        ctx.contribute::<Arc<dyn ui_components::CardPreview>>(Arc::new(
            preview::ScenePreviews::default(),
        ));
        // A calm scene asked for by anyone: the view on the whole screen until it's left —
        // once a screen, the app choosing the window (several instances on one screen
        // don't each start one).
        ctx.contribute(workbench_model::Answers::of(|_: ShowCalmScene, host| {
            host.open_full_screen_everywhere(EXTENSION)
        }));
        // Started after the minutes without input its settings say (off by default).
        ctx.contribute::<Arc<dyn IdleAction>>(Arc::new(IdleStart));
        Ok(())
    }
}

/// Its view, opened bare over the window.
#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        "relax"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(RelaxView::new(settings::config())))
    }
}

/// [`START`]: the scene over the window, as a modal (nothing behind it gets anything).
struct Start;

impl CommandHandler for Start {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.open_overlay_everywhere(EXTENSION, &())
    }
}

/// Starts it after the minutes without input the settings say (off by default).
#[derive(Debug)]
pub struct IdleStart;

impl IdleAction for IdleStart {
    fn after(&self) -> Option<Duration> {
        settings::config().idle_minutes.map(|m| Duration::from_secs(u64::from(m) * 60))
    }

    fn command(&self) -> CommandId {
        START
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
