//! The file's format chosen from the status bar's file window: each row opens its list,
//! the current value checked; a terminator or encoding chosen is saved as such, a file read
//! again as another encoding, a language and an indentation the tab's.

use super::*;

/// A project in a fresh scratch folder with `files` written, laid out; the folder goes at
/// the end (`Drop`).
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(files: &[(&str, &[u8])]) -> (Workbench, PathBuf, Scratch) {
    let dir = std::env::temp_dir().join(format!("file-format-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    for (file, bytes) in files {
        std::fs::write(dir.join(file), bytes).unwrap();
    }
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    (wb, dir.clone(), Scratch(dir))
}

/// What the open list's rows do.
fn picks(wb: &Workbench) -> Vec<FormatPick> {
    match &wb.choice_purpose {
        ChoicePurpose::Format(_, picks) => picks.clone(),
        other => panic!("not a format list: {other:?}"),
    }
}

#[test]
fn a_line_separator_chosen_is_saved_so() {
    let (mut wb, dir, _scratch) = project(&[("a.txt", b"a\r\nb\r\n")]);
    wb.open_file(&dir.join("a.txt")).unwrap();
    assert!(wb.choose_line_ending());
    assert_eq!(
        picks(&wb),
        vec![FormatPick::LineEnding(LineEnding::Lf), FormatPick::LineEnding(LineEnding::CrLf)]
    );
    let _ = wb.choice_picked(0);
    let i = wb.active;
    assert!(wb.tabs[i].doc.is_dirty(), "a change to save");
    wb.save_tab(i, None).unwrap();
    assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"a\nb\n");
    assert!(!wb.tabs[i].doc.is_dirty());
}

#[test]
fn an_encoding_converts_or_reads_the_file_again() {
    let (mut wb, dir, _scratch) = project(&[("u.txt", "câ\n".as_bytes())]);
    wb.open_file(&dir.join("u.txt")).unwrap();
    let i = wb.active;
    assert!(wb.choose_encoding());
    let offered = picks(&wb);
    assert!(offered.contains(&FormatPick::Convert(Encoding::Utf16Le)));
    assert!(offered.contains(&FormatPick::ReadAs(Encoding::Latin1)));
    assert!(!offered.contains(&FormatPick::ReadAs(Encoding::Utf8)), "not as it is already");
    // Read again as Latin-1: each byte a character, as the file is (nothing to save).
    wb.choice_menu.close();
    wb.format_picked(wb.tabs[i].doc.id(), FormatPick::ReadAs(Encoding::Latin1));
    assert_eq!(wb.tabs[i].doc.buffer().to_string(), "cÃ¢\n");
    assert_eq!(wb.tabs[i].doc.encoding(), Encoding::Latin1);
    assert!(!wb.tabs[i].doc.is_dirty());
    // Back as UTF-8, then written as UTF-16 LE.
    wb.format_picked(wb.tabs[i].doc.id(), FormatPick::ReadAs(Encoding::Utf8));
    assert_eq!(wb.tabs[i].doc.buffer().to_string(), "câ\n");
    wb.format_picked(wb.tabs[i].doc.id(), FormatPick::Convert(Encoding::Utf16Le));
    assert!(wb.tabs[i].doc.is_dirty());
    wb.save_tab(i, None).unwrap();
    let bytes = std::fs::read(dir.join("u.txt")).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xFE], "with its mark");
    // With unsaved changes, reading again isn't offered (they'd be lost).
    let mut text = TextSystem::new();
    wb.type_text(&mut text, "x");
    assert!(wb.choose_encoding());
    assert!(!picks(&wb).iter().any(|p| matches!(p, FormatPick::ReadAs(_))));
}

/// Latin-1 can't write `ș`: the encoding stays, the user is told.
#[test]
fn an_encoding_that_cant_write_the_text_is_refused() {
    let (mut wb, dir, _scratch) = project(&[("s.txt", "ș\n".as_bytes())]);
    wb.open_file(&dir.join("s.txt")).unwrap();
    let i = wb.active;
    wb.format_picked(wb.tabs[i].doc.id(), FormatPick::Convert(Encoding::Latin1));
    assert_eq!(wb.tabs[i].doc.encoding(), Encoding::Utf8);
    assert!(!wb.tabs[i].doc.is_dirty());
    assert_eq!(wb.data.notifications.shown().count(), 1);
}

