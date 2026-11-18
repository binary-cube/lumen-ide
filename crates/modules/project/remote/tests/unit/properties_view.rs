use super::*;
use remote::application::{Connector, RemoteFs};
use remote::properties::Mode;
use remote::{RemoteEntry, RemoteError};

/// A server whose files say their permissions and owners.
struct Server;

impl RemoteFs for Server {
    fn list(&mut self, _: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        Ok(Vec::new())
    }
    fn read(&mut self, _: &str) -> Result<Vec<u8>, RemoteError> {
        Err(RemoteError::NotFound(String::new()))
    }
    fn write(&mut self, path: &str, _: &[u8]) -> Result<RemoteEntry, RemoteError> {
        Err(RemoteError::NotFound(path.into()))
    }
    fn remove(&mut self, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn rename(&mut self, _: &str, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn stat(&mut self, _: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        Ok(None)
    }
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        Ok(paths
            .iter()
            .map(|p| {
                let (mode, owner) = match p.as_str() {
                    "a.php" => (0o644, 33),
                    "b.php" => (0o664, 33),
                    "app" => (0o755, 1000),
                    _ => return None,
                };
                Some(Properties {
                    path: p.clone(),
                    is_dir: p == "app",
                    size: 10,
                    modified: 1_700_000_000,
                    mode: Some(Mode::new(mode)),
                    owner: Some(Principal::id(owner)),
                    group: Some(Principal::named("www-data", Some(33))),
                })
            })
            .collect())
    }
    fn accounts(&mut self) -> Result<Accounts, RemoteError> {
        Ok(Accounts {
            users: vec![
                Principal::named("www-data", Some(33)),
                Principal::named("dev", Some(1000)),
            ],
            groups: vec![Principal::named("www-data", Some(33))],
        })
    }
    fn capabilities(&mut self) -> Capabilities {
        Capabilities { chmod: true, chown: false, chgrp: true, server_hashes: Vec::new() }
    }
}

struct Connect;

impl Connector for Connect {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        Ok(Box::new(Server))
    }
}

fn view_of(paths: &[&str]) -> PropertiesView {
    let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    let mut view = PropertiesView::default();
    view.take_loaded(load(&Connect, &paths).unwrap());
    view
}

/// Only what's on the server is shown; the accounts get the owners' names, sorted.
#[test]
fn what_is_read_keeps_what_the_server_has_and_names_the_owners() {
    let loaded = load(&Connect, &["a.php".into(), "local-only.php".into()]).unwrap();
    assert_eq!(loaded.props.len(), 1);
    let names: Vec<String> = loaded.accounts.users.iter().map(ToString::to_string).collect();
    assert_eq!(names, ["dev [1000]", "www-data [33]"]);
    assert!(!loaded.caps.chown && loaded.caps.chgrp);
}

/// One file: its octal; the boxes and the octal follow each other.
#[test]
fn the_boxes_and_the_octal_follow_each_other() {
    let mut view = view_of(&["a.php"]);
    assert_eq!(view.octal.text(), "0644");
    view.toggle(Bit::Access(Class::Owner, Access::Execute));
    assert_eq!(view.octal.text(), "0744");
    view.typing = true;
    view.octal = TextInput::new("");
    assert!(view.text_input("7x5a5"), "only octal digits are taken");
    assert_eq!(view.octal.text(), "755");
    assert!(!view.octal_invalid());
    assert_eq!(view.edit().and_then(ModeEdit::octal).as_deref(), Some("0755"));
    // Not a mode (yet): the boxes stay, the field is marked.
    view.octal = TextInput::new("75");
    view.octal_typed();
    assert!(view.octal_invalid());
    assert_eq!(view.edit().and_then(ModeEdit::octal).as_deref(), Some("0755"));
}

/// Several: the bits they differ in are mixed (no octal), the owner shared or mixed.
#[test]
fn several_show_what_they_share_and_mix_the_rest() {
    let view = view_of(&["a.php", "b.php"]);
    let edit = view.edit().unwrap();
    assert_eq!(edit.state(Bit::Access(Class::Group, Access::Write)), Tri::Mixed);
    assert_eq!(view.octal.text(), "");
    assert_eq!(view.label_of(Choice::Owner), "www-data [33]");
    let mixed = view_of(&["a.php", "app"]);
    assert_eq!(mixed.label_of(Choice::Owner), t("remote.props_mixed"));
    assert_eq!(mixed.label_of(Choice::Group), "www-data [33]");
}

