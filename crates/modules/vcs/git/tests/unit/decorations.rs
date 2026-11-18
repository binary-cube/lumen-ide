use super::*;
use crate::fake::{Decorated, FakeRepository, commit};
use workbench_model::{RefactoringHost, ServicesHost, SystemHost, ViewHost};

fn repo(head: &str, committed: &str) -> FakeRepository {
    let repo = FakeRepository {
        committed_texts: vec![("a.php".into(), committed.into())],
        blame: vec![
            BlameHunk { lines: 0..2, commit: commit("1111111aaa", "first") },
            BlameHunk { lines: 2..3, commit: commit("2222222bbb", "second") },
        ],
        ..FakeRepository::default()
    };
    *repo.head.lock().unwrap() = Some(head.into());
    repo
}

/// The workbench with `a.php` active in a project.
struct Host<'a> {
    decorated: &'a Decorated,
    line: usize,
}

impl WorkbenchHost for Host<'_> {
    fn project_root(&self) -> Option<std::path::PathBuf> {
        Some("/p".into())
    }
    fn project_id(&self) -> Option<ProjectId> {
        Some(self.decorated.project)
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        Some(workbench_model::ActiveFile {
            path: "/p/a.php".into(),
            text: self.decorated.text.to_string().into(),
            line: self.line,
            cursor: 0,
            selection: None,
        })
    }
    fn repository(&self) -> Option<Arc<dyn Repository>> {
        None
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, _: workbench_model::Notification) {}
    fn events(&self) -> &EventBus {
        &self.decorated.events
    }
    fn project_storage(&self) -> Option<std::path::PathBuf> {
        None
    }
}

impl ViewHost for Host<'_> {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
}

impl workbench_model::EditorHost for Host<'_> {
    fn active_document(&self) -> Option<DocumentId> {
        Some(self.decorated.doc)
    }
}

impl RefactoringHost for Host<'_> {}
impl ServicesHost for Host<'_> {}
impl SystemHost for Host<'_> {}

#[test]
fn the_lines_changed_since_the_last_commit_are_marked() {
    let decorated = Decorated::new(repo("h1", "a\nb\nc\n"), "a\nB\nc\nd\n");
    let marks = decorated.decorations.decorations(decorated.doc).unwrap().marks;
    let expected = vec![
        LineMark { lines: 1..2, kind: MarkKind::Modified },
        LineMark { lines: 3..4, kind: MarkKind::Added },
    ];
    assert_eq!(*marks, expected);
    // The branch, beside the project's name.
    assert_eq!(decorated.decorations.label().as_deref(), Some("main"));
    // The committed text, for the module's commands (revert, rollback).
    let host = Host { decorated: &decorated, line: 1 };
    assert_eq!(committed_text(&host).as_deref(), Some("a\nb\nc\n"));
}

