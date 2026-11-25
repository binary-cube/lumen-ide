//! The app's web engine (Chromium, plan 99) offered, downloaded and kept up to date, as a
//! module. The app draws web pages with it once it's there; until then they are the
//! system's. The first time a page wants it (the app says so through the engine's install,
//! `web_engine::install::service::want`), the download is offered once — what it is, how
//! big, that it's once — in a notification with Download and Not Now; Not Now is kept, and
//! the download stays in Settings › Web Engine. Its progress goes in the same notification.
//! Once a run, a minute after the first window opens, a newer engine is looked for and
//! downloaded in the background (the one in use stays until the engine starts again), when
//! the user lets it.
//!
//! The install's state is the engine's (`web_engine::install::service`, one for every
//! window); what's told is told by the first window that looks.

pub mod settings;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use web_engine::install::service::{self, Failure, Status, Updated};
use workbench_model::{
    CommandContribution, CommandHandler, ModuleThreads, Notification, NotificationId,
    ProjectContext, ProjectService, SettingsContribution, WorkbenchHost,
};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tools.web_engine"),
    name: "Web Engine",
    description: "Offers, downloads and keeps up to date the engine the app draws web pages with",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: ICON, ..extensions::ModuleInfo::BUILT_IN },
};

/// Download the engine (or try again).
pub const DOWNLOAD: CommandId = CommandId("web_engine.download");
/// The offer not made again (it stays in Settings).
pub const NOT_NOW: CommandId = CommandId("web_engine.not_now");
/// The download under way stopped (what came is kept, resumed next time).
pub const STOP: CommandId = CommandId("web_engine.stop");
/// A newer engine looked for now.
pub const CHECK: CommandId = CommandId("web_engine.check");
/// The engine removed from the disk (downloaded again when wanted).
pub const REMOVE: CommandId = CommandId("web_engine.remove");
/// Its icon (the built-in `globe`).
pub const ICON: &str = "globe";

/// The engine's download as it's published (MB, rounded up): told before it's asked for.
pub const DOWNLOAD_MB: u32 = 100;
/// …and what it takes on the disk once unpacked.
pub const INSTALLED_MB: u32 = 300;

/// How long after the first window opens a newer engine is looked for.
const UPDATE_AFTER: Duration = Duration::from_secs(60);

/// A command contributed: its id, its label (the locales' fallback), what it does.
type Command = (CommandId, &'static str, Arc<dyn CommandHandler>);

#[derive(Debug, Default)]
pub struct WebEngineModule;

impl Module for WebEngineModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        let commands: [Command; 5] = [
            (DOWNLOAD, "Download the Web Engine", Arc::new(Download)),
            (NOT_NOW, "Don't Offer the Web Engine Again", Arc::new(NotNow)),
            (STOP, "Stop the Web Engine's Download", Arc::new(Stop)),
            (CHECK, "Check for a Newer Web Engine", Arc::new(Check)),
            (REMOVE, "Remove the Web Engine", Arc::new(Remove)),
        ];
        for (id, label, handler) in commands {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts: &[] },
                icon: Some(ICON),
                handler,
            });
        }
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(Announcer));
        ctx.contribute::<Arc<dyn SettingsContribution>>(Arc::new(
            settings::WebEngineSettings::default(),
        ));
        ctx.contribute(ModuleThreads(&[service::THREAD, UPDATE_THREAD]));
        Ok(())
    }
}

/// The thread a newer engine is waited for on (it only sleeps until it looks).
const UPDATE_THREAD: &str = "web-engine-update";

/// What the windows told, for the whole app.
#[derive(Debug, Default)]
struct Told {
    /// The offer was made this run.
    offered: bool,
    /// The user asked for the download (its progress and its end are told; an update's
    /// aren't).
    asked: bool,
    /// The user asked for a newer engine (how the look ended is told; the background's
    /// isn't).
    checking: bool,
    /// The notification it's told in (offer, progress, done: one, updated).
    notification: Option<NotificationId>,
}

static TOLD: Mutex<Option<Told>> = Mutex::new(None);
/// The install's state last looked at by a window (the one that sees a change tells it).
static SEEN: AtomicU64 = AtomicU64::new(0);
/// The look for a newer engine is planned (once a run).
static UPDATE_PLANNED: AtomicBool = AtomicBool::new(false);
/// The planned look, held until it runs.
static UPDATE: Mutex<Option<kernel::work::Later>> = Mutex::new(None);

fn told<T>(f: impl FnOnce(&mut Told) -> T) -> T {
    let mut guard = TOLD.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(Told::default))
}

