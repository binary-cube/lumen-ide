use super::*;
use crate::fake::{Decorated, FakeRepository};
use workbench_model::{EditorHost, RefactoringHost, ServicesHost, SystemHost, ViewHost};

use std::sync::Arc;

/// A workbench with one open file, its committed text read by the module's decorations.
struct Host {
    text: String,
    line: usize,
    notes: Vec<String>,
    choices: Vec<String>,
    decorated: Decorated,
}

impl Host {
    /// `a.php` open as `text`, committed as `base` (`None`: not in the last commit).
    fn new(text: &str, base: Option<&str>, line: usize) -> Self {
        let repo = FakeRepository {
            committed_texts: base.map(|b| ("a.php".into(), b.into())).into_iter().collect(),
            ..FakeRepository::default()
        };
        *repo.head.lock().unwrap() = Some("h1".into());
        Self {
            text: text.into(),
            line,
            notes: Vec::new(),
            choices: Vec::new(),
            decorated: Decorated::new(repo, text),
        }
    }
}

impl WorkbenchHost for Host {
    fn project_root(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn project_id(&self) -> Option<kernel::ProjectId> {
        Some(self.decorated.project)
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        Some(workbench_model::ActiveFile {
            path: "/p/a.php".into(),
            text: self.text.as_str().into(),
            line: self.line,
            cursor: 0,
            selection: None,
        })
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        Some(Arc::new(crate::fake::FakeRepository::default()))
    }
    fn choose(&mut self, list: ChoiceList, _: workbench_model::OnChoice) -> bool {
        self.choices = list.items.into_iter().map(|i| i.label).collect();
        true
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, notification: workbench_model::Notification) {
        self.notes.push(notification.title);
    }
    fn events(&self) -> &kernel::EventBus {
        &self.decorated.events
    }
    fn project_storage(&self) -> Option<std::path::PathBuf> {
        None
    }
}

impl ViewHost for Host {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
}

impl EditorHost for Host {
    fn active_document(&self) -> Option<kernel::DocumentId> {
        Some(self.decorated.doc)
    }
    fn edit_active(&mut self, range: std::ops::Range<usize>, text: &str) -> bool {
        self.text.replace_range(range, text);
        true
    }
}

impl RefactoringHost for Host {}
impl ServicesHost for Host {}
impl SystemHost for Host {}

#[test]
fn the_change_at_the_cursor_is_reverted() {
    let mut host = Host::new("a\nB\nc\n", Some("a\nb\nc\n"), 1);
    assert!(RevertChange.enabled(&host));
    assert!(RevertChange.run(&mut host));
    assert_eq!(host.text, "a\nb\nc\n");
    // Nothing changed there any more.
    assert!(!RevertChange.run(&mut host));
}

#[test]
fn a_gutter_bar_offers_revert_and_compare() {
    let mut host = Host::new("a\nB\nc\n", Some("a\nb\nc\n"), 1);
    assert!(ChangeActions.run(&mut host));
    assert_eq!(host.choices.len(), 2);
    // A file not in the last commit: only comparing.
    let mut host = Host::new("a\nB\nc\n", None, 1);
    ChangeActions.run(&mut host);
    assert_eq!(host.choices.len(), 1);
}
