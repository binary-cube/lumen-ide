//! Views put to sleep when left alone (plan 101), whoever they are: an agent's session, a
//! diagram's page, any view built on `ui_components::sleep`. This module only decides
//! **when** and passes it on; each view's module decides **what** (stopping what it holds,
//! keeping its state, bringing it back). It names no view and no module:
//!
//! - the timer ([`timer`]) looks once a minute and orders the views left alone to sleep;
//! - other modules ask it by a view's key ([`PutToSleep`], [`WakeUp`], [`SleepStateOf`]) and
//!   follow each view's state on their project's bus ([`SleepStateChanged`]);
//! - two commands act on the view with the keyboard ("Put Tab to Sleep", "Wake Up Tab");
//! - a kind's hint, offering to turn its sleeping on, is shown the first time one of its
//!   views opens in the editor while it's off (the hint is its module's; this shows it).
//!
//! Each kind's idle time is its own module's setting, told to `ui_components::sleep`.
//! Removed, nothing sleeps and nothing else changes.

mod timer;

use std::sync::{Arc, Mutex, PoisonError};

use command_model::{CommandId, CommandSpec};
use contracts::sleep::{PutToSleep, SleepStateChanged, SleepStateOf, WakeUp};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::sleep::{self as views, CanSleep, SleepPlace, Slot};
use workbench_model::{CommandContribution, CommandHandler, WorkbenchHost};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("sleep"),
    name: "Sleeping Tabs",
    description: "Heavy tabs left alone put to sleep, woken as they were",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "moon", ..extensions::ModuleInfo::BUILT_IN },
};

/// Puts the view with the keyboard to sleep now (by hand).
pub const SLEEP_NOW: CommandId = CommandId("sleep.now");
/// Wakes the sleeping view with the keyboard.
pub const WAKE_NOW: CommandId = CommandId("sleep.wake");

#[derive(Debug, Default)]
pub struct SleepModule;

impl Module for SleepModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        views::set_starter(Some(Arc::new(timer::ensure_running)));
        views::set_announcer(Some(Arc::new(publish)));
        ctx.contribute::<Arc<dyn workbench_model::ProjectService>>(Arc::new(Sleeper));
        ctx.contribute(workbench_model::Answers::of(|asked: PutToSleep, host| {
            let Some(slot) = views::find(&asked.key) else { return false };
            match slot.sleep_by_hand() {
                Ok(()) => true,
                Err(can) => {
                    // Never silently nothing: why, told; and how it stands, again (whoever
                    // asked may have shown it going to sleep).
                    host.notify(views::refusal(slot.lock().kind, can));
                    slot.announce_again();
                    false
                }
            }
        }));
        ctx.contribute(workbench_model::Answers::of(|asked: WakeUp, host| {
            let Some(slot) = views::find(&asked.key) else { return false };
            let woken = wake_shown(&slot, host);
            if !woken {
                slot.announce_again();
            }
            woken
        }));
        ctx.contribute(workbench_model::Answers::of(|asked: SleepStateOf, _host| {
            views::want_memory();
            views::find(&asked.key).map(|slot| slot.lock().state())
        }));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SLEEP_NOW,
                label: "Put Tab to Sleep",
                menu: None,
                shortcuts: &[],
            },
            icon: Some(views::ASLEEP_ICON),
            handler: Arc::new(SleepNow),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec { id: WAKE_NOW, label: "Wake Up Tab", menu: None, shortcuts: &[] },
            icon: Some(views::WAKE_ICON),
            handler: Arc::new(WakeNow),
        });
        Ok(())
    }
}

/// The view in `slot` woken and its tab shown (asked again by its name, its tab comes to the
/// front; drawn, it comes back). `false`: not asleep, or its tab can't be shown — then it's
/// left as it was.
fn wake_shown(slot: &Arc<Slot>, host: &mut dyn WorkbenchHost) -> bool {
    let place = {
        let state = slot.lock();
        if !state.asleep {
            return false;
        }
        state.place.clone()
    };
    let shown = match &place {
        SleepPlace::Editor { name, extension } => host.open_view(name, extension, &()),
        SleepPlace::Panel { tool, name, extension } => {
            host.show_in_panel(tool, name, extension, &())
        }
    };
    shown && slot.wake()
}

