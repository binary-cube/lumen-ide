//! The app's updates, as a module (plan 84c–84f). A little after the first project opens,
//! then twice a day while the app runs (when the user lets it), the newest release is
//! looked for on GitHub ([`checker`]); a newer one is told once, in a notification with
//! What's New (its notes, a modal: [`view`]) and Update. Nothing is downloaded, nor
//! installed, unless the user chooses: Update downloads it in the background (its progress
//! in the same notification), Restart Now closes the app as Quit does — the work still
//! running and the unsaved files asked about first, Cancel keeping everything — and the
//! app puts it in its place and starts again with the windows it had. Help › Check for
//! Updates… asks at any time; Skip This Version stops it being offered (a newer one is).
//!
//! The state is the app's, one for every window ([`state`]); what's told is told by the
//! first window that looks. Its settings are on their own page ([`settings`]).

mod checker;
pub mod settings;
mod state;
pub mod view;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec, MenuAt};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::t;
use ui_components::ViewerProvider;
use workbench_model::{
    CommandContribution, CommandHandler, ModuleThreads, Notification, ProjectContext,
    ProjectService, SettingsContribution, WorkbenchHost,
};

use crate::state::{Phase, Tell};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tools.updates"),
    name: "Updates",
    description: "Tells when a new version is out, shows what's in it, and updates the app when the user chooses",
    depends_on: &[],
};

/// Help › Check for Updates… (the application's menu on macOS).
pub const CHECK: CommandId = CommandId("updates.check");
/// The found version's notes.
pub const WHATS_NEW: CommandId = CommandId("updates.whats_new");
/// Download the version found; once ready, restart to put it in place.
pub const INSTALL: CommandId = CommandId("updates.install");
/// The version found not offered again.
pub const SKIP: CommandId = CommandId("updates.skip");
/// Its icon (the built-in `download`).
pub const ICON: &str = "download";

/// A command contributed: its id, its label (the locales' fallback), its place in the
/// menu bar, what it does.
type Command = (CommandId, &'static str, Option<MenuAt>, Arc<dyn CommandHandler>);

#[derive(Debug, Default)]
pub struct UpdatesModule;

impl Module for UpdatesModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        let about = workbench_model::menus::sections::APP_ABOUT;
        let commands: [Command; 4] = [
            (CHECK, "Check for Updates…", Some(MenuAt::new(about, 20)), Arc::new(Check)),
            (WHATS_NEW, "What's New in the Update…", None, Arc::new(WhatsNew)),
            (INSTALL, "Update and Restart", None, Arc::new(Install)),
            (SKIP, "Skip This Version", None, Arc::new(Skip)),
        ];
        for (id, label, menu, handler) in commands {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu, shortcuts: &[] },
                icon: Some(ICON),
                handler,
            });
        }
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(Announcer));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(view::NotesProvider));
        ctx.contribute::<Arc<dyn SettingsContribution>>(Arc::new(settings::UpdatesSettings));
        ctx.contribute(ModuleThreads(&[checker::THREAD, checker::WAITING]));
        Ok(())
    }
}

/// The open windows (a project in them or not: the start screen too), woken when there's
/// something to tell (the first one to look tells it); the looks begin with the first.
#[derive(Debug)]
struct Announcer;

impl ProjectService for Announcer {
    fn attach(&self, _project: &ProjectContext<'_>) -> Vec<kernel::Subscription> {
        Vec::new()
    }

    fn window_opened(&self, wake: &Arc<dyn Fn() + Send + Sync>) -> Vec<kernel::Subscription> {
        let id = kernel::id::new_uuid();
        if state::add_wake(id, Arc::clone(wake)) {
            checker::start();
        }
        vec![kernel::Subscription::on_end(move || state::remove_wake(id))]
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        let focused = host.window_focused();
        let Some(tell) = state::take_tell_for(|id| host.shows_notification(id), focused) else {
            return false;
        };
        show(host, tell);
        true
    }
}

