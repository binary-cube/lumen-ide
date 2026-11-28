//! What a view that can sleep keeps ([`SleepPart`]): its slot, registered while it lives;
//! the orders taken at its poll; what its tab's menu and icon say; its card while asleep.
//! The view calls it from its own handlers (a key, a click: [`SleepPart::touch`]) and does
//! the rest itself: stopping what it holds, starting again.

use std::sync::Arc;
use std::time::Instant;

use super::{
    ASLEEP_ICON, AsleepCard, CanSleep, Quiet, SLEEP_ACTION, SleepKind, SleepPlace, SleepWhy, Slot,
    Veto, WAKE_ACTION, WAKE_ICON, Waker,
};
use crate::ViewMenuEntry;
use kernel::geometry::Rect;

/// Why a view sleeps, as it fell asleep: by hand or after how long, what it held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slept {
    pub why: SleepWhy,
    pub freed: Option<u64>,
}

/// A view's part of the sleeping (see the module).
#[derive(Debug)]
pub struct SleepPart {
    slot: Arc<Slot>,
    /// Asleep (or coming back): why, for its card.
    slept: Option<Slept>,
    /// Where its card's button went (a click there wakes it).
    wake_button: Option<Rect>,
}

impl SleepPart {
    /// A view of `kind` known by `key`, opened in `project`, its tab at `place`: registered
    /// until it's dropped (then told gone).
    pub fn new(
        kind: &'static SleepKind,
        key: String,
        project: Option<kernel::ProjectId>,
        place: SleepPlace,
    ) -> Self {
        let slot = super::register(kind, key, project, place);
        Self { slot, slept: None, wake_button: None }
    }

    /// Its slot (what the timer and the requests read).
    pub fn slot(&self) -> &Arc<Slot> {
        &self.slot
    }

