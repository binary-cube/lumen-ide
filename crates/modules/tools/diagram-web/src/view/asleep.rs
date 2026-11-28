//! A draw.io tab asleep (plan 101f): when and how often is the sleeping's
//! (`ui_components::sleep`); what's the tab's own is here.
//!
//! - What keeps it awake ([`Veto`]): no page (draw.io not installed, no file), something not
//!   written yet, a cell being typed into or a dialog open, an export waiting for its place.
//!   Quiet: since the diagram last changed (by the user, or on disk).
//! - Falling asleep: its page's picture asked for ([`WebPage::freeze`]), then its page let go
//!   (once the picture's copy of its frame is made, at the renderer's next frame). Where the
//!   user was is already kept ([`crate::state`]).
//! - Asleep: the picture at its own size (never stretched), dimmed, the card over it; a key
//!   or a click (or "Wake Up") wakes it — moving onto its tab doesn't.
//! - Coming back: a page made anew, given the diagram and where the user was; the picture and
//!   a "coming back" card stay until the page is drawn.

use super::*;
use ui_components::sleep::{
    self as views, Quiet, SleepKind, SleepPart, SleepPlace, SleepTexts, SleepWhy, Veto,
};

/// draw.io tabs, as a kind of view that can sleep: their hint, their words.
pub static DIAGRAMS: SleepKind = SleepKind {
    id: "diagram",
    viewer: crate::PROVIDER_ID,
    hint: Some(crate::SLEEP_HINT),
    texts: &TEXTS,
};

static TEXTS: SleepTexts = SleepTexts {
    asleep_title: "diagram_web.asleep_title",
    resuming: "diagram_web.resuming",
    by_hand: "diagram_web.asleep_by_hand",
    idle: "diagram_web.asleep_idle",
    freed: "diagram_web.asleep_freed",
    wake_up: "diagram_web.wake_up",
    how: "diagram_web.asleep_how",
    sleep_now: "diagram_web.sleep_now",
    going_to_sleep: "diagram_web.going_to_sleep",
    waking_up: "diagram_web.waking_up",
    cant: "diagram_web.cant_sleep",
    cant_sleeping: "diagram_web.cant_sleep_sleeping",
    cant_waking: "diagram_web.cant_sleep_waking",
};

/// No page to put to sleep (draw.io not installed, no file, it can't be shown here).
pub(crate) const NO_PAGE: Veto = Veto {
    id: "no page",
    menu: "diagram_web.sleep_now",
    refusal: "diagram_web.cant_sleep",
    by_hand: true,
    never: true,
};
/// Something isn't written yet.
pub(crate) const UNSAVED: Veto = Veto {
    id: "not written yet",
    menu: "diagram_web.sleep_now_unsaved",
    refusal: "diagram_web.cant_sleep_unsaved",
    by_hand: true,
    never: false,
};
/// A cell being typed into, or a dialog open (what's there isn't in the file yet).
pub(crate) const EDITING: Veto = Veto {
    id: "editing",
    menu: "diagram_web.sleep_now_editing",
    refusal: "diagram_web.cant_sleep_editing",
    by_hand: true,
    never: false,
};
/// An export waits for where it goes.
pub(crate) const EXPORTING: Veto = Veto {
    id: "an export waits",
    menu: "diagram_web.sleep_now_exporting",
    refusal: "diagram_web.cant_sleep_exporting",
    by_hand: true,
    never: false,
};

/// When the diagram last changed (by the user, or on disk): how long it's been quiet.
#[derive(Debug)]
pub(crate) struct LastChange(Mutex<std::time::Instant>);

impl Default for LastChange {
    fn default() -> Self {
        Self(Mutex::new(std::time::Instant::now()))
    }
}

impl LastChange {
    /// It changed now.
    pub(crate) fn now(&self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = std::time::Instant::now();
    }
}

impl Quiet for LastChange {
    fn quiet_for(&self) -> Option<std::time::Duration> {
        Some(self.0.lock().unwrap_or_else(PoisonError::into_inner).elapsed())
    }
}

/// Where its page stands as it sleeps.
#[derive(Debug, Default)]
pub(crate) enum Slumber {
    #[default]
    Awake,
    /// Its picture asked for, for this sleep.
    Freezing(SleepWhy),
    /// Asleep, its picture (`None`: none could be taken).
    Asleep { picture: Option<render_api::ImageData> },
    /// Coming back: the picture shown until the new page is drawn.
    Waking(Option<render_api::ImageData>),
}

impl DiagramWebView {
    /// A view of `file` in the tab at `tab` (its path: where it's brought back to when woken
    /// from elsewhere, what its hint points at).
    pub fn in_tab(tab: &Path, file: Option<PathBuf>) -> Self {
        let mut view = Self::for_file(file);
        if view.path.is_some() {
            let name = tab.with_extension("").to_string_lossy().into_owned();
            let extension = tab.extension().and_then(|e| e.to_str()).unwrap_or(EXTENSION);
            // A known extension: `drawio` (a file's own tab) or ours.
            let extension = if extension == EXTENSION { EXTENSION } else { crate::FILE_EXTENSION };
            let key = format!("diagram-{:x}", kernel::id::new_uuid());
            let place = SleepPlace::Editor { name, extension };
            view.sleep = Some(SleepPart::new(&DIAGRAMS, key, None, place));
        }
        view
    }

    /// Asleep or on its way (no page of its own to show, or the one going).
    pub(crate) fn sleeping(&self) -> bool {
        !matches!(self.slumber, Slumber::Awake)
    }

    /// Acted on by the user now: it sleeps only once left alone long enough.
    pub(crate) fn touch(&self) {
        if let Some(part) = &self.sleep {
            part.touch();
        }
    }

