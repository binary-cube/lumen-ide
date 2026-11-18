//! The options of the project's own bundles, as its `bin/console config:dump-reference`
//! prints them, kept in a file of the project's folder. Asked for by a command, never on its
//! own: it runs the project's code. The suggestions only read the file.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use code_intelligence::{CompletionItem, CompletionKind, suggest};
use i18n::{t, tr};
use workbench_model::{CommandHandler, Completion, Notification, WorkbenchHost};

/// Where what was read is kept, in the project's folder.
const FILE: &str = "symfony-config-reference.yaml";
/// A name standing for any key (a firewall's, a handler's).
const ANY: &str = "*";
/// Where the bundles are declared and the console that describes them.
const BUNDLES: &str = "config/bundles.php";
const CONSOLE: &str = "bin/console";
/// How long one bundle may take.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Where PHP usually is when the app doesn't have the shell's `PATH`.
const USUAL_PHP: &[&str] = &["/opt/homebrew/bin/php", "/usr/local/bin/php", "/usr/bin/php"];

/// The file kept for project `root`.
pub(crate) fn file_of(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join(FILE)
}

/// The options known, by the dotted path of what holds them (`""`: the bundles).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Reference {
    children: HashMap<String, BTreeSet<String>>,
}

/// The key a line declares, if it's one (`name:`, `name: value`).
fn key_of(body: &str) -> Option<&str> {
    let (key, _) = body.split_once(':')?;
    let key = key.trim_matches(['\'', '"']);
    let fits = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '@');
    (!key.is_empty() && key.chars().all(fits)).then_some(key)
}

impl Reference {
    /// What the console printed, for one or several bundles. `# Prototype` says the next
    /// key stands for any name.
    pub(crate) fn parse(text: &str) -> Self {
        let mut reference = Self::default();
        let mut open: Vec<(usize, String)> = Vec::new();
        let mut prototype = false;
        for line in text.lines() {
            let body = line.trim_start();
            if body.starts_with('#') {
                prototype =
                    prototype || body.trim_start_matches(['#', ' ']).starts_with("Prototype");
                continue;
            }
            if body.is_empty() {
                continue;
            }
            let was_prototype = std::mem::take(&mut prototype);
            let Some(key) = key_of(body) else { continue };
            let depth = line.len() - body.len();
            while open.last().is_some_and(|(d, _)| *d >= depth) {
                open.pop();
            }
            let parent = open.last().map_or(String::new(), |(_, path)| path.clone());
            let name = if was_prototype { ANY } else { key };
            reference.children.entry(parent.clone()).or_default().insert(name.to_owned());
            let path = if parent.is_empty() { name.to_owned() } else { format!("{parent}.{name}") };
            open.push((depth, path));
        }
        reference
    }

    /// What's known under the keys `path`, each matched by name or by a prototype.
    fn under(&self, path: &[String]) -> Option<&BTreeSet<String>> {
        let mut at = String::new();
        for key in path {
            let children = self.children.get(&at)?;
            let step = if children.contains(key) { key.as_str() } else { ANY };
            children.contains(step).then_some(())?;
            at = if at.is_empty() { step.to_owned() } else { format!("{at}.{step}") };
        }
        self.children.get(&at)
    }

    /// The keys that may follow `path`, matching `query`.
    pub(crate) fn suggestions(&self, path: &[String], query: &str) -> Vec<CompletionItem> {
        let Some(keys) = self.under(path) else { return Vec::new() };
        let names = keys.iter().map(String::as_str).filter(|k| *k != ANY);
        suggest(query, names, CompletionKind::Property, None)
    }
}

/// The keys holding a key starting at `start`, outermost first; `when@…` isn't one of them.
/// A key inside a list has none (`None`).
pub(crate) fn path_at(text: &str, start: usize) -> Option<Vec<String>> {
    let line = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let mut indent = text[line..start].len();
    let mut path = Vec::new();
    for above in text[..line].lines().rev() {
        let body = above.trim_start();
        let depth = above.len() - body.len();
        if body.is_empty() || body.starts_with('#') || depth >= indent {
            continue;
        }
        if body.starts_with('-') {
            return None;
        }
        path.push(key_of(body)?.to_owned());
        indent = depth;
        if depth == 0 {
            break;
        }
    }
    if path.last().is_some_and(|root| root.starts_with("when@")) {
        path.pop();
    }
    path.reverse();
    Some(path)
}

/// The bundles `config/bundles.php` declares, by their short names.
pub(crate) fn bundles(php: &str) -> Vec<String> {
    let mut found: Vec<String> = php
        .lines()
        .filter_map(|line| {
            let class = line.split("::class").next().filter(|_| line.contains("::class"))?;
            let name = class.trim().rsplit('\\').next()?;
            (!name.is_empty() && name.ends_with("Bundle")).then(|| name.to_owned())
        })
        .collect();
    found.dedup();
    found
}

fn php() -> Option<PathBuf> {
    process::which("php").or_else(|| USUAL_PHP.iter().map(PathBuf::from).find(|p| p.is_file()))
}

/// How it went: how many bundles were read, or why none was.
fn read_all(root: &Path, php: &Path) -> Result<usize, String> {
    if !root.join(CONSOLE).is_file() {
        return Err(t("symfony.no_console"));
    }
    let declared = std::fs::read_to_string(root.join(BUNDLES)).map_err(|e| e.to_string())?;
    let (mut all, mut read, mut last_error) = (String::new(), 0, None);
    for bundle in bundles(&declared) {
        let args = [CONSOLE, "config:dump-reference", &bundle, "--format=yaml", "--env=dev", "-n"];
        let run = process::Run::new(php, args).dir(root).timeout(TIMEOUT).owner("symfony");
        match process::run(&run) {
            Ok(out) if out.success() => {
                all.push_str(&out.stdout);
                all.push('\n');
                read += 1;
            }
            // Some bundles have no options.
            Ok(out) => last_error = Some(out.summary()),
            Err(e) => return Err(format!("{e:?}")),
        }
    }
    if read == 0 {
        return Err(last_error.unwrap_or_default());
    }
    let file = file_of(root);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(file, all).map_err(|e| e.to_string())?;
    Ok(read)
}

/// "Read Symfony's Configuration Options": runs the project's console in the background.
pub(crate) struct ReadReference;

impl CommandHandler for ReadReference {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        // Whether it has a console is told when it runs: a menu never reads the disk.
        host.project_root().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(root) = host.project_root() else { return false };
        let Some(php) = php() else {
            host.notify(Notification::error(t("symfony.no_php")));
            return false;
        };
        let work = Notification::progress(t("symfony.reading_reference"));
        let id = work.id;
        host.notify(work);
        host.spawn(Box::new(move || {
            let result = read_all(&root, &php);
            Box::new(move |host: &mut dyn WorkbenchHost| {
                let note = match result {
                    Ok(n) => Notification::success(tr!("symfony.reference_read", count = n)),
                    Err(e) => Notification::error(tr!("symfony.reference_failed", detail = e)),
                };
                host.update_notification(note.with_id(id));
            }) as Completion
        }));
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/config_reference.rs"]
mod tests;
