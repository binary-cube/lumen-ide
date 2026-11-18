use std::time::{Duration, Instant};

use super::*;
use extractors::Extractor;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "extractor-templates-{name}-{}-{:x}",
        std::process::id(),
        kernel::id::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn until(done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn a_folder_s_templates_are_read_sorted_with_their_errors() {
    let dir = scratch("read");
    std::fs::write(dir.join("b.extractor"), "{{#rows}}{{row.index}}{{/rows}}").unwrap();
    std::fs::write(dir.join("a.extractor"), "---\nname: Mine\n---\n{{#rows}}").unwrap();
    std::fs::write(dir.join("notes.txt"), "{{x}}").unwrap();
    std::fs::create_dir_all(dir.join("sub.extractor")).unwrap();
    let read = read_dir(&dir, Origin::Project);
    let ids: Vec<&str> = read.iter().map(|t| t.id()).collect();
    assert_eq!(ids, ["template:project:a.extractor", "template:project:b.extractor"]);
    assert!(read[0].problem().is_some(), "an unclosed section");
    assert_eq!(read[0].name(), "a", "its header is wrong: its file's name");
    assert!(read[1].problem().is_none());
    assert_eq!(read_dir(&dir.join("none"), Origin::User).len(), 0);
    assert_eq!(read_dir(&dir, Origin::User)[0].id(), "template:user:a.extractor");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_huge_or_binary_file_is_an_error_not_a_crash() {
    let dir = scratch("huge");
    std::fs::write(dir.join("big.extractor"), "x".repeat(extractors::template::MAX_LEN + 10))
        .unwrap();
    std::fs::write(dir.join("bin.extractor"), [0xff, 0xfe, 0x00]).unwrap();
    let read = read_dir(&dir, Origin::Project);
    assert!(read.iter().all(|t| t.problem().is_some()), "{read:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_project_s_templates_and_the_shared_ones_reach_the_catalog_and_errors_the_editor() {
    let root = scratch("project");
    let user = scratch("user");
    let own = own_dir(&root);
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(own.join("ok.extractor"), "---\nname: Ok\n---\n{{#rows}}x{{/rows}}").unwrap();
    std::fs::write(own.join("bad.extractor"), "line\n {{nope}}").unwrap();
    std::fs::write(user.join("shared.extractor"), "{{table}}").unwrap();
    let events = kernel::EventBus::new();
    let heard: Arc<Mutex<Vec<DiagnosticsFound>>> = Arc::default();
    let into = Arc::clone(&heard);
    let _s = events.subscribe(move |e: &DiagnosticsFound| into.lock().unwrap().push(e.clone()));
    let project = ProjectId::new();
    let templates = Templates::new(project, &root, Some(user.clone()), events);
    templates.read_now();
    let ids: Vec<String> =
        extractors::catalog().scripted(project).iter().map(|e| e.id().to_owned()).collect();
    assert_eq!(
        ids,
        [
            "template:project:bad.extractor",
            "template:project:ok.extractor",
            "template:user:shared.extractor"
        ]
    );
    let found = heard.lock().unwrap().clone();
    assert_eq!(found.len(), 1, "only the file with an error: {found:?}");
    let d = &found[0].diagnostics[0];
    assert_eq!(found[0].source, SOURCE);
    assert_eq!(
        d.path,
        std::path::Path::new(kernel::brand::PROJECT_DIR).join(DIR).join("bad.extractor")
    );
    assert_eq!((d.line, d.column), (1, Some(3)));
    // Fixed: its errors cleared; then deleted after a new error: cleared too.
    std::fs::write(own.join("bad.extractor"), "fine").unwrap();
    templates.read_now();
    let last = heard.lock().unwrap().last().cloned().unwrap();
    assert!(last.diagnostics.is_empty());
    std::fs::write(own.join("bad.extractor"), "{{#rows}}").unwrap();
    templates.read_now();
    std::fs::remove_file(own.join("bad.extractor")).unwrap();
    templates.read_now();
    let last = heard.lock().unwrap().last().cloned().unwrap();
    assert!(last.diagnostics.is_empty(), "{last:?}");
    // Another project doesn't see them.
    assert!(extractors::catalog().scripted(ProjectId::new()).is_empty());
    extractors::catalog().forget(project);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(user);
}

#[test]
fn a_change_in_the_folder_reads_them_again() {
    let root = scratch("follow");
    let own = own_dir(&root);
    std::fs::create_dir_all(&own).unwrap();
    let events = kernel::EventBus::new();
    let project = ProjectId::new();
    let templates = Templates::new(project, &root, None, events.clone());
    let _subscriptions = templates.follow();
    templates.reload();
    assert!(until(|| !templates.busy()));
    assert!(extractors::catalog().scripted(project).is_empty());
    let path = own.join("new.extractor");
    std::fs::write(&path, "x").unwrap();
    // Something elsewhere in the project: nothing read.
    events.publish(&workspace::events::FilesWritten { paths: vec![root.join("src/a.php")] });
    assert!(until(|| !templates.busy()));
    assert!(extractors::catalog().scripted(project).is_empty());
    events.publish(&workspace::events::EntriesChanged { paths: vec![path.clone()] });
    assert!(until(|| extractors::catalog().scripted(project).len() == 1));
    std::fs::remove_file(&path).unwrap();
    events.publish(&workspace::events::EntriesDeleted { paths: vec![path] });
    assert!(until(|| extractors::catalog().scripted(project).is_empty()));
    extractors::catalog().forget(project);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_folders_are_where_they_should_be() {
    let root = std::path::Path::new("/p");
    assert_eq!(own_dir(root), root.join(kernel::brand::PROJECT_DIR).join("extractors"));
    let t = Templates::new(ProjectId::new(), root, None, kernel::EventBus::new());
    assert!(t.concerns(&own_dir(root).join("a.extractor")));
    assert!(t.concerns(&root.join(kernel::brand::PROJECT_DIR)));
    assert!(!t.concerns(&root.join("src")));
}

#[test]
fn the_template_messages_read_as_written() {
    // Read from the catalogs themselves (nothing registered: the other tests' texts stay).
    for (lang, locale) in [("en", i18n::Locale::En), ("ro", i18n::Locale::Ro)] {
        let path =
            format!("{}/../../../domain/core/i18n/locales/{lang}.toml", env!("CARGO_MANIFEST_DIR"));
        let catalog = i18n::parse_catalog(&std::fs::read_to_string(path).unwrap());
        let format = |key: &str, args: &[(&str, i18n::Value)]| {
            let (_, text) = catalog.iter().find(|(k, _)| k == key).unwrap();
            i18n::Message::parse(text).format(locale, args)
        };
        let m = format(
            "extract.template_close_other",
            &[
                ("name", "rows".into()),
                ("open", "columns".into()),
                ("line", 2.into()),
                ("column", 5.into()),
            ],
        );
        assert!(m.contains("{{/rows}}") && m.contains("{{#columns}}") && m.contains('2'), "{m}");
        let m = format("extract.template_needs_columns", &[]);
        assert!(m.contains("{{#columns}}…{{/columns}}"), "{m}");
        assert!(format("extract.template_unclosed_tag", &[]).contains("}}"));
        assert!(
            format("extract.template_unclosed_section", &[("name", "rows".into())])
                .contains("{{#rows}}")
        );
    }
}
