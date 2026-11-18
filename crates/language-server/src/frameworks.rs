//! The project's frameworks and the modules' help with documents, asked as the editor asks
//! them. A framework's model is made again when the disk changed, before the first answer
//! that needs it: this program has no screen to keep moving.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use code_intelligence::{
    AssistContext, DocumentAssist, FrameworkCompletion, FrameworkContext, FrameworkSupport,
    active_frameworks,
};
use std::io::Write;

use crate::answers::At;
use crate::session::Session;

/// What the project's frameworks need, per session.
#[derive(Debug, Default)]
pub struct Frameworks {
    active: Vec<Arc<dyn FrameworkSupport>>,
    /// Changes when a file changes on the disk.
    disk: AtomicU64,
    /// The disk's generation the frameworks' models were made for, plus one (0: never).
    prepared: AtomicU64,
}

impl Frameworks {
    pub(crate) fn disk_changed(&self) {
        self.disk.fetch_add(1, Ordering::Relaxed);
    }
}

impl<W: Write> Session<W> {
    /// The frameworks the project uses, activated.
    pub(crate) fn start_frameworks(&mut self) {
        let known = self.engines.frameworks.clone();
        let active = self.with_project(|project| {
            let detected: Vec<(String, String)> = known
                .iter()
                .filter_map(|f| Some((f.id().to_owned(), f.detect(project)?)))
                .collect();
            let ids = active_frameworks(&known, &detected, &[]);
            let on = known.iter().filter(|f| ids.contains(&f.id()));
            on.map(|f| f.activate(project)).collect()
        });
        self.frameworks.active = active;
    }

    fn with_framework_at<R>(
        &self,
        path: &Path,
        language: &str,
        text: &str,
        offset: usize,
        ask: impl Fn(&dyn FrameworkSupport, &FrameworkContext<'_>) -> Option<R>,
    ) -> Option<R> {
        let frameworks = &self.frameworks;
        if frameworks.active.is_empty() {
            return None;
        }
        let disk = frameworks.disk.load(Ordering::Relaxed);
        self.with_project(|project| {
            let at = FrameworkContext {
                project,
                path,
                language,
                text,
                offset,
                generation: disk,
                structure: disk,
                own: disk,
            };
            if frameworks.prepared.swap(disk + 1, Ordering::Relaxed) != disk + 1 {
                frameworks.active.iter().for_each(|f| f.prepare(&at));
            }
            frameworks.active.iter().find_map(|f| ask(f.as_ref(), &at))
        })
    }

    fn with_assists_at<R>(
        &self,
        at: &At,
        ask: impl Fn(&dyn DocumentAssist, &AssistContext<'_>) -> Option<R>,
    ) -> Option<R> {
        let version = self.open.get(&at.path).map_or(0, |o| o.version.unsigned_abs());
        let context = AssistContext {
            project: None,
            root: self.root.as_deref(),
            path: &at.path,
            language: &at.language,
            text: &at.text,
            offset: at.offset,
            version,
        };
        let fits = |a: &&Arc<dyn DocumentAssist>| a.asked_for(&at.language, &at.language);
        self.engines.assists.iter().filter(fits).find_map(|a| ask(a.as_ref(), &context))
    }

    fn ask_framework<R>(
        &self,
        at: &At,
        ask: impl Fn(&dyn FrameworkSupport, &FrameworkContext<'_>) -> Option<R>,
    ) -> Option<R> {
        self.with_framework_at(&at.path, &at.language, &at.text, at.offset, ask)
    }

    /// What the modules and the frameworks suggest at `at`.
    pub(crate) fn help_completion(&self, at: &At) -> Option<FrameworkCompletion> {
        code_intelligence::template::join_suggestions(
            self.with_assists_at(at, |a, c| a.complete(c)),
            self.ask_framework(at, |f, c| f.complete(c)),
        )
    }

    pub(crate) fn help_hover(&self, at: &At) -> Option<code_intelligence::HoverInfo> {
        self.with_assists_at(at, |a, c| a.hover(c))
            .or_else(|| self.ask_framework(at, |f, c| f.hover(c)))
    }

    pub(crate) fn help_definition(&self, at: &At) -> Option<code_intelligence::Location> {
        let assist = self.with_assists_at(at, |a, c| a.definition(c));
        assist
            .and_then(|target| target.first_place().cloned())
            .or_else(|| self.ask_framework(at, |f, c| f.definition(c)))
    }
}