    /// Its kind.
    pub fn kind(&self) -> &'static SleepKind {
        self.slot.lock().kind
    }

    /// Acted on by the user now (a key, text, a paste, a click, the wheel, a command, its tab
    /// chosen): its time left alone starts again. Hover, frames and windows moving over it
    /// aren't.
    pub fn touch(&self) {
        self.slot.lock().touched = Some(Instant::now());
    }

    /// What it runs started (again): touched now, running, awake.
    pub fn started(&self, quiet: Option<Arc<dyn Quiet>>) {
        {
            let mut state = self.slot.lock();
            state.touched = Some(Instant::now());
            state.quiet = quiet;
            state.running = true;
            state.asleep = false;
        }
        self.slot.announce();
    }

    /// What it runs ended by itself (not asleep): nothing to put to sleep.
    pub fn ended(&self) {
        {
            let mut state = self.slot.lock();
            state.running = false;
            state.quiet = None;
            state.memory = None;
        }
        self.slot.announce();
    }

    /// What of its own keeps it awake now (`None`: nothing), told when it changes.
    pub fn set_veto(&self, veto: Option<Veto>) {
        self.slot.set_veto(veto);
    }

    /// What wakes its window (given at its paint, once: the same every frame).
    pub fn give_waker(&self, waker: &Waker) {
        let mut state = self.slot.lock();
        if state.waker.is_none() {
            state.waker = Some(Arc::clone(waker));
        }
    }

    /// Drawn with the keyboard (`true`) or without: what the commands act on.
    pub fn focus(&self, focused: bool) {
        super::with_focused(|f| {
            let mine = std::ptr::eq(f.as_ptr(), Arc::as_ptr(&self.slot));
            if focused {
                *f = Arc::downgrade(&self.slot);
            } else if mine {
                *f = std::sync::Weak::new();
            }
        });
    }

    /// An order to sleep, if one waits and still holds (fresh; an idle one not touched
    /// since): taken. One that doesn't hold is let go.
    pub fn take_order(&self) -> Option<SleepWhy> {
        let order = {
            let mut state = self.slot.lock();
            let order = state.order.take()?;
            let by_hand = order.why == SleepWhy::ByHand;
            ::sleep::order_holds(order.at, by_hand, state.touched, Instant::now())
                .then_some(order.why)
        };
        if order.is_none() {
            self.slot.announce();
        }
        order
    }

    /// Asked to wake from elsewhere: taken (`true` once).
    pub fn take_wake(&self) -> bool {
        std::mem::take(&mut self.slot.lock().wake)
    }

    /// Whether an order (to sleep or to wake) waits for its poll: an unseen view is polled.
    pub fn order_waits(&self) -> bool {
        let state = self.slot.lock();
        state.order.is_some() || state.wake
    }

    /// Fallen asleep (its view stopped what it holds), `why`: what it held is said freed.
    pub fn fell_asleep(&mut self, why: SleepWhy) {
        let freed = {
            let mut state = self.slot.lock();
            let freed = state.memory.take();
            state.asleep = true;
            state.running = false;
            state.quiet = None;
            state.order = None;
            freed
        };
        self.slept = Some(Slept { why, freed });
        self.slot.announce();
        tracing::info!(
            kind = self.kind().id,
            by_hand = why == SleepWhy::ByHand,
            "a view was put to sleep"
        );
    }

    /// No longer asleep: coming back (`true`: from now until its view says it's settled), or
    /// back (`false`: its card's reason let go).
    pub fn waking(&mut self, on: bool) {
        {
            let mut state = self.slot.lock();
            state.waking = on;
            state.asleep = false;
        }
        if !on {
            self.slept = None;
        }
        self.slot.announce();
    }

    /// Asleep now.
    pub fn asleep(&self) -> bool {
        self.slot.lock().asleep
    }

    /// Why it sleeps (`None`: awake).
    pub fn slept(&self) -> Option<Slept> {
        self.slept
    }

    /// Whether it can be put to sleep by hand now.
    pub fn can_sleep(&self) -> CanSleep {
        self.slot.lock().can_sleep()
    }

    /// Put to sleep by hand (taken at its next poll; the caller may poll at once).
    pub fn sleep_by_hand(&self) -> Result<(), CanSleep> {
        self.slot.sleep_by_hand()
    }

    /// A refusal told in its kind's words.
    pub fn refusal(&self, can: CanSleep) -> workbench_model::Notification {
        super::refusal(self.kind(), can)
    }

    /// Its tab's icon while asleep (the moon), else `None` (its own).
    pub fn tab_icon(&self) -> Option<&'static str> {
        self.asleep().then_some(ASLEEP_ICON)
    }

    /// Its tab's menu: asleep or waking, "Wake Up" (greyed out while it wakes); else "Put to
    /// Sleep", greyed out with why while it can't now; nothing while it can't at all. Read
    /// from memory.
    pub fn tab_menu(&self) -> Vec<ViewMenuEntry> {
        let (can, asleep, texts) = {
            let state = self.slot.lock();
            (state.can_sleep(), state.asleep, state.kind.texts)
        };
        if asleep || can == CanSleep::Waking {
            let waking = can == CanSleep::Waking;
            let label = i18n::t(if waking { texts.waking_up } else { texts.wake_up });
            return vec![
                ViewMenuEntry::item(WAKE_ACTION, label).with_icon(WAKE_ICON).disabled_if(waking),
            ];
        }
        let label = match can {
            CanSleep::No | CanSleep::Waking => return Vec::new(),
            CanSleep::Yes => texts.sleep_now,
            CanSleep::NotNow(veto) => veto.menu,
            CanSleep::Sleeping => texts.going_to_sleep,
        };
        vec![
            ViewMenuEntry::item(SLEEP_ACTION, i18n::t(label))
                .with_icon(ASLEEP_ICON)
                .disabled_if(can != CanSleep::Yes),
        ]
    }

    /// Its card over `area` (asleep; `resuming`: coming back, no button), drawn in its
    /// kind's words; where its button went kept.
    pub fn paint_card(
        &mut self,
        layer: &mut render_api::Layer,
        paint: (&mut text_layout::TextSystem, &icons::Icons),
        ctx: &crate::ds::Ctx<'_>,
        area: Rect,
        resuming: bool,
    ) {
        let texts = self.kind().texts;
        let card = AsleepCard {
            title: i18n::t(if resuming { texts.resuming } else { texts.asleep_title }),
            why: self.slept.map(|s| why(texts, s)).unwrap_or_default(),
            button: (!resuming).then(|| i18n::t(texts.wake_up)),
            how: i18n::t(texts.how),
        };
        self.wake_button = super::paint_card(layer, paint, ctx, area, &card);
    }

    /// Where its card's button is (`None`: no card, or coming back).
    pub fn wake_button(&self) -> Option<Rect> {
        self.wake_button
    }

    /// Its card's button forgotten (no card drawn now).
    pub fn forget_card(&mut self) {
        self.wake_button = None;
    }

    /// Announced as it stands now, if that changed.
    pub fn announce(&self) {
        self.slot.announce();
    }

    /// The menu's action ids: whether `id` is one of the sleeping's (and which).
    pub fn action(id: &str) -> Option<bool> {
        match id {
            SLEEP_ACTION => Some(true),
            WAKE_ACTION => Some(false),
            _ => None,
        }
    }
}

/// Why it sleeps, in words: by hand or after N minutes, and about how much it freed.
fn why(texts: &::sleep::SleepTexts, slept: Slept) -> String {
    let why = match slept.why {
        SleepWhy::ByHand => i18n::t(texts.by_hand),
        SleepWhy::Idle(idle) => i18n::tr!(texts.idle, minutes = idle.as_secs() / 60),
    };
    match slept.freed {
        Some(bytes) => {
            let freed = i18n::tr!(texts.freed, size = i18n::bytes(bytes).as_str());
            format!("{why} · {freed}")
        }
        None => why,
    }
}

impl Drop for SleepPart {
    /// Gone (its tab closed): told, so what shows it lets it go at once.
    fn drop(&mut self) {
        let (project, key) = {
            let state = self.slot.lock();
            (state.project, state.key.clone())
        };
        super::publish(project, &super::SleepStateChanged { key, state: None });
    }
}
