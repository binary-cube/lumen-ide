//! "What's Indexed" in the settings: the project's marks with what each holds, the folders the
//! modules leave out on their own, and the large folders worth excluding. What a folder holds
//! is counted in the background, once per look at the page.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use i18n::{bytes, t, tr};
use settings_model::SettingsService;
use workbench_model::{
    OnYes, PerProject, ProjectContext, Question, SettingsBlock, SettingsContribution,
    SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};
use workspace::marks::{PathMark, PathMarks, ProjectMarks};

use crate::apply;

/// A folder is worth excluding from this size on, or this many files.
const LARGE_BYTES: u64 = 1 << 30;
const LARGE_FILES: u64 = 20_000;
/// How deep a suggestion goes.
const SUGGESTED_DEPTH: usize = 3;
/// A folder with a child holding this share of it: the child is what's suggested.
const DOMINANT: u64 = 7;
const MAX_SUGGESTIONS: usize = 6;
/// A count never takes longer than this (the rest isn't counted).
const SCAN_FOR: Duration = Duration::from_secs(30);

/// The buttons' actions: one per row (an action is a `&'static str`).
const REMOVE: [&str; 24] = [
    "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12", "r13", "r14",
    "r15", "r16", "r17", "r18", "r19", "r20", "r21", "r22", "r23",
];
const EXCLUDE: [&str; MAX_SUGGESTIONS] = ["e0", "e1", "e2", "e3", "e4", "e5"];

/// What a folder or a file holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Held {
    pub files: u64,
    pub bytes: u64,
}

/// What a project's folders hold, as last counted.
#[derive(Debug, Default)]
struct Scan {
    held: HashMap<String, Held>,
    suggestions: Vec<(String, Held)>,
}

/// An open project, as the page needs it.
pub(crate) struct Entry {
    root: std::path::PathBuf,
    pub(crate) marks: ProjectMarks,
    events: kernel::EventBus,
    settings: Option<Arc<SettingsService>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    scan: Mutex<Option<Arc<Scan>>>,
    /// What the symbol index takes per folder, as last measured.
    memory: Mutex<Arc<HashMap<String, u64>>>,
    scanning: AtomicBool,
    fresh: AtomicBool,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry").field("root", &self.root).finish_non_exhaustive()
    }
}

static PROJECTS: PerProject<Entry> = PerProject::new();

/// Follows project `context` while the subscriptions live.
pub(crate) fn attach(context: &ProjectContext<'_>) -> Vec<kernel::Subscription> {
    let entry = Arc::new(Entry {
        root: context.root.to_path_buf(),
        marks: context.marks.clone(),
        events: context.events.clone(),
        settings: context.settings.clone(),
        wake: Arc::clone(&context.wake),
        scan: Mutex::default(),
        memory: Mutex::default(),
        scanning: AtomicBool::new(false),
        fresh: AtomicBool::new(false),
    });
    PROJECTS.insert(context.id, &entry);
    crate::tell_libraries(&entry.root, &entry.marks.get());
    let id = context.id;
    let changed = Arc::clone(&entry);
    let measured = Arc::clone(&entry);
    let on_disk = Arc::clone(&entry);
    let file = entry.root.join(kernel::brand::PROJECT_DIR).join(workspace::marks::FILE_NAME);
    vec![
        context.events.subscribe(move |event: &workspace::events::OwnFilesWritten| {
            if !event.paths.contains(&file) {
                return;
            }
            let (entry, file) = (Arc::clone(&on_disk), file.clone());
            kernel::work::run("path-marks", move || {
                follow_file(&file, &entry.marks, &entry.events);
            });
        }),
        context.events.subscribe(move |event: &workspace::events::IndexMemoryMeasured| {
            *measured.memory.lock().unwrap_or_else(PoisonError::into_inner) =
                Arc::clone(&event.folders);
            measured.fresh.store(true, Ordering::Release);
            (measured.wake)();
        }),
        context.events.subscribe(move |_: &workspace::events::PathMarksChanged| {
            *changed.scan.lock().unwrap_or_else(PoisonError::into_inner) = None;
            crate::tell_libraries(&changed.root, &changed.marks.get());
        }),
        kernel::Subscription::on_end(move || {
            kernel::paths::set_libraries(&entry.root, Vec::new());
            PROJECTS.remove(id);
        }),
    ]
}