    /// The sleeping told what keeps it awake now (cheap: compared before it's told).
    pub(crate) fn tell_veto(&self) {
        let Some(part) = &self.sleep else { return };
        let veto = if !self.page_ready() || self.page == PageState::Failed {
            Some(NO_PAGE)
        } else if self.session.dirty() {
            Some(UNSAVED)
        } else if self.session.busy() {
            Some(EDITING)
        } else if self.export.is_some() {
            Some(EXPORTING)
        } else {
            None
        };
        part.set_veto(veto);
    }

    /// Its page drawn: coming back, it's back; and whenever the sleeping doesn't know yet how
    /// quiet it is (its first page, a page after a sleep), it started — its time left alone
    /// counting from now. (Its tab chosen touches it earlier: that's no start.)
    pub(crate) fn page_shown(&mut self) {
        let Some(part) = &mut self.sleep else { return };
        match std::mem::take(&mut self.slumber) {
            Slumber::Waking(picture) => {
                if let Some(picture) = picture {
                    picture.release();
                }
                part.waking(false);
            }
            other => self.slumber = other,
        }
        if part.slot().lock().quiet.is_none() {
            part.started(Some(Arc::clone(&self.quiet) as Arc<dyn Quiet>));
        }
    }

    /// The orders waiting (a wake, a sleep), taken at its poll: `true` if it changed.
    pub(crate) fn take_sleep_orders(&mut self) -> bool {
        let Some(part) = &self.sleep else { return false };
        if part.take_wake() {
            return self.wake_by_user();
        }
        let Some(why) = part.take_order() else { return false };
        tracing::debug!(
            by_hand = why == SleepWhy::ByHand,
            "a diagram falls asleep: its picture asked"
        );
        self.slumber = Slumber::Freezing(why);
        true
    }

    /// Its page's picture come (`None`: none could be taken): asleep from now on.
    pub(crate) fn got_picture(&mut self, picture: Option<render_api::ImageData>) {
        tracing::debug!(picture = picture.is_some(), "a diagram's picture came");
        let Slumber::Freezing(why) = self.slumber else {
            // Not asked for (or not any more): let go.
            if let Some(picture) = picture {
                picture.release();
            }
            return;
        };
        self.slumber = Slumber::Asleep { picture };
        if let Some(part) = &mut self.sleep {
            part.fell_asleep(why);
        }
    }

    /// Asleep and acted on by the user (a key, a click, "Wake Up"): a page made anew, the
    /// picture shown till it's drawn. `true` if it was asleep.
    pub(crate) fn wake_by_user(&mut self) -> bool {
        let Slumber::Asleep { picture, .. } = std::mem::take(&mut self.slumber) else {
            return false;
        };
        self.slumber = Slumber::Waking(picture);
        self.new_page();
        if let Some(part) = &mut self.sleep {
            part.waking(true);
        }
        true
    }

    /// Its page as the sleeping wants it: asked for its picture while it falls asleep; kept
    /// once asleep until its picture's copy of its frame is made (the renderer's next
    /// frame); then none.
    pub(crate) fn page_wanted(&self) -> Option<bool> {
        match &self.slumber {
            Slumber::Awake | Slumber::Waking(_) => Some(false),
            Slumber::Freezing(_) => Some(true),
            Slumber::Asleep { picture } => {
                picture.as_ref().is_some_and(|p| p.copy_source().is_some()).then_some(false)
            }
        }
    }

    /// Asleep, its picture's copy of its page's frame not made yet (its page kept till then).
    pub(crate) fn copy_waits(&self) -> bool {
        matches!(&self.slumber, Slumber::Asleep { picture: Some(p) } if p.copy_source().is_some())
    }

    /// Asleep or coming back: its picture, dimmed, and the card over it, in `bounds`. `true`
    /// if drawn (nothing else is then).
    pub(crate) fn paint_asleep(&mut self, ctx: &mut ViewerPaint<'_>) -> bool {
        let (picture, resuming) = match &self.slumber {
            Slumber::Asleep { picture } => (picture.clone(), false),
            Slumber::Waking(picture) => (picture.clone(), true),
            Slumber::Awake | Slumber::Freezing(_) => return false,
        };
        let bounds = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(bounds));
        views::paint_picture(layer, bounds, picture.as_ref(), ctx.palette.editor_background);
        let over = ctx.scene.push_layer(Some(bounds));
        let ds = Ctx::new(ctx.palette, ctx.scale);
        if let Some(part) = &mut self.sleep {
            part.paint_card(over, (ctx.text, ctx.icons), &ds, bounds, resuming);
        }
        true
    }

    /// Its tab's sleeping rows (read from memory).
    pub(crate) fn sleep_menu(&self) -> Vec<ui_components::ViewMenuEntry> {
        self.sleep.as_ref().map(SleepPart::tab_menu).unwrap_or_default()
    }

    /// "Put to Sleep" / "Wake Up" from its menu or the commands: `None` if `id` is neither.
    pub(crate) fn sleep_command(&mut self, id: &str) -> Option<bool> {
        let sleeping = SleepPart::action(id)?;
        self.touch();
        if !sleeping {
            return Some(self.wake_by_user());
        }
        let part = self.sleep.as_ref()?;
        match part.sleep_by_hand() {
            Ok(()) => Some(self.take_sleep_orders()),
            Err(can) => {
                // Never silently nothing: why it can't, told.
                let refusal = part.refusal(can);
                self.notifications.push(refusal);
                Some(true)
            }
        }
    }

    /// Whether it can be put to sleep by hand now (tests).
    #[cfg(test)]
    pub(crate) fn can_sleep(&self) -> views::CanSleep {
        self.sleep.as_ref().map_or(views::CanSleep::No, SleepPart::can_sleep)
    }
}
