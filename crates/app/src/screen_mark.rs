//! The app's mark on calm screens (a [`ui_workbench::screen_widgets::ScreenWidget`] of the
//! app's own, in the bottom left corner): its logo, name and version on a pane of glass.
//! The logo is decoded on a worker (`screen-mark`) when a screen opens, at the size it's
//! drawn, and goes with the screen; nothing shows before it's in (no stand-in flashing).

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use render_api::ImageData;
use text_layout::TextSystem;
use ui_workbench::ds::{self, Ctx};
use ui_workbench::screen_widgets::{
    ScreenWidget, WIDGET_HEIGHT, WidgetContext, WidgetPaint, WidgetPlace, WidgetView,
};

/// The logo's side on its pane, and the pane's padding (logical pixels).
const LOGO: f32 = 64.0;
const PAD: f32 = 16.0;
/// Between the logo and the words.
const GAP: f32 = 14.0;

#[derive(Debug)]
pub struct ScreenMark;

impl ScreenWidget for ScreenMark {
    fn id(&self) -> &'static str {
        "app.mark"
    }

    fn place(&self) -> WidgetPlace {
        WidgetPlace::BottomLeft
    }

    fn open(&self, _ctx: &WidgetContext) -> Option<Box<dyn WidgetView>> {
        Some(Box::new(Mark {
            logo: Logo::Unasked,
            version: format!("v{}", kernel::brand::version()),
        }))
    }
}

/// The app's logo: not asked for yet, being decoded (at a side), or decoded (`None`: it
/// couldn't be).
enum Logo {
    Unasked,
    Decoding(u32, Receiver<Option<ImageData>>),
    Ready(u32, Option<ImageData>),
}

struct Mark {
    logo: Logo,
    version: String,
}

impl Mark {
    /// The logo at `side` pixels: decoded once (again for another side, a zoom); `None`
    /// while it's being decoded, then the picture (`Some(None)`: it couldn't be).
    fn logo(&mut self, side: u32) -> Option<Option<&ImageData>> {
        let asked = match &self.logo {
            Logo::Unasked => None,
            Logo::Decoding(s, _) | Logo::Ready(s, _) => Some(*s),
        };
        if asked != Some(side) {
            let (tx, rx) = std::sync::mpsc::channel();
            kernel::work::run("screen-mark", move || {
                let logo = image_decode::decode_fitting(kernel::brand::ICON_PNG, side)
                    .map(|d| ImageData::new(d.width, d.height, d.pixels))
                    .map_err(|e| tracing::warn!("the logo couldn't be decoded: {e}"))
                    .ok();
                let _ = tx.send(logo);
            });
            self.logo = Logo::Decoding(side, rx);
        }
        if let Logo::Decoding(side, rx) = &self.logo
            && let Ok(logo) = rx.try_recv()
        {
            self.logo = Logo::Ready(*side, logo);
        }
        match &self.logo {
            Logo::Ready(_, logo) => Some(logo.as_ref()),
            _ => None,
        }
    }

    /// The name as on the app's card: capitals, widely spaced, an accent shadow under them.
    fn wordmark<'a>(ds: &'a Ctx<'_>) -> ds::Wordmark<'a> {
        ds::Wordmark {
            family: &ds.palette.ui_font,
            size: ds.px(NAME),
            tracking: ds.px(NAME * 0.3),
            accent: ds.palette.accent,
            opacity: ds.opacity,
        }
    }
}

/// The name's size (logical pixels), and the version's pill height under it, with the gap
/// between them (as on the app's card).
const NAME: f32 = 24.0;
const PILL: f32 = 24.0;
const UNDER: f32 = 10.0;

impl WidgetView for Mark {
    /// As tall as every widget; as wide as its logo and its name.
    fn size(&mut self, text: &mut TextSystem, ds: &Ctx<'_>, _room: f32) -> Option<(f32, f32)> {
        let side = ds.px(LOGO).ceil() as u32;
        self.logo(side)?;
        let words = Self::wordmark(ds).width(text, kernel::brand::NAME);
        let w = ds.px(PAD) + ds.px(LOGO) + ds.px(GAP) + words + ds.px(PAD + 8.0);
        Some((w.ceil(), ds.px(WIDGET_HEIGHT)))
    }

    fn paint(&mut self, p: WidgetPaint<'_, '_>) {
        let ds = p.ds;
        let side = ds.px(LOGO).ceil() as u32;
        let Some(logo) = self.logo(side).map(|l| l.cloned()) else { return };
        let r = p.rect;
        ui_workbench::screen_widgets::glass_pane(p.layer, ds, r, p.look);
        let s = ds.px(LOGO);
        let at = kernel::Rect::new(
            (r.min_x() + ds.px(PAD)).round(),
            (r.min_y() + (r.size.height - s) / 2.0).round(),
            s,
            s,
        );
        // Not decoded: the logo's place left empty, the words still there.
        if let Some(image) = &logo {
            p.layer.images.push(render_api::ImageInstance {
                image: image.clone(),
                bounds: at,
                opacity: p.look.opacity.clamp(0.0, 1.0),
            });
        }
        // The name and the version's pill under it, together in the middle of its height.
        let block = ds.px(NAME) + ds.px(UNDER) + ds.px(PILL);
        let top = r.min_y() + (r.size.height - block) / 2.0;
        let x = (at.max_x() + ds.px(GAP)).round();
        let baseline = (top + ds.px(NAME) * 0.86).round();
        ds::wordmark(p.layer, p.text, &Self::wordmark(ds), kernel::brand::NAME, (x, baseline));
        let pill_top = (top + ds.px(NAME) + ds.px(UNDER)).round();
        let family = &ds.palette.ui_font;
        ds::version_pill(
            p.layer,
            p.text,
            family,
            &self.version,
            (x, pill_top),
            (ds.scale, ds.opacity),
        );
    }
}

/// Registers the app's own widgets for calm screens.
pub fn register() {
    ui_workbench::screen_widgets::register(Arc::new(ScreenMark));
}

#[cfg(test)]
#[path = "../tests/unit/screen_mark.rs"]
mod tests;
