//! Focus mode as a module: a button on the right strip, just above Relax mode's, that
//! turns it on and off (lit while on; also the `focus.toggle` command, its shortcut chosen
//! in the keymap). While on, the area being worked in stays in color and the rest of the
//! window turns gray — through the app's region emphasis ([`workbench_model::RegionEmphasis`]:
//! [`policy::FocusEmphasis`] says how, from its settings) — and the theme's animated
//! effects stand still. Its parts: [`state`] (on or off), [`settings`] (`[focus]`, its page),
//! [`policy`] (what it asks of the window), [`button`] (its button).

pub mod button;
pub mod policy;
pub mod settings;
pub mod state;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::ToolWindowProvider;
use workbench_model::{CommandContribution, CommandHandler, RegionEmphasis, WorkbenchHost};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tools.focus"),
    name: "Focus mode",
    description: "The area being worked in stays in color, the rest of the window turns gray",
    depends_on: &[],
};

/// Turns it on or off.
pub const TOGGLE: CommandId = CommandId("focus.toggle");
/// Its button's icon (contributed: Lucide's `contrast`, its half filled): one part in
/// color, the rest without — what the mode does.
pub const ICON: &str = "contrast";
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 2a10 10 0 0 1 0 20z" fill="#000"/></svg>"##;

#[derive(Debug, Default)]
pub struct FocusModule;

impl Module for FocusModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(button::FocusButton));
        ctx.contribute(CommandContribution {
            // No shortcut of its own: one is chosen in the keymap.
            spec: CommandSpec { id: TOGGLE, label: "Focus Mode", menu: None, shortcuts: &[] },
            icon: Some(ICON),
            handler: Arc::new(Toggle),
        });
        ctx.contribute::<Arc<dyn RegionEmphasis>>(Arc::new(policy::FocusEmphasis));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::FocusSettings,
        ));
        Ok(())
    }
}

/// [`TOGGLE`]: on, or off.
struct Toggle;

impl CommandHandler for Toggle {
    fn run(&self, _host: &mut dyn WorkbenchHost) -> bool {
        state::toggle();
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
