//! An update put in place by the app itself (plan 85), while it still runs: the app set
//! aside beside it (`Lumen.app.old`, `lumen.exe.old` — a running program can be renamed on
//! every system, Windows included), the new one moved to its place, the old one given back
//! if that fails ([`put_in_place`]). Then the app quits and starts again ([`restart`]): on
//! macOS opened by the system (`open`), so the new app answers for itself (what it was
//! allowed is kept); elsewhere the new program started at once. Started so, it waits for
//! the old process to end ([`WAIT_ARG`], [`wait_for`]) before anything else, and reopens
//! the session. What's set aside goes at its next start ([`clean_left`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::stage;

/// Started again with the session to reopen (its id, 32 hex digits).
pub const REOPEN_ARG: &str = "--reopen-session";
/// Started again with the process to wait for first (the app before the update: its id).
pub const WAIT_ARG: &str = "--wait-for";
/// How the last update went, in the updates' folder.
pub const RESULT: &str = "result.toml";
/// How long a new start waits for the old process at most.
const WAIT_MOST: Duration = Duration::from_secs(30);

/// What's replaced for the program at `exe`: its bundle on macOS (`…/Name.app/Contents/
/// MacOS/name`), the program itself elsewhere.
pub fn target(exe: &Path) -> PathBuf {
    let bundle = exe.parent().and_then(Path::parent).and_then(Path::parent);
    match bundle {
        Some(b) if cfg!(target_os = "macos") && b.extension().is_some_and(|e| e == "app") => {
            b.to_path_buf()
        }
        _ => exe.to_path_buf(),
    }
}

/// This app's place ([`target`] of this program), known from its start: once an update
/// is in place, this program's own path may name the one set aside (Linux, Windows).
pub fn place() -> Option<PathBuf> {
    static PLACE: OnceLock<Option<PathBuf>> = OnceLock::new();
    PLACE.get_or_init(|| std::env::current_exe().ok().map(|exe| target(&exe))).clone()
}

/// Whether the app at `target` can be replaced by this user: its folder takes a new file
/// (`/Applications` of another user, `Program Files`, a read-only volume don't).
pub fn writable(target: &Path) -> bool {
    let Some(dir) = target.parent() else { return false };
    let probe = dir.join(format!(".{}-update-{}", kernel::brand::ID, kernel::id::new_uuid()));
    let made = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    made
}

/// Whether the app was installed by its installer (Windows: its uninstaller beside it):
/// it's updated through the installer then (Settings › Apps keeps its version).
pub fn installed_by_setup() -> bool {
    cfg!(windows)
        && place()
            .and_then(|p| p.parent().map(|d| d.join("unins000.exe")))
            .is_some_and(|u| u.exists())
}

/// The release files this app can update from, best first: an app the installer installed
/// takes the installer (`windows-x64-setup`), then the archive.
pub fn platforms() -> Vec<&'static str> {
    let preferred = app_updates::platform::preferred(std::env::consts::OS, std::env::consts::ARCH);
    let mut out = Vec::with_capacity(preferred.len() + 1);
    if installed_by_setup() && preferred.contains(&"windows-x64") {
        out.push("windows-x64-setup");
    }
    out.extend_from_slice(preferred);
    out
}

/// The update staged in `dir` put in this app's place ([`place`]), the app still running;
/// how it went noted for its next start. The new version, or why not (the app as it was).
/// An installer moves nothing now: it runs as the app ends ([`restart`]).
pub fn put_in_place(dir: &Path) -> Result<String, String> {
    let target = place().ok_or("this program's path is unknown")?;
    let staged = stage::read(dir).ok_or_else(|| format!("{}: no update staged", dir.display()))?;
    let version = if staged.is_setup() { staged.version } else { apply(dir, &target)? };
    if let Some(root) = dir.parent()
        && let Err(e) = note(root, &Outcome { version: version.clone(), error: None })
    {
        tracing::warn!("how the update went couldn't be noted: {e}");
    }
    tracing::info!(%version, target = %target.display(), "the update is in place");
    Ok(version)
}

/// The app started again (this process ending: the new one waits for it), the session
/// `reopen` reopened by it — or, the update staged in `dir` being an installer, that run
/// (it waits for this process, installs, starts the app). `false` if it couldn't be.
pub fn restart(dir: &Path, reopen: Option<u128>) -> bool {
    let Some(target) = place() else { return false };
    let reopen = reopen.map(kernel::id::uuid_text);
    let pid = std::process::id();
    let (program, args) = match stage::read(dir).filter(stage::Staged::is_setup) {
        Some(setup) => (setup.app, setup_args(pid, reopen.as_deref())),
        None => restart_command(&target, pid, reopen.as_deref()),
    };
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut command);
    match command.spawn() {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(target = %target.display(), "the app couldn't be started again: {e}");
            false
        }
    }
}

/// What starts the app at `target` again: an app bundle opened by the system (`open -n`:
/// the new app answers for itself, never the old one), a program at once; told to wait for
/// `pid` and to reopen `reopen`.
pub fn restart_command(target: &Path, pid: u32, reopen: Option<&str>) -> (PathBuf, Vec<OsString>) {
    let mut args: Vec<OsString> = Vec::new();
    let bundle = target.extension().is_some_and(|e| e == "app");
    let program = if bundle && cfg!(target_os = "macos") {
        args.extend(["-n".into(), target.as_os_str().to_owned(), "--args".into()]);
        PathBuf::from("/usr/bin/open")
    } else if bundle {
        target.join("Contents").join("MacOS").join(kernel::brand::ID)
    } else {
        target.to_path_buf()
    };
    args.extend([WAIT_ARG.into(), pid.to_string().into()]);
    if let Some(id) = reopen {
        args.extend([REOPEN_ARG.into(), id.into()]);
    }
    (program, args)
}