/// `generation` is new to the windows, and this one is the first to see it (the others
/// polling after it don't tell it again). A plain load while nothing changed.
fn first_to_see(seen: &AtomicU64, generation: u64) -> bool {
    seen.load(Ordering::Acquire) != generation
        && seen.swap(generation, Ordering::AcqRel) != generation
}

/// The windows, woken at each change of the install (the first one to look tells it).
#[derive(Debug)]
struct Announcer;

impl ProjectService for Announcer {
    fn attach(&self, _project: &ProjectContext<'_>) -> Vec<kernel::Subscription> {
        Vec::new()
    }

    fn window_opened(&self, wake: &Arc<dyn Fn() + Send + Sync>) -> Vec<kernel::Subscription> {
        service::follow(wake);
        plan_update();
        Vec::new()
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        if !first_to_see(&SEEN, service::generation()) {
            return false;
        }
        let look = Look {
            status: service::status(),
            wanted: service::wanted(),
            declined: settings::config().declined,
            updated: service::updated(),
        };
        show(host, told(|t| t.tell(look)))
    }
}

/// What's told shown by `host`: `true` when something was.
fn show(host: &mut dyn WorkbenchHost, telling: Telling) -> bool {
    match telling {
        Telling::Nothing => return false,
        Telling::Show(notification) => host.update_notification(notification),
        Telling::Dismiss(id) => host.dismiss_notification(id),
    }
    true
}

/// A newer engine looked for once a run, a while after the first window opened, when the
/// settings let it (an engine there: one missing waits for the user).
fn plan_update() {
    if UPDATE_PLANNED.swap(true, Ordering::AcqRel) {
        return;
    }
    let later = kernel::work::Later::run(UPDATE_THREAD, UPDATE_AFTER, || {
        if settings::config().check && matches!(service::status(), Status::Installed { .. }) {
            service::start();
        }
    });
    *UPDATE.lock().unwrap_or_else(PoisonError::into_inner) = later;
}

/// What a window looked at after a change of the install.
#[derive(Debug, Clone)]
struct Look {
    status: Status,
    /// A page wanted the engine while it was missing.
    wanted: bool,
    /// The offer was refused (saved in the settings).
    declined: bool,
    /// How the last look for a newer engine ended (`None`: one runs, or none ran).
    updated: Option<Updated>,
}

/// What a change of the install tells the user.
#[derive(Debug, Clone, PartialEq)]
enum Telling {
    Nothing,
    /// Its notification shown, or updated in place (the same id).
    Show(Notification),
    /// Its notification gone (the download stopped by the user).
    Dismiss(NotificationId),
}

impl Told {
    /// What's told now: the offer (a page wanted it, it's missing, not refused, not
    /// offered yet this run), the download's progress and its end (asked for by the user),
    /// how a look for a newer one the user asked for ended.
    fn tell(&mut self, look: Look) -> Telling {
        let Look { status, wanted, declined, updated } = look;
        let id = *self.notification.get_or_insert_with(NotificationId::new);
        let ours = |n: Notification| Telling::Show(n.with_id(id));
        if self.checking
            && let Some(updated) = updated
        {
            self.checking = false;
            self.notification = None;
            return ours(checked(updated, &status));
        }
        match status {
            Status::Missing if wanted && !declined && !self.offered => {
                self.offered = true;
                ours(offer())
            }
            Status::Downloading { received, total } if self.asked => {
                let mut progress = Notification::progress(t("web_engine.downloading_title"))
                    .with_detail(downloaded(received, total));
                // A size not known: the bar sweeps instead of standing at 0 %.
                if total > 0 {
                    progress = progress.with_progress(received as f32 / total as f32);
                }
                ours(progress.with_action(t("web_engine.stop"), STOP))
            }
            Status::Unpacking if self.asked => {
                ours(Notification::progress(t("web_engine.unpacking")))
            }
            Status::Installed { version, .. } if self.asked => {
                self.asked = false;
                self.notification = None;
                ours(
                    Notification::success(t("web_engine.ready_title"))
                        .with_detail(tr!("web_engine.ready_detail", version = version.as_str())),
                )
            }
            Status::Failed(failure) if self.asked => {
                self.asked = false;
                let detail = failure_text(failure);
                ours(
                    Notification::warning(t("web_engine.failed_title"))
                        .with_detail(detail)
                        .with_action(t("web_engine.retry"), DOWNLOAD),
                )
            }
            // Stopped by the user: its notification goes (a new one for the next try).
            Status::Missing if self.asked => {
                self.asked = false;
                self.notification = None;
                Telling::Dismiss(id)
            }
            _ => Telling::Nothing,
        }
    }
}