/// `tell` shown: the update's own notification (one, updated as it goes), or a word.
fn show(host: &mut dyn WorkbenchHost, tell: Tell) {
    // What's New open shows the download itself: no notification over it (told once it
    // closes, if the download still goes on or is ready).
    if state::is_notes_open() && matches!(tell, Tell::Progress | Tell::Ready) {
        return;
    }
    let app = kernel::brand::NAME;
    let phase = state::phase();
    let version = phase.available().map(|a| a.release.version.to_string()).unwrap_or_default();
    let version = version.as_str();
    let ours = |n: Notification| n.with_id(state::notification().0);
    let notification = match tell {
        Tell::Available => ours(
            Notification::info(i18n::tr!("updates.available_title", app = app, version = version))
                .with_detail(i18n::tr!(
                    "updates.available_detail",
                    current = kernel::brand::version()
                ))
                .with_action(t("updates.whats_new"), WHATS_NEW)
                .with_action(t("updates.update"), INSTALL),
        ),
        Tell::Progress => {
            let Phase::Downloading { received, total, .. } = phase else { return };
            ours(
                Notification::progress(i18n::tr!(
                    "updates.downloading_title",
                    app = app,
                    version = version
                ))
                .with_detail(i18n::tr!(
                    "updates.downloading_detail",
                    received = i18n::bytes(received),
                    total = i18n::bytes(total)
                ))
                .with_progress(if total > 0 {
                    received as f32 / total as f32
                } else {
                    0.0
                }),
            )
        }
        Tell::Ready => ours(
            Notification::success(i18n::tr!("updates.ready_title", app = app, version = version))
                .with_detail(t("updates.ready_detail"))
                .with_action(t("updates.restart_now"), INSTALL)
                .with_action(t("updates.whats_new"), WHATS_NEW),
        ),
        // In the spinner's place (the next one is a new notification).
        Tell::UpToDate => {
            let told = ours(Notification::success(i18n::tr!(
                "updates.up_to_date",
                app = app,
                version = kernel::brand::version()
            )));
            state::forget_notification();
            told
        }
        Tell::Failed(text) => ours(Notification::warning(text)),
        Tell::Updated(outcome) => match outcome.error {
            None => Notification::success(i18n::tr!(
                "updates.updated",
                app = app,
                version = outcome.version.as_str()
            )),
            Some(why) => Notification::warning(i18n::tr!(
                "updates.update_failed",
                version = outcome.version.as_str(),
                why = why.as_str()
            )),
        },
    };
    host.update_notification(notification);
}

/// The version found: downloaded (Update), or — ready — put in the app's place, the app
/// still running, then restarted into (Restart Now: the app closes as Quit does, asking
/// about what still runs and what isn't saved; cancelled, the next start is the new one).
/// Not put in place: said, the app as it was.
pub(crate) fn install(host: &mut dyn WorkbenchHost) {
    match state::phase() {
        Phase::Ready { staged, .. } => {
            if let Err(why) = place(&staged.dir) {
                host.notify(Notification::warning(i18n::tr!(
                    "updates.cant_place",
                    why = why.as_str()
                )));
                return;
            }
            if !host.restart_to_update(&staged.dir) {
                host.notify(Notification::warning(t("updates.cant_restart")));
            }
        }
        Phase::Available(_) => checker::download(),
        Phase::Idle | Phase::Downloading { .. } => {}
    }
}

/// Ready, not now (What's New's Later): put in the app's place all the same, quietly — the
/// next start is the new version (what fails is tried again at Restart Now).
pub(crate) fn later() {
    if let Phase::Ready { staged, .. } = state::phase() {
        let _ = place(&staged.dir);
    }
}

/// The staged update in `dir` put in the app's place, once.
fn place(dir: &std::path::Path) -> Result<(), String> {
    if state::is_placed(dir) {
        return Ok(());
    }
    put_in_place(dir)?;
    state::mark_placed(dir);
    Ok(())
}

#[cfg(not(test))]
fn put_in_place(dir: &std::path::Path) -> Result<(), String> {
    self_update::install::put_in_place(dir).map(drop)
}

/// In tests: nothing moved (a folder named `broken` fails).
#[cfg(test)]
fn put_in_place(dir: &std::path::Path) -> Result<(), String> {
    if dir.ends_with("broken") { Err("disk full".into()) } else { Ok(()) }
}

/// The version found not offered again (saved; a newer one is offered).
pub(crate) fn skip(host: &mut dyn WorkbenchHost) {
    let phase = state::phase();
    let Some(found) = phase.available() else { return };
    let version = found.release.version.clone();
    if let Some(user) = host.user_settings() {
        settings::skip(user, &version);
    }
    // A download running stops: the skipped version is never made ready.
    state::let_go();
    if let Some(id) = state::forget_notification() {
        host.dismiss_notification(id);
    }
    let text = i18n::tr!("updates.skipped", version = version.to_string());
    host.notify(Notification::info(text));
}

/// [`CHECK`]: GitHub asked now (up to date, a failure: said too).
struct Check;

impl CommandHandler for Check {
    /// Looked for at once, a spinner meanwhile: the update's notification, replaced by the
    /// answer (found, up to date, or what went wrong).
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if !matches!(state::phase(), Phase::Downloading { .. } | Phase::Ready { .. }) {
            let (id, _) = state::notification();
            host.update_notification(Notification::progress(t("updates.checking")).with_id(id));
            // The answer told here, where the spinner is (not in another window).
            state::spinning(id);
        }
        checker::check(true);
        true
    }
}

/// [`WHATS_NEW`]: the version found's notes, in the modal (only while one is found).
struct WhatsNew;

impl CommandHandler for WhatsNew {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        state::phase().available().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(notes) = view::Notes::current() else { return false };
        host.open_modal(&notes.title(), view::EXTENSION, &notes)
    }
}

/// [`INSTALL`]: download, or restart once ready.
struct Install;

impl CommandHandler for Install {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        matches!(state::phase(), Phase::Available(_) | Phase::Ready { .. })
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        install(host);
        true
    }
}

/// [`SKIP`]: the version found not offered again.
struct Skip;

impl CommandHandler for Skip {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        state::phase().available().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        skip(host);
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