/// The marks as `file` has them now, if that's not what's held (written by hand): told.
pub(crate) fn follow_file(file: &Path, marks: &ProjectMarks, events: &kernel::EventBus) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else { return false };
    let written = PathMarks::parse(&text);
    if written == *marks.get() {
        return false;
    }
    marks.replace(written);
    events.publish(&workspace::events::PathMarksChanged);
    true
}

/// What's under `root`, by folder (down to the depth suggestions go) and by what's marked.
pub(crate) fn count(root: &Path, marks: &PathMarks, until: Instant) -> HashMap<String, Held> {
    let mut held: HashMap<String, Held> = HashMap::new();
    let marked: Vec<&str> = marks.iter().map(|(path, _)| path).collect();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if Instant::now() >= until {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name();
            if kind.is_dir() && !workspace::search::skipped_dir(&name.to_string_lossy()) {
                stack.push(entry.path());
            } else if kind.is_file() {
                let path = entry.path();
                let Ok(relative) = path.strip_prefix(root) else { continue };
                let relative = relative.to_string_lossy().replace('\\', "/");
                let size = entry.metadata().map_or(0, |m| m.len());
                let mut add = |key: &str| {
                    let at = held.entry(key.to_owned()).or_default();
                    at.files += 1;
                    at.bytes += size;
                };
                for (i, _) in relative.match_indices('/').take(SUGGESTED_DEPTH) {
                    add(&relative[..i]);
                }
                for mark in &marked {
                    let inside = relative.strip_prefix(mark);
                    let deep = inside.is_some_and(|r| r.starts_with('/'));
                    if inside == Some("") || (deep && mark.matches('/').count() >= SUGGESTED_DEPTH)
                    {
                        add(mark);
                    }
                }
            }
        }
    }
    held
}

/// The large folders nobody marked yet: the deepest that holds most of what's large, the
/// libraries left alone (their classes are used).
pub(crate) fn suggestions(held: &HashMap<String, Held>, marks: &PathMarks) -> Vec<(String, Held)> {
    let large = |h: &Held| h.bytes >= LARGE_BYTES || h.files >= LARGE_FILES;
    let free = |path: &str| {
        marks.of(path).is_none()
            && path.split('/').all(|part| !kernel::paths::is_dependency_dir(part))
    };
    let candidates: Vec<(&String, &Held)> =
        held.iter().filter(|(path, h)| large(h) && free(path)).collect();
    let dominated = |path: &str, h: &Held| {
        let prefix = format!("{path}/");
        candidates
            .iter()
            .any(|(child, c)| child.starts_with(&prefix) && c.bytes * 10 >= h.bytes * DOMINANT)
    };
    let mut kept: Vec<(String, Held)> = candidates
        .iter()
        .filter(|(path, h)| !dominated(path, h))
        .map(|(path, h)| ((*path).clone(), **h))
        .collect();
    kept.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then_with(|| a.0.cmp(&b.0)));
    kept.truncate(MAX_SUGGESTIONS);
    kept
}

/// What a folder holds on disk, and in memory when the index has some of it.
fn described(held: Option<Held>, memory: Option<&u64>) -> String {
    let text = held_text(held);
    match memory.filter(|m| **m > 0) {
        Some(memory) => {
            tr!("marks.in_memory", holds = text.as_str(), size = bytes(*memory).as_str())
        }
        None => text,
    }
}

fn held_text(held: Option<Held>) -> String {
    held.map_or_else(
        || t("marks.counting"),
        |h| tr!("marks.holds", files = h.files as i64, size = bytes(h.bytes).as_str()),
    )
}

fn mark_name(mark: PathMark) -> String {
    t(match mark {
        PathMark::Excluded => "marks.state_excluded",
        PathMark::Library => "marks.state_library",
        PathMark::Generated => "marks.state_generated",
        PathMark::Normal => "marks.state_normal",
    })
}