/// How a look for a newer engine the user asked for ended, as they're told it (`status`:
/// the one installed now).
fn checked(updated: Updated, status: &Status) -> Notification {
    match updated {
        Updated::Newer(version) => Notification::success(t("web_engine.updated_title"))
            .with_detail(tr!("web_engine.updated_detail", version = version.as_str())),
        Updated::UpToDate => {
            let up_to_date = Notification::info(t("web_engine.up_to_date_title"));
            match status {
                Status::Installed { version, .. } => up_to_date
                    .with_detail(tr!("web_engine.up_to_date_detail", version = version.as_str())),
                _ => up_to_date,
            }
        }
        Updated::Failed(failure) => Notification::warning(t("web_engine.update_failed_title"))
            .with_detail(failure_text(failure))
            .with_action(t("web_engine.retry"), CHECK),
    }
}

/// What the user is told of why the last try failed: its kind, in their language (never a
/// path, an address or the system's own message).
pub(crate) fn failure_text(failure: Failure) -> String {
    t(match failure {
        Failure::Offline => "web_engine.offline",
        Failure::NoDataFolder => "web_engine.failure_no_data_folder",
        Failure::NoPackage => "web_engine.failure_no_package",
        Failure::Untrusted => "web_engine.failure_untrusted",
        Failure::Damaged => "web_engine.failure_damaged",
        Failure::Server => "web_engine.failure_server",
        Failure::Disk => "web_engine.failure_disk",
    })
}

/// How much of the download came: "of" its size when it's known, else only what came.
pub(crate) fn downloaded(received: u64, total: u64) -> String {
    if total == 0 {
        return tr!("web_engine.downloading_received", received = i18n::bytes(received));
    }
    tr!(
        "web_engine.downloading_detail",
        received = i18n::bytes(received),
        total = i18n::bytes(total)
    )
}

/// The offer: what the engine is, how big, that it's once; Download and Not Now.
fn offer() -> Notification {
    Notification::info(t("web_engine.offer_title"))
        .with_detail(tr!(
            "web_engine.offer_detail",
            download = DOWNLOAD_MB as i64,
            installed = INSTALLED_MB as i64
        ))
        .with_action(t("web_engine.download"), DOWNLOAD)
        .with_action(t("web_engine.not_now"), NOT_NOW)
        .with_timeout(workbench_model::Timeout::Never)
}

/// The download asked for by the user: its progress told from now on (as it was, if one was
/// under way already and nothing new started).
pub(crate) fn download() {
    let before = told(|t| std::mem::replace(&mut t.asked, true));
    if !service::start() {
        told(|t| t.asked = before);
    }
}

/// [`DOWNLOAD`]: the engine downloaded (or tried again), its progress told.
struct Download;

impl CommandHandler for Download {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        matches!(service::status(), Status::Missing | Status::Failed(_))
    }

    fn run(&self, _host: &mut dyn WorkbenchHost) -> bool {
        download();
        true
    }
}

/// [`NOT_NOW`]: the offer not made again (saved, and said once it is); its notification
/// goes.
struct NotNow;

impl CommandHandler for NotNow {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let saved = host.user_settings().is_some_and(settings::decline);
        if let Some(id) = told(|t| t.notification.take()) {
            host.dismiss_notification(id);
        }
        // "Not offered again" only once it's kept (this run doesn't offer it again anyway).
        if saved {
            host.notify(Notification::info(t("web_engine.declined")));
        }
        true
    }
}

/// [`STOP`]: the download under way stopped.
struct Stop;

impl CommandHandler for Stop {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        service::status().busy()
    }

    fn run(&self, _host: &mut dyn WorkbenchHost) -> bool {
        service::stop();
        true
    }
}

/// [`CHECK`]: a newer engine looked for (and downloaded) now.
struct Check;

impl CommandHandler for Check {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        matches!(service::status(), Status::Installed { .. }) && !service::updating()
    }

    /// The look said in a notification only once it started, that notification then telling
    /// how it ended.
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if !service::start() {
            return true;
        }
        let id = told(|t| {
            t.checking = true;
            *t.notification.get_or_insert_with(NotificationId::new)
        });
        host.update_notification(Notification::info(t("web_engine.checking")).with_id(id));
        true
    }
}

/// [`REMOVE`]: the engine removed (its versions no engine runs from, then the pages'
/// storage); pages are the system's until it's downloaded again.
struct Remove;

impl CommandHandler for Remove {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        matches!(service::status(), Status::Installed { .. } | Status::Failed(_))
            && !service::updating()
    }

    /// Said only when it's done (never while a download or an update is under way).
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if service::remove() {
            host.notify(Notification::info(t("web_engine.removed")));
        }
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