/// The installer's arguments: silent, no restart of Windows; it waits for `pid` and
/// starts the app reopening `reopen` (`packaging/windows/installer.iss`).
pub fn setup_args(pid: u32, reopen: Option<&str>) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(format!("/WAITPID={pid}").into());
    if let Some(id) = reopen {
        args.push(format!("/REOPEN={id}").into());
    }
    args
}

/// Process `pid` waited for until it ends (at most [`WAIT_MOST`]): a new start after an
/// update, before anything else (the old one still closing would take its place).
pub fn wait_for(pid: u32) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    let since = Instant::now();
    while since.elapsed() < WAIT_MOST {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        if system.process(pid).is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    tracing::warn!(pid = pid.as_u32(), "the app before the update still runs: started anyway");
}

/// `command` started on its own: not in the app's process group (a Ctrl-C in the terminal
/// the app was started from, or the terminal closing, doesn't stop it midway).
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP.
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
}

/// How an update went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub version: String,
    /// Why it failed (`None`: it's in place).
    pub error: Option<String>,
}

/// The staged update in `dir` put at `target`: the app there set aside first, given back if
/// the new one can't take its place; once in place, the download goes (the old one at the
/// next start: it's still running). The new version on success.
pub fn apply(dir: &Path, target: &Path) -> Result<String, String> {
    let staged = stage::read(dir).ok_or_else(|| format!("{}: no update staged", dir.display()))?;
    let mut aside = aside(target);
    // One left by an earlier update that couldn't be removed (an exe still running on
    // Windows): another name, so it never blocks this one.
    if aside.exists() && remove(&aside).is_err() {
        aside =
            aside.with_extension(format!("old-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    }
    // Its files may be held a moment by another program (Windows: an antivirus, Explorer):
    // tried again.
    retry(|| std::fs::rename(target, &aside))
        .map_err(|e| format!("{} couldn't be set aside: {e}", target.display()))?;
    let placed = retry(|| std::fs::rename(&staged.app, target)).or_else(|_| {
        // Another volume: copied instead.
        copy_all(&staged.app, target)
    });
    if let Err(why) = placed {
        let _ = remove(target);
        let _ = std::fs::rename(&aside, target);
        return Err(format!("the new version couldn't be put in place: {why}"));
    }
    let _ = std::fs::remove_dir_all(dir);
    Ok(staged.version)
}

/// Where the app is set aside meanwhile: beside it (`Lumen.app.old`).
fn aside(target: &Path) -> PathBuf {
    let mut name = target.file_name().map(OsString::from).unwrap_or_default();
    name.push(".old");
    target.with_file_name(name)
}

/// `step` tried again while what it moves may still be held (Windows, a moment after the app
/// ends); another volume (or anything that won't change by waiting) given up at once.
fn retry(mut step: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut last = None;
    for _ in 0..40 {
        match step() {
            Ok(()) => return Ok(()),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::CrossesDevices | std::io::ErrorKind::NotFound
                ) =>
            {
                return Err(e);
            }
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("never tried")))
}

fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

/// `from` (a file or a folder) copied to `to`, its files' permissions kept; links skipped.
fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    let kind = std::fs::symlink_metadata(from)?.file_type();
    if kind.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else if kind.is_file() {
        std::fs::copy(from, to).map(drop)
    } else {
        Ok(())
    }
}

/// What earlier updates left beside the app at `target` (`Lumen.app.old`, `….old-<id>`: set
/// aside, not removable then) removed — best effort, at the app's start.
pub fn clean_left(target: &Path) {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else { return };
    let prefix = format!("{}.old", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = remove(&entry.path());
        }
    }
}

/// `outcome` noted in the updates' folder `root`, for the app's next start.
fn note(root: &Path, outcome: &Outcome) -> std::io::Result<()> {
    let mut table = toml::Table::new();
    table.insert("version".into(), toml::Value::String(outcome.version.clone()));
    if let Some(error) = &outcome.error {
        table.insert("error".into(), toml::Value::String(error.clone()));
    }
    std::fs::create_dir_all(root)?;
    files::write_atomic(&root.join(RESULT), toml::to_string(&table).unwrap_or_default().as_bytes())
}

/// How the last update went (told once: the note goes), and the helpers' copies left in the
/// temporary folder removed.
pub fn take_result(root: &Path) -> Option<Outcome> {
    clean_helpers();
    let path = root.join(RESULT);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let table: toml::Table = text.parse().ok()?;
    let version = table.get("version")?.as_str()?.to_owned();
    let mut error = table.get("error").and_then(toml::Value::as_str).map(str::to_owned);
    // Noted before it ran (an installer), but this isn't that version: it didn't finish.
    let running = kernel::brand::version();
    if error.is_none() && running != "0.0.0" && version != running {
        error = Some("the installer didn't finish; the app is still the version it was".into());
    }
    Some(Outcome { version, error })
}

/// The copies of the program earlier versions left in the temporary folder (their update
/// helper: one can't delete itself while it runs, on Windows).
fn clean_helpers() {
    let prefix = format!("{}-update-", kernel::brand::ID);
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/install.rs"]
mod tests;
