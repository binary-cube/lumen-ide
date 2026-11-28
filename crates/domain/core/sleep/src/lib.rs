//! When a heavy view (an agent's session, a diagram's page…) may be put to sleep, whoever it
//! is: the facts its slot holds, the one rule over them, the reasons it stays awake, and how
//! it stands for a menu. Pure: no clock read here (the caller says `now`), no threads, nothing
//! of any module. What a view does to fall asleep and to wake is its own; this only says
//! when.
//!
//! The rule (plan 101, as 100l made it): a view sleeps when its kind's sleeping is on and it
//! was left untouched (no key, click, wheel, paste, command, its tab not chosen) for the idle
//! time, on screen or not, it was quiet as long (its own measure: a program's output, a
//! page's changes), nothing of its own keeps it awake (a veto: work under way, something
//! unsaved), and it isn't asleep, ordered to sleep or waking already.

use std::time::{Duration, Instant};

/// How long an order to sleep stays good: one not taken by then is let go (never applied
/// later, after the view was used again).
pub const ORDER_FRESH: Duration = Duration::from_secs(20);

/// A kind of view that can sleep, declared once by its module: its id, its hint, its words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepKind {
    /// Its id (stable: the log, its slots).
    pub id: &'static str,
    /// Its views' viewer (what the commands check has the keyboard).
    pub viewer: &'static str,
    /// The hint offering to turn its sleeping on, shown the first time one of its views
    /// opens in the editor while it's off (`None`: no hint).
    pub hint: Option<&'static str>,
    /// What its card, its menus and its refusals say (i18n keys: its own nouns).
    pub texts: &'static SleepTexts,
}

/// The i18n keys a kind's views are told with (each kind its own: "Session asleep",
/// "Diagram asleep"), the same parts for all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SleepTexts {
    /// The card's title asleep, and while it comes back.
    pub asleep_title: &'static str,
    pub resuming: &'static str,
    /// Why, on the card: by hand; after `{minutes}` idle; `{size}` freed.
    pub by_hand: &'static str,
    pub idle: &'static str,
    pub freed: &'static str,
    /// The card's button, and the line under it (any key, a click).
    pub wake_up: &'static str,
    pub how: &'static str,
    /// The menu's rows: put to sleep, on its way, waking.
    pub sleep_now: &'static str,
    pub going_to_sleep: &'static str,
    pub waking_up: &'static str,
    /// A refusal: can't now, it's going to sleep, it's waking.
    pub cant: &'static str,
    pub cant_sleeping: &'static str,
    pub cant_waking: &'static str,
}

/// Something of a view's own that keeps it awake now (work under way, something not sent or
/// not saved, not knowing how it would come back): named for the log, told in its menu and
/// in a refusal by its texts' keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Veto {
    /// Its id in the log (`text typed, not sent`).
    pub id: &'static str,
    /// The menu row's label while it holds (i18n key: "Put to sleep (something typed…)").
    pub menu: &'static str,
    /// What a refused order tells (i18n key).
    pub refusal: &'static str,
    /// It holds against the user's own order too (else only the idle one waits for it).
    pub by_hand: bool,
    /// It can't sleep at all now (its row hidden, not greyed out): it ended, nobody can
    /// bring it back.
    pub never: bool,
}

/// Whether a view can be put to sleep by hand now, as its menus say it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CanSleep {
    /// Never now: asleep already, or a veto that hides the row.
    #[default]
    No,
    /// Not now, for this reason (its row greyed out, saying why).
    NotNow(Veto),
    /// Not now: it's being put to sleep (the order given, not taken yet).
    Sleeping,
    /// Not now: it's waking up.
    Waking,
    Yes,
}

/// What a view's slot says of it: what the rule and the menus read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SleepFacts {
    /// When the user last acted on it, or it last started (`None`: never started).
    pub touched: Option<Instant>,
    /// How long it has done nothing by itself, by its own measure (`None`: not known).
    pub quiet_for: Option<Duration>,
    /// What of its own keeps it awake now.
    pub veto: Option<Veto>,
    pub asleep: bool,
    /// An order to sleep is on its way.
    pub ordered: bool,
    pub waking: bool,
}

/// Why it fell asleep, as its card tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepWhy {
    /// The user's own order.
    ByHand,
    /// Left alone this long.
    Idle(Duration),
}

/// What keeps the view described by `facts` awake now, after `idle` (`None`: nothing, it may
/// sleep): the first reason, as the log names it. Any doubt keeps it awake.
pub fn awake_because(facts: &SleepFacts, idle: Duration, now: Instant) -> Option<&'static str> {
    let Some(touched) = facts.touched else { return Some("never started") };
    let reason = if facts.asleep {
        "asleep"
    } else if facts.ordered {
        "ordered to sleep"
    } else if facts.waking {
        "waking"
    } else if let Some(veto) = facts.veto {
        veto.id
    } else if now.saturating_duration_since(touched) < idle {
        "used lately"
    } else if !facts.quiet_for.is_some_and(|q| q >= idle) {
        "active lately"
    } else {
        return None;
    };
    Some(reason)
}

/// Whether the view described by `facts` can be put to sleep by hand now: waking or going to
/// sleep first, then asleep (nothing to do), then its veto (only one that holds against the
/// user's order counts), else yes — whatever its idle time.
pub fn can_sleep(facts: &SleepFacts) -> CanSleep {
    if facts.waking {
        return CanSleep::Waking;
    }
    if facts.ordered {
        return CanSleep::Sleeping;
    }
    if facts.asleep || facts.touched.is_none() {
        return CanSleep::No;
    }
    match facts.veto {
        Some(veto) if veto.never => CanSleep::No,
        Some(veto) if veto.by_hand => CanSleep::NotNow(veto),
        _ => CanSleep::Yes,
    }
}

/// Whether an order given at `ordered` is still to be taken at `now`: fresh, and (unless it's
/// the user's own) the view not touched since.
pub fn order_holds(
    ordered: Instant,
    by_hand: bool,
    touched: Option<Instant>,
    now: Instant,
) -> bool {
    now.saturating_duration_since(ordered) < ORDER_FRESH
        && (by_hand || touched.is_none_or(|t| t <= ordered))
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
