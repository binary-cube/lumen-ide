//! An update put in place, once the app closed (plan 84f). The app, quitting, starts the
//! helper ([`start_helper`]): a copy of its own program in the temporary folder (so it
//! doesn't run from what it replaces), told the staged update, where the app is and the
//! session to reopen, with a pipe the app holds until it ends. The helper ([`apply_main`])
//! waits for that pipe to close (the app gone, on every system alike), sets the app aside,
//! puts the new one in its place — the old one given back if that fails —, notes how it
//! went ([`take_result`]: told by the app at its next start) and starts the app again, which
//! reopens its windows.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::stage::{self, Staged};

/// The helper's first argument (then the staged folder).
pub const HELPER_ARG: &str = "--apply-update";
/// Where the app is (what's replaced).
pub const TARGET_ARG: &str = "--target";
/// Started again with the session to reopen (its id, 32 hex digits).
pub const REOPEN_ARG: &str = "--reopen-session";
/// How the last update went, in the updates' folder.
pub const RESULT: &str = "result.toml";

/// The pipes to the helpers started: held until this process ends (the helper waits for
/// that).
static HELD: Mutex<Vec<ChildStdin>> = Mutex::new(Vec::new());

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

/// Whether the app at `target` can be replaced by this user: its folder takes a new file
/// (`/Applications` of another user, `Program Files`, a read-only volume don't).
pub fn writable(target: &Path) -> bool {
    let Some(dir) = target.parent() else { return false };
    let probe = dir.join(format!(".{}-update-{}", kernel::brand::ID, kernel::id::new_uuid()));
    let made = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    made
}

/// The helper started for `staged`, the app at `target` (the session `reopen` reopened
/// after): it waits until this process ends.
pub fn start_helper(staged: &Staged, target: &Path, reopen: Option<u128>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("this program: {e}"))?;
    let helper = std::env::temp_dir().join(format!(
        "{}-update-{}{}",
        kernel::brand::ID,
        kernel::id::uuid_text(kernel::id::new_uuid()),
        std::env::consts::EXE_SUFFIX
    ));
    std::fs::copy(&exe, &helper).map_err(|e| format!("{}: {e}", helper.display()))?;
    let mut command = Command::new(&helper);
    command.arg(HELPER_ARG).arg(&staged.dir).arg(TARGET_ARG).arg(target);
    if let Some(id) = reopen {
        command.arg(REOPEN_ARG).arg(kernel::id::uuid_text(id));
    }
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut command);
    let mut child = command.spawn().map_err(|e| format!("{}: {e}", helper.display()))?;
    if let Some(pipe) = child.stdin.take() {
        HELD.lock().unwrap_or_else(PoisonError::into_inner).push(pipe);
    }
    tracing::info!(version = %staged.version, target = %target.display(), "the update's helper started");
    Ok(())
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

/// The helper (`args`: what [`start_helper`] gave, after the program): waits for the app to
/// end, puts the update in place, notes how it went, starts the app again. Its exit code.
pub fn apply_main(args: &[OsString]) -> i32 {
    let mut args = args.iter();
    let (Some(_), Some(dir)) = (args.next(), args.next()) else { return 2 };
    let dir = PathBuf::from(dir);
    let mut target = None;
    let mut reopen = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some(TARGET_ARG) => target = args.next().map(PathBuf::from),
            Some(REOPEN_ARG) => reopen = args.next().and_then(|a| a.to_str()).map(str::to_owned),
            _ => {}
        }
    }
    let Some(target) = target else { return 2 };
    // The app ends: its end of the pipe closes (whatever ends it).
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
    std::thread::sleep(Duration::from_millis(300));
    let outcome = match apply(&dir, &target) {
        Ok(version) => Outcome { version, error: None },
        Err(why) => Outcome {
            version: stage::read(&dir).map(|s| s.version).unwrap_or_default(),
            error: Some(why),
        },
    };
    if let Some(root) = dir.parent() {
        let _ = note(root, &outcome);
    }
    relaunch(&target, reopen.as_deref());
    i32::from(outcome.error.is_some())
}

/// The staged update in `dir` put at `target`: the app there set aside first, given back if
/// the new one can't take its place; once in place, the old one and the download go.
/// The new version on success.
pub fn apply(dir: &Path, target: &Path) -> Result<String, String> {
    let staged = stage::read(dir).ok_or_else(|| format!("{}: no update staged", dir.display()))?;
    let mut aside = aside(target);
    // One left by an earlier update that couldn't be removed (an exe still running on
    // Windows): another name, so it never blocks this one.
    if aside.exists() && remove(&aside).is_err() {
        aside =
            aside.with_extension(format!("old-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    }
    // The app's files may still be held a moment after it ends (Windows): tried again.
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
    let _ = remove(&aside);
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

/// The app at `target` started again (the session `reopen` reopened by it), on its own.
fn relaunch(target: &Path, reopen: Option<&str>) {
    let program = if target.extension().is_some_and(|e| e == "app") {
        target.join("Contents").join("MacOS").join(kernel::brand::ID)
    } else {
        target.to_path_buf()
    };
    let mut command = Command::new(program);
    if let Some(id) = reopen {
        command.arg(REOPEN_ARG).arg(id);
    }
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut command);
    let _ = command.spawn();
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
    Some(Outcome {
        version: table.get("version")?.as_str()?.to_owned(),
        error: table.get("error").and_then(toml::Value::as_str).map(str::to_owned),
    })
}

/// The helpers' copies of the program left in the temporary folder (one can't delete
/// itself while it runs, on Windows).
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
