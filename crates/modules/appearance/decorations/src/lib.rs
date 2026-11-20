//! The themes' decorations: a theme declaring layers of them (plan 41) gets them drawn by
//! this module — a garland (an evergreen strand, a wire or a ribbon with bulbs, beads or
//! flags) along the window's edges, particles drifting behind the content (snow, leaves,
//! petals, stars, sparkles, confetti, rain), ornaments in the top corners, a glow along the
//! edges —, and so does any theme the user puts their own over (a preset, or the layers
//! saved from the studio). Contributed as a [`DecorationsProvider`]: the app asks it for a
//! window's decorations (nothing is made when there's nothing to draw) and lets them go when
//! the theme changes. Their settings are its own ([`settings`]).

pub mod art;
pub mod fx;
pub mod glyphs;
pub mod paint;
pub mod settings;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use theming::decorations::Decorations;
use ui_components::decorations::{DecorationMode, DecorationsProvider, WindowDecorations};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("decorations"),
    name: "Theme decorations",
    description: "What a theme draws besides its colors (a garland, particles, corner ornaments, a glow), and your own over any theme",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "sparkles", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct DecorationsModule;

impl Module for DecorationsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn DecorationsProvider>>(Arc::new(Provider));
        // Whoever edits them (a theme studio) changes them through these.
        for answer in settings::answers() {
            ctx.contribute(answer);
        }
        // Matrix's characters, as icons (drawn only once an effect asks for them).
        for glyph in glyphs::all() {
            ctx.contribute(*glyph);
        }
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::DecorationSettings,
        ));
        Ok(())
    }
}

/// Draws every kind the themes may declare.
#[derive(Debug)]
pub struct Provider;

/// A theme's decorations with the user's own over or instead of them (the same `Arc` when
/// the user has none).
pub fn with_overlay(
    decorations: &Arc<Decorations>,
    overlay: &settings::Overlay,
) -> Arc<Decorations> {
    match overlay.layers(decorations.dark) {
        Some(layers) => Arc::new(decorations.with_overlay(&layers, overlay.with)),
        None => Arc::clone(decorations),
    }
}

impl DecorationsProvider for Provider {
    fn create(&self, decorations: &Arc<Decorations>) -> Option<Box<dyn WindowDecorations>> {
        if settings::mode() == DecorationMode::Off {
            return None;
        }
        let drawn = with_overlay(decorations, &settings::overlay());
        if drawn.is_empty() {
            return None;
        }
        Some(Box::new(paint::Painter::new(drawn)))
    }

    fn preview(&self, decorations: &Arc<Decorations>) -> Option<Box<dyn WindowDecorations>> {
        (!decorations.is_empty()).then(|| {
            Box::new(paint::Painter::new(Arc::clone(decorations))) as Box<dyn WindowDecorations>
        })
    }

    fn fps(&self) -> u32 {
        settings::fps()
    }

    fn idle(&self) -> Option<std::time::Duration> {
        settings::idle()
    }

    fn respects_system(&self) -> bool {
        settings::follows_system()
    }

    fn mode(&self) -> DecorationMode {
        settings::mode()
    }

    fn version(&self) -> u64 {
        settings::version()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