/// The block on the settings' "What's Indexed" page.
#[derive(Debug, Default)]
pub(crate) struct IndexedPage;

impl IndexedPage {
    fn entry(scope: &SettingsScope<'_>) -> Option<Arc<Entry>> {
        let own = scope.project?;
        PROJECTS
            .all()
            .into_iter()
            .find(|e| e.settings.as_deref().is_some_and(|s| std::ptr::eq(s, own)))
    }

    /// What was counted; asks for a count when there's none.
    fn counted(entry: &Arc<Entry>) -> Option<Arc<Scan>> {
        let known = entry.scan.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if known.is_none() && !entry.scanning.swap(true, Ordering::AcqRel) {
            let entry = Arc::clone(entry);
            kernel::work::run("path-marks", move || {
                let marks = entry.marks.get();
                let held = count(&entry.root, &marks, Instant::now() + SCAN_FOR);
                let suggestions = suggestions(&held, &marks);
                let scan = Arc::new(Scan { held, suggestions });
                *entry.scan.lock().unwrap_or_else(PoisonError::into_inner) = Some(scan);
                entry.scanning.store(false, Ordering::Release);
                entry.fresh.store(true, Ordering::Release);
                (entry.wake)();
            });
        }
        known
    }
}

impl SettingsContribution for IndexedPage {
    fn id(&self) -> &'static str {
        "path-marks"
    }

    fn page(&self) -> &'static str {
        "indexed"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "indexed",
            label: "marks.page",
            icon: "folder",
            group: SettingsGroup::Tools,
            order: 55,
        })
    }

    fn poll(&self) -> bool {
        PROJECTS.all().iter().any(|e| e.fresh.swap(false, Ordering::AcqRel))
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let mut rows = Vec::new();
        if let Some(entry) = Self::entry(&scope) {
            let scan = Self::counted(&entry);
            let marks = entry.marks.get();
            let memory = Arc::clone(&*entry.memory.lock().unwrap_or_else(PoisonError::into_inner));
            if marks.is_empty() {
                rows.push(SettingsRow::Notes(vec![t("marks.none")]));
            }
            for (i, (path, mark)) in marks.iter().take(REMOVE.len()).enumerate() {
                let held = scan.as_ref().map(|s| s.held.get(path).copied().unwrap_or_default());
                let holds = described(held, memory.get(path));
                let text = format!("{path} · {} · {holds}", mark_name(mark));
                rows.push(SettingsRow::Actions {
                    text,
                    buttons: vec![(REMOVE[i], t("command.marks_clear"))],
                });
            }
            for (i, (path, held)) in scan.iter().flat_map(|s| &s.suggestions).enumerate() {
                let holds = described(Some(*held), memory.get(path));
                let text = tr!("marks.suggested", path = path.as_str(), holds = holds.as_str());
                rows.push(SettingsRow::Actions {
                    text,
                    buttons: vec![(EXCLUDE[i], t("command.marks_excluded"))],
                });
            }
        } else {
            rows.push(SettingsRow::Notes(vec![t("marks.no_project")]));
        }
        let rules = kernel::paths::skipped_from_search().join(", ");
        rows.push(SettingsRow::Notes(vec![tr!("marks.rules", folders = rules.as_str())]));
        SettingsBlock { title: t("marks.page"), detail: t("marks.page_detail"), rows }
    }

    fn choose(&self, _scope: SettingsScope<'_>, _key: &'static str, _value: i64) {}

    fn press(&self, scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        let entry = Self::entry(&scope)?;
        let marks = entry.marks.get();
        let at = |list: &[&str]| list.iter().position(|a| *a == action);
        let (target, mark) = if let Some(i) = at(&REMOVE) {
            (marks.iter().nth(i)?.0.to_owned(), None)
        } else {
            let scan = entry.scan.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
            (scan.suggestions.get(at(&EXCLUDE)?)?.0.clone(), Some(PathMark::Excluded))
        };
        apply(&entry.root, &entry.marks, &entry.events, &[target], mark, |e| {
            tracing::warn!(error = %e, "the path marks weren't saved");
        });
        None
    }
}

#[cfg(test)]
#[path = "../tests/unit/page.rs"]
mod tests;
