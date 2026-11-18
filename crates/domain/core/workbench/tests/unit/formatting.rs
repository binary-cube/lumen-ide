use std::path::{Path, PathBuf};
use std::sync::Mutex;

use code_quality::TextEdit;

use super::*;
use crate::{ActiveFile, Job, Notification};

/// An editor with one file; its jobs run when asked.
struct Editor {
    file: ActiveFile,
    events: kernel::EventBus,
    jobs: Vec<Job>,
}

impl Editor {
    fn new(text: &str) -> Self {
        Self {
            file: ActiveFile {
                path: PathBuf::from("/p/q.sql"),
                text: text.into(),
                line: 0,
                cursor: 0,
                selection: None,
            },
            events: kernel::EventBus::default(),
            jobs: Vec::new(),
        }
    }
}

impl WorkbenchHost for Editor {
    fn project_root(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/p"))
    }
    fn active_file(&self) -> Option<ActiveFile> {
        Some(self.file.clone())
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn project_storage(&self) -> Option<PathBuf> {
        None
    }
    fn language_of(&self, _: &Path) -> Option<String> {
        Some("sql".into())
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn choose(&mut self, _: crate::ChoiceList, _: crate::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, job: Job) {
        self.jobs.push(job);
    }
    fn notify(&mut self, _: Notification) {}
}

impl crate::ViewHost for Editor {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
}

impl crate::EditorHost for Editor {
    fn edit_active(&mut self, range: std::ops::Range<usize>, text: &str) -> bool {
        let mut new = self.file.text.to_string();
        new.replace_range(range, text);
        self.file.text = new.into();
        true
    }
}

impl crate::RefactoringHost for Editor {}
impl crate::ServicesHost for Editor {}
impl crate::SystemHost for Editor {}

/// Uppercases the text (what was asked of it noted).
#[derive(Debug, Default)]
struct Upper {
    asked: Mutex<Vec<FormatScope>>,
}

impl TextFormatter for Upper {
    fn languages(&self) -> &'static [&'static str] {
        &["sql"]
    }
    fn format(&self, request: &FormatRequest) -> Formatted {
        self.asked.lock().unwrap().push(request.scope.clone());
        let upper = request.text.to_uppercase();
        if upper == *request.text {
            return Formatted::Unchanged { left: 0 };
        }
        Formatted::Changed { edit: TextEdit { range: 0..request.text.len(), text: upper }, left: 1 }
    }
}

static OUTCOMES: Mutex<Vec<FormatOutcome>> = Mutex::new(Vec::new());

fn noted(_: &mut dyn WorkbenchHost, outcome: FormatOutcome) {
    OUTCOMES.lock().unwrap().push(outcome);
}

fn taken() -> Vec<FormatOutcome> {
    std::mem::take(&mut *OUTCOMES.lock().unwrap())
}

#[test]
fn the_active_text_is_changed_and_said_how_it_went() {
    // One test owns the outcomes (the tests run in parallel).
    let upper = Arc::new(Upper::default());
    let mut editor = Editor::new("select 1");
    assert!(format_active(&mut editor, upper.clone(), FormatScope::At(3), noted));
    assert_eq!(&*editor.file.text, "SELECT 1");
    assert_eq!(*upper.asked.lock().unwrap(), [FormatScope::At(3)]);
    assert!(format_active(&mut editor, upper.clone(), FormatScope::Whole, noted));
    assert_eq!(taken(), [FormatOutcome::Changed { left: 1 }, FormatOutcome::Unchanged { left: 0 }]);

    // A large text is laid out in the background, changed only if it's still as it was.
    let big = "x".repeat(IN_BACKGROUND);
    let mut editor = Editor::new(&big);
    assert!(format_active(&mut editor, upper.clone(), FormatScope::Whole, noted));
    assert_eq!(&*editor.file.text, big, "nothing yet");
    let job = editor.jobs.pop().unwrap();
    let done = job();
    done(&mut editor);
    assert_eq!(&*editor.file.text, big.to_uppercase());
    let mut editor = Editor::new(&big);
    format_active(&mut editor, upper, FormatScope::Whole, noted);
    let done = editor.jobs.pop().unwrap()();
    editor.file.text = "typed meanwhile".into();
    done(&mut editor);
    assert_eq!(&*editor.file.text, "typed meanwhile");
    assert_eq!(taken(), [FormatOutcome::Changed { left: 1 }, FormatOutcome::Moved]);
}