/// Polled until nothing more comes for a while (the reads still going on taken), so what
/// follows starts from rest.
fn quiet(decorated: &mut Decorated) {
    let mut calm = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while calm < 20 && std::time::Instant::now() < deadline {
        calm = if decorated.decorations.poll() { 0 } else { calm + 1 };
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The repository looked at again, in the background: whether something shown changed.
fn looked(decorated: &mut Decorated) -> bool {
    assert!(!decorated.decorations.vcs_changed(), "never read where it's asked");
    let until = std::time::Instant::now() + std::time::Duration::from_millis(400);
    while std::time::Instant::now() < until {
        if decorated.decorations.poll() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[test]
fn a_new_version_of_the_document_is_compared_again() {
    let mut decorated = Decorated::new(repo("h1", "a\nb\n"), "a\nb\n");
    assert!(decorated.decorations.decorations(decorated.doc).unwrap().marks.is_empty());
    decorated.text = document::TextBuffer::new("a\nb\nc\n");
    decorated.version = 2;
    decorated.settle();
    let marks = decorated.decorations.decorations(decorated.doc).unwrap().marks;
    assert_eq!(*marks, vec![LineMark { lines: 2..3, kind: MarkKind::Added }]);
    // A closed document is forgotten.
    decorated.decorations.keep(&[]);
    assert!(decorated.decorations.decorations(decorated.doc).is_none());
}

#[test]
fn the_blame_annotations_are_shown_and_hidden() {
    let mut decorated = Decorated::new(repo("h1", "a\nb\nc\n"), "a\nb\nc\n");
    let doc = decorated.doc;
    assert_eq!(decorated.decorations.annotation_chars(doc), 0);
    assert!(toggle_annotations(&Host { decorated: &decorated, line: 2 }));
    assert_eq!(decorated.decorations.annotation_chars(doc), ANNOTATION_CHARS);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let annotations = loop {
        decorated.settle();
        let annotations = decorated.decorations.decorations(doc).unwrap().annotations.unwrap();
        if !annotations.is_empty() {
            break annotations;
        }
        assert!(std::time::Instant::now() < deadline, "the blame was never read");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    // The lines (and the empty one after the last newline).
    assert!(annotations.len() >= 3);
    let first = annotations[0].as_ref().unwrap();
    assert!(first.first && first.text.ends_with("Ana"), "{first:?}");
    assert!(!annotations[1].as_ref().unwrap().first);
    // The commit on the cursor's line (a click on its annotation), and its tooltip.
    let host = Host { decorated: &decorated, line: 2 };
    assert_eq!(annotated_commit(&host).as_deref(), Some("2222222bbb"));
    let tip = decorated.decorations.annotation_tip(doc, 0).unwrap();
    // Its day in the user's time zone.
    let (y, m, d) = crate::local_time::date(1_700_000_000);
    assert!(tip.starts_with(&format!("1111111 · Ana · {y}-{m:02}-{d:02}\nfirst")), "{tip}");
    // Hidden again.
    assert!(toggle_annotations(&host));
    assert!(decorated.decorations.decorations(doc).unwrap().annotations.is_none());
    assert_eq!(decorated.decorations.annotation_chars(doc), 0);
    assert!(annotated_commit(&host).is_none());
}

#[test]
fn a_new_head_is_announced_and_its_texts_read() {
    let repo = Arc::new(repo("h1", "a\n"));
    // The repository shared with the test (its `HEAD` moves).
    let mut decorated = Decorated::new(Arc::clone(&repo), "a\nb\n");
    let heads = Arc::new(Mutex::new(Vec::new()));
    let _subscription = {
        let heads = Arc::clone(&heads);
        decorated.events.subscribe(move |e: &HeadChanged| {
            heads.lock().unwrap().push((e.head.clone(), e.branch.clone()));
        })
    };
    // Nothing moved (what was still being read taken first: a busy machine finishes it
    // late).
    quiet(&mut decorated);
    assert!(!looked(&mut decorated));
    // A commit: announced, the committed text read again.
    *repo.head.lock().unwrap() = Some("h2".into());
    assert!(looked(&mut decorated));
    assert_eq!(*heads.lock().unwrap(), vec![(Some("h2".into()), Some("main".into()))]);
    decorated.settle();
    let marks = decorated.decorations.decorations(decorated.doc).unwrap().marks;
    assert_eq!(*marks, vec![LineMark { lines: 1..2, kind: MarkKind::Added }]);
    // Another branch checked out: shown.
    *repo.head_branch.lock().unwrap() = Some("feature".into());
    assert!(looked(&mut decorated));
    assert_eq!(decorated.decorations.label().as_deref(), Some("feature"));
}

#[test]
fn outside_a_repository_nothing_is_decorated() {
    let events = EventBus::default();
    let context = DecoratorContext {
        id: ProjectId::new(),
        root: std::path::Path::new("/p"),
        events: &events,
        wake: Arc::new(|| {}),
        repository: None,
    };
    assert!(GitDecorator.open(&context).is_none());
}

#[test]
fn a_click_on_a_change_bar_compares_with_the_last_commit() {
    let decorated = Decorated::new(repo("h1", "a\nb\nc\n"), "a\nB\nc\n");
    let doc = decorated.doc;
    // The bar's first line: the file compared with `HEAD` (when the change actions decline).
    assert_eq!(decorated.decorations.mark_clicked(doc, 1), Some(crate::COMPARE_HEAD));
    // No bar there.
    assert_eq!(decorated.decorations.mark_clicked(doc, 0), None);
}

/// The top bar's label: the branch; the operation in progress with the branch it works on
/// and its step; `HEAD` on a commit said so.
#[test]
fn the_label_says_the_branch_and_what_is_in_progress() {
    use version_control::{Operation, OperationKind};
    assert_eq!(label("main", false, None), "main");
    assert_eq!(label("275df80", true, None), i18n::tr!("git.label_detached", at = "275df80"));
    let rebase = Operation::rebase(Some("refs/heads/feature"), Some("3"), Some("7"));
    let step = i18n::tr!("git.label_rebase_step", step = 3, of = 7);
    assert_eq!(label("275df80", true, Some(&rebase)), format!("feature · {step}"));
    let merge = Operation { kind: OperationKind::Merge, branch: None, step: None };
    assert_eq!(
        label("main", false, Some(&merge)),
        format!("main · {}", i18n::t("git.label_merge"))
    );
}
