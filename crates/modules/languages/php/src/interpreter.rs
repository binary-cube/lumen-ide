//! The installed PHP interpreter (the CLI interpreter): its version is the
//! default level for projects without `composer.json`. Searched once per process: in PATH
//! and in the usual locations (an app launched from Finder lacks the shell's PATH).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

/// Full version of the found interpreter (e.g. `8.3.6`) and its path.
pub fn installed() -> Option<&'static (String, PathBuf)> {
    FOUND.get_or_init(find).as_ref()
}

/// The interpreter, if it was looked for already (never runs it: `None` until then).
pub fn known() -> Option<&'static (String, PathBuf)> {
    FOUND.get().and_then(Option::as_ref)
}

static FOUND: OnceLock<Option<(String, PathBuf)>> = OnceLock::new();

fn candidates() -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from(if cfg!(windows) { "php.exe" } else { "php" })];
    if cfg!(target_os = "macos") {
        out.extend(
            ["/opt/homebrew/bin/php", "/usr/local/bin/php", "/usr/bin/php"].map(PathBuf::from),
        );
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home).join("Library/Application Support/Herd/bin/php"));
        }
    } else if cfg!(windows) {
        out.extend(
            [r"C:\php\php.exe", r"C:\xampp\php\php.exe", r"C:\wamp64\bin\php\php.exe"]
                .map(PathBuf::from),
        );
        // Laragon: C:\laragon\bin\php\php-8.x…\php.exe
        if let Ok(dirs) = std::fs::read_dir(r"C:\laragon\bin\php") {
            let mut found: Vec<PathBuf> =
                dirs.flatten().map(|d| d.path().join("php.exe")).filter(|p| p.is_file()).collect();
            found.sort();
            out.extend(found.into_iter().rev());
        }
    } else {
        out.extend(["/usr/bin/php", "/usr/local/bin/php"].map(PathBuf::from));
    }
    out
}

fn find() -> Option<(String, PathBuf)> {
    candidates().into_iter().find_map(|path| Some((version(&path)?, path)))
}

/// `php -r 'echo PHP_VERSION;'`, with a timeout (a stuck `php` does not block the editor).
fn version(path: &PathBuf) -> Option<String> {
    let run = process::Run::new(path, ["-n", "-r", "echo PHP_VERSION;"])
        .timeout(Duration::from_secs(3))
        .owner("php");
    let output = process::run(&run).ok()?;
    let text = output.stdout.trim();
    (output.success() && text.chars().next()?.is_ascii_digit()).then(|| text.to_owned())
}
