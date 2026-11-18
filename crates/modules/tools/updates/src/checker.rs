//! Looking for updates and bringing one: on the module's own pool ([`THREAD`]), never the
//! UI thread. By itself a little after the first project opens ([`FIRST`]), then every
//! [`EVERY`] while the app runs (when the user lets it: `[updates] check`); when asked
//! ([`check`] with `manual`). A download goes only when the user asks for it ([`download`]):
//! first it's checked that the app can replace itself where it is. Each step begins and ends
//! through [`state`]'s own transitions: two never step on each other.

use std::time::Duration;

use app_updates::Version;
use pinned_downloads::fetch::Http;
use self_update::check::{Ask, CheckError};

use crate::settings;
use crate::state::{self, Looked};

/// The pool the module's work runs on (the Task Manager names it).
pub const THREAD: &str = "updates";
/// The thread waiting for the next look.
pub const WAITING: &str = "updates-wait";
/// The first look, after the app starts (not with everything it starts at once).
pub const FIRST: Duration = Duration::from_secs(30);
/// How often it looks while the app runs.
pub const EVERY: Duration = Duration::from_secs(12 * 60 * 60);

/// The looks begin (the first project opened): how the last update went told first, if the
/// app was just started again after one, and what an earlier update left behind cleared.
pub fn start() {
    kernel::work::run(THREAD, || {
        if let Ok(exe) = std::env::current_exe() {
            self_update::install::clean_left(&self_update::install::target(&exe));
        }
        let outcome = self_update::folder().and_then(|f| self_update::install::take_result(&f));
        if let Some(outcome) = outcome {
            state::tell(state::Tell::Updated(outcome));
        }
    });
    schedule(FIRST);
}

fn schedule(delay: Duration) {
    let later = kernel::work::Later::run(WAITING, delay, || {
        if settings::config().check {
            check(false);
        }
        schedule(EVERY);
    });
    state::keep_later(later);
}

/// GitHub asked for a newer version (`manual`: the user asked — "up to date" said too, and
/// a failure). One found already being downloaded or ready isn't asked about again.
pub fn check(manual: bool) {
    let Some(current) = Version::parse(kernel::brand::version()) else { return };
    if !state::begin_look(manual) {
        return;
    }
    kernel::work::run(THREAD, move || {
        let config = settings::config();
        let fetch = Http::new(self_update::HOSTS);
        let ask = Ask {
            repository: kernel::brand::REPOSITORY,
            current: &current,
            prereleases: config.prereleases,
            skipped: config.skipped.as_ref(),
            platforms: app_updates::platform::preferred(
                std::env::consts::OS,
                std::env::consts::ARCH,
            ),
        };
        let looked = match self_update::check::check(&fetch, ask) {
            Ok(Some(found)) => {
                tracing::info!(version = %found.release.version, "an update found");
                Looked::Found(Box::new(found))
            }
            Ok(None) => Looked::UpToDate,
            Err(why) => {
                tracing::info!("updates couldn't be looked for: {why}");
                let text = i18n::tr!("updates.check_failed", why = why.to_string());
                match why {
                    CheckError::Offline(_) => Looked::Offline(text),
                    CheckError::Failed(_) => Looked::Failed(text),
                }
            }
        };
        state::end_look(looked);
    });
}

/// The version found brought: checked first that this app can be replaced where it is
/// (else said, with its page to download it from), then downloaded and unpacked.
pub fn download() {
    let Some((available, stop)) = state::begin_download() else { return };
    kernel::work::run(THREAD, move || {
        let version = available.release.version.clone();
        let target = std::env::current_exe().map(|e| self_update::install::target(&e));
        let replaceable = target.as_deref().is_ok_and(self_update::install::writable);
        if !replaceable {
            let place = target.map(|t| t.display().to_string()).unwrap_or_default();
            let why = i18n::tr!(
                "updates.not_writable",
                app = kernel::brand::NAME,
                path = place,
                version = version.to_string(),
                page = available.release.page.as_str()
            );
            state::end_download(&version, Err(why));
            return;
        }
        let Some(root) = self_update::folder() else {
            let why = i18n::tr!("updates.download_failed", why = "no data folder");
            state::end_download(&version, Err(why));
            return;
        };
        let fetch = Http::new(self_update::HOSTS);
        let total = available.file.size;
        let progress =
            |received: u64, of: Option<u64>| state::progress(received, of.unwrap_or(total));
        let name = self_update::stage::app_name();
        let ended = self_update::stage::stage(&fetch, &available, &root, &name, &progress, &stop)
            .map_err(|why| {
                tracing::warn!(%version, "the update couldn't be downloaded: {why}");
                i18n::tr!("updates.download_failed", why = why.as_str())
            });
        if let Ok(staged) = &ended {
            tracing::info!(%version, dir = %staged.dir.display(), "the update is ready");
        }
        state::end_download(&version, ended);
    });
}