#[test]
fn a_language_and_an_indentation_are_the_tab_s() {
    let (mut wb, dir, _scratch) = project(&[("c.txt", b"{\"a\": 1}\n")]);
    wb.open_file(&dir.join("c.txt")).unwrap();
    let i = wb.active;
    assert!(wb.choose_language());
    let json = LanguageId::new("json");
    assert!(picks(&wb).contains(&FormatPick::Language(json.clone())));
    wb.format_picked(wb.tabs[i].doc.id(), FormatPick::Language(json.clone()));
    assert_eq!(wb.tabs[i].syntax.language(), &json);
    assert!(wb.choose_indent());
    assert_eq!(picks(&wb).len(), 4);
    wb.format_picked(wb.tabs[i].doc.id(), FormatPick::Indent(Indent::Tabs));
    assert_eq!(wb.tabs[i].doc.indent(), Indent::Tabs);
    assert!(!wb.tabs[i].doc.is_dirty(), "nothing to save");
    // The file window's rows run them.
    let rows = wb.file_detail_rows();
    let actions: Vec<_> = rows
        .into_iter()
        .filter_map(|r| match r {
            ui_workbench::status_popover::PopoverRow::Value { action, .. } => action,
            _ => None,
        })
        .collect();
    for command in [
        crate::commands::CHOOSE_LANGUAGE,
        crate::commands::CHOOSE_ENCODING,
        crate::commands::CHOOSE_LINE_ENDING,
        crate::commands::CHOOSE_INDENT,
    ] {
        assert!(actions.contains(&command.0), "{command:?}");
    }
}

/// The file split in two groups: a terminator chosen and saved in one leaves no copy
/// unsaved (a save changes no version: the copies are saved with it).
#[test]
fn a_format_saved_leaves_no_copy_unsaved() {
    let (mut wb, dir, _scratch) = project(&[("a.txt", b"a\nb\n")]);
    wb.open_file(&dir.join("a.txt")).unwrap();
    let i = wb.active;
    assert!(wb.split_tab(i, crate::split_tree::Split::Right, false));
    let i = wb.active;
    wb.format_picked(wb.tabs[i].doc.id(), FormatPick::LineEnding(LineEnding::CrLf));
    let copies = |wb: &mut Workbench| {
        all_tabs_mut(&mut wb.tabs, &mut wb.editors.groups)
            .filter(|t| t.doc.path().is_some())
            .map(|t| t.doc.is_dirty())
            .collect::<Vec<_>>()
    };
    assert_eq!(copies(&mut wb), [true, true], "both copies changed");
    wb.save_tab(i, None).unwrap();
    assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"a\r\nb\r\n");
    assert_eq!(copies(&mut wb), [false, false], "and both saved");
}

/// The list belongs to the file it was opened for: chosen after another tab became the
/// active one, it changes that file, not the active one.
#[test]
fn a_pick_goes_to_the_file_its_list_was_opened_for() {
    let (mut wb, dir, _scratch) = project(&[("a.txt", b"a\n"), ("b.txt", b"b\n")]);
    wb.open_file(&dir.join("a.txt")).unwrap();
    let a = wb.tabs[wb.active].doc.id();
    assert!(wb.choose_line_ending());
    assert!(matches!(wb.choice_purpose, ChoicePurpose::Format(doc, _) if doc == a));
    wb.choice_menu.close();
    wb.open_file(&dir.join("b.txt")).unwrap();
    let b = wb.active;
    wb.format_picked(a, FormatPick::LineEnding(LineEnding::CrLf));
    let tab_a = wb.tabs.iter().find(|t| t.doc.id() == a).unwrap();
    assert_eq!(tab_a.doc.line_ending(), LineEnding::CrLf);
    assert_eq!(wb.tabs[b].doc.line_ending(), LineEnding::Lf, "the active one untouched");
}