/// What the server lets change: the owner not (no chown here), the group yes.
#[test]
fn a_list_is_offered_only_when_the_server_lets_it_change() {
    let mut view = view_of(&["a.php"]);
    assert!(!view.may_change(Choice::Owner));
    assert!(view.may_change(Choice::Group));
    let (items, chosen) = view.options(Choice::Group);
    assert_eq!(items, [PickerItem::person("www-data", "").with_cells(vec!["33".into()])]);
    assert_eq!(chosen, Some(0));
    // A list opened: the keys go to it, ↵ chooses.
    view.open_list(Choice::Group);
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(!view.picker.is_open());
    assert_eq!(view.label_of(Choice::Group), "www-data [33]");
    view.choose(Choice::Owner, 1);
    assert_eq!(view.label_of(Choice::Owner), "www-data [33]");
}

/// The size: a file's at once; a folder's unknown until measured.
#[test]
fn a_folder_size_waits_to_be_calculated() {
    assert_eq!(view_of(&["a.php"]).size_text(), i18n::bytes(10));
    assert_eq!(view_of(&["app"]).size_text(), t("remote.props_size_unknown"));
    let mut view = view_of(&["a.php", "app"]);
    view.measured = Some(Ok(Measured { bytes: 90, files: 4, folders: 2 }));
    assert!(view.size_text().starts_with(&i18n::bytes(100)));
}

/// Not a remote project: said, nothing read.
#[test]
fn without_a_session_it_says_so() {
    let mut view = PropertiesView::default();
    view.set_data(&PropertiesData {
        project: kernel::ProjectId::new(),
        paths: vec!["a.php".into()],
        queries: None,
    });
    assert!(view.failed.is_some());
    assert!(view.loading.is_none());
    // Esc closes it.
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
}

/// OK is there only once something changed; recursively, the permissions shown go down
/// the folders even unchanged.
#[test]
fn ok_applies_only_what_changed() {
    let mut view = view_of(&["a.php"]);
    assert_eq!(view.change(), None);
    view.choose(Choice::Group, 0);
    assert_eq!(view.change(), None, "the group it already has");
    view.toggle(Bit::Access(Class::Group, Access::Write));
    let change = view.change().unwrap();
    assert_eq!(change.mode.and_then(|m| m.octal()).as_deref(), Some("0664"));
    assert!(!change.recursive);
    // A typed octal that isn't one: nothing to apply.
    view.octal = TextInput::new("78");
    assert_eq!(view.change(), None);
    let mut folder = view_of(&["app"]);
    folder.recursive = true;
    let change = folder.change().unwrap();
    assert!(change.recursive && change.mode.is_some() && !change.x_on_folders);
}

/// Recursively: counted first (a measure known already is used), then asked; Back returns.
#[test]
fn a_recursive_change_is_counted_and_asked_first() {
    let mut view = view_of(&["app"]);
    view.recursive = true;
    view.measured = Some(Ok(Measured { bytes: 0, files: 10, folders: 2 }));
    view.ok();
    assert!(matches!(view.phase, Phase::Confirm(13)));
    // Meanwhile the boxes don't change.
    let before = view.edit().copied();
    view.hits =
        vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Bit(Bit::Access(Class::Owner, Access::Read)))];
    view.press(Point::new(5.0, 5.0), 1);
    assert_eq!(view.edit().copied(), before);
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Back)];
    view.press(Point::new(5.0, 5.0), 1);
    assert!(matches!(view.phase, Phase::Editing));
}

/// What applying did: all done closes the dialog and says so; refusals are listed.
#[test]
fn the_outcome_closes_or_lists_the_refusals() {
    let mut view = view_of(&["a.php"]);
    view.applied(Ok(Applied { changed: 1, seen: 1, failed: Vec::new(), stopped: false }));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    assert_eq!(view.take_notifications().len(), 1);
    let refused = vec![("a.php".to_owned(), "operation not permitted".to_owned())];
    view.applied(Ok(Applied { changed: 0, seen: 1, failed: refused.clone(), stopped: false }));
    assert!(matches!(&view.phase, Phase::Failed(f) if *f == refused));
    assert_eq!(view.take_action(), None);
}