/// The view with the keyboard, if it's one that can sleep (its kind's viewer has the keys).
fn focused(host: &dyn WorkbenchHost) -> Option<Arc<Slot>> {
    let slot = views::focused()?;
    let viewer = slot.lock().kind.viewer;
    (host.keyed_view() == Some(viewer)).then_some(slot)
}

/// [`SLEEP_NOW`]: offered only while the view with the keyboard can sleep now.
struct SleepNow;

impl CommandHandler for SleepNow {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        focused(host).is_some_and(|slot| slot.lock().can_sleep() == CanSleep::Yes)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        self.enabled(&*host) && host.view_command(views::SLEEP_ACTION)
    }
}

/// [`WAKE_NOW`]: offered only while the view with the keyboard sleeps.
struct WakeNow;

impl CommandHandler for WakeNow {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        focused(host).is_some_and(|slot| slot.lock().asleep)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        self.enabled(&*host) && host.view_command(views::WAKE_ACTION)
    }
}

// ---------------------------------------------------------------------------------------
// Each open project's bus: where its views' states are told.

/// The buses heard, each project's (a closed one's let go).
static BUSES: Mutex<Vec<(u64, kernel::ProjectId, kernel::EventBus)>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `bus` heard as project `project`'s until the guard returned is dropped.
fn hear(project: kernel::ProjectId, bus: kernel::EventBus) -> kernel::Subscription {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    lock(&BUSES).push((id, project, bus));
    kernel::Subscription::on_end(move || lock(&BUSES).retain(|(b, ..)| *b != id))
}

/// `event` published on project `project`'s bus (`None`: a view of no project — on every
/// open project's). The buses are taken out of the lock first: a listener may publish too.
fn publish(project: Option<kernel::ProjectId>, event: &SleepStateChanged) {
    let buses: Vec<kernel::EventBus> = lock(&BUSES)
        .iter()
        .filter(|(_, p, _)| project.is_none_or(|q| q == *p))
        .map(|(.., bus)| bus.clone())
        .collect();
    for bus in buses {
        bus.publish(event);
    }
}

// ---------------------------------------------------------------------------------------
// The hints: each kind's offer, shown once per view opened in the editor while it's off.

/// The views whose hint was shown already (by key; a view gone let go).
static HINTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The views of the window's project (or of none) opened in the editor whose kind's sleeping
/// is off and has a hint, not shown yet: `(key, hint, tab name, tab extension)`. A view of no
/// known project is looked at by every window: only the one with its tab shows its hint.
fn hints_due(
    project: Option<kernel::ProjectId>,
) -> Vec<(String, &'static str, String, &'static str)> {
    let slots = views::slots();
    let mut hinted = lock(&HINTED);
    hinted.retain(|key| slots.iter().any(|s| s.lock().key == *key));
    let mut due = Vec::new();
    for slot in &slots {
        let state = slot.lock();
        let SleepPlace::Editor { name, extension } = &state.place else { continue };
        let Some(hint) = state.kind.hint else { continue };
        let ours = state.project.is_none_or(|p| Some(p) == project);
        if !ours || views::idle_for(state.kind.id).is_some() || hinted.contains(&state.key) {
            continue;
        }
        due.push((state.key.clone(), hint, name.clone(), *extension));
    }
    due
}

/// The sleeping's part of each project: its bus heard; at the window's poll, the hints due.
#[derive(Debug)]
struct Sleeper;

impl workbench_model::ProjectService for Sleeper {
    fn attach(&self, project: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        vec![hear(project.id, project.events.clone())]
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        // Cheap first: nothing that can sleep, nothing to look at.
        if !views::any() {
            return false;
        }
        timer::ensure_running();
        for (key, hint, name, extension) in hints_due(host.project_id()) {
            let tab = workbench_model::ViewTab::new(name, extension);
            // Shown (or seen for good, or closed this run): not asked again; not here (its
            // tab in another window): that window's turn.
            if host.show_hint_by(hint, workbench_model::HintAnchor::Tab(tab)) {
                lock(&HINTED).push(key);
            }
        }
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
