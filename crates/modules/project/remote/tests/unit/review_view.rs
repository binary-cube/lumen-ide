use super::*;
use remote::{LocalEntry, RemoteEntry};

fn change(path: &str, action: Action, here: bool, there: bool, conflict: bool) -> Row {
    let local = here.then(|| LocalEntry { path: path.into(), size: 1, modified: 1 });
    let remote =
        there.then(|| RemoteEntry { path: path.into(), is_dir: false, size: 2, modified: 2 });
    let chosen = !conflict && matches!(action, Action::Download | Action::Upload);
    Row {
        change: Change { path: path.into(), action, conflict, chosen, remote, local },
        texts: conflict.then(|| ("server".to_owned(), "mine".to_owned())),
    }
}

fn review(rows: Vec<Row>) -> TransferReview {
    let mut view = TransferReview::default();
    view.data = Some(ReviewData {
        project: kernel::ProjectId::new(),
        root: PathBuf::from("/p"),
        direction: Direction::Sync,
        scope: String::new(),
        fetched_only: false,
    });
    view.started = true;
    view.rows = rows;
    view.phase = Phase::Review;
    view
}

#[test]
fn each_change_says_why_and_removals_and_conflicts_wait() {
    let rows = vec![
        change("new.php", Action::Download, false, true, false),
        change("edit.php", Action::Upload, true, true, false),
        change("gone.php", Action::DeleteLocal, true, false, false),
        change("both.php", Action::Download, true, true, true),
    ];
    let why: Vec<_> = rows.iter().map(|r| reason(&r.change)).collect();
    assert_eq!(why[0], (t("remote.why_new_there"), Tone::Neutral));
    assert_eq!(why[1], (t("remote.why_changed_here"), Tone::Neutral));
    assert_eq!(why[2].1, Tone::Danger, "a removal stands out");
    assert_eq!(why[3], (t("remote.why_conflict"), Tone::Danger));
    let view = review(rows);
    assert_eq!(view.chosen(), 2, "removals and conflicts wait for the user");
    assert!(view.summary().contains(&tr!("remote.review_conflicts", count = 1)));
}

#[test]
fn a_conflict_is_decided_compared_and_checked_from_the_keys() {
    let mut view = review(vec![
        change("a.php", Action::Upload, true, true, false),
        change("both.php", Action::Download, true, true, true),
    ]);
    // Space on a conflict: the server's copy, checked.
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Down);
    view.text_input(" ");
    assert_eq!((view.rows[1].change.action, view.rows[1].change.chosen), (Action::Download, true));
    // → ours instead.
    view.edit_key(ViewerKey::Right);
    assert_eq!(view.rows[1].change.action, Action::Upload);
    // Space again: left out.
    view.text_input(" ");
    assert!(!view.rows[1].change.chosen);
    // Compared beside: the server's copy read-only on the left, ours on the right.
    match view.compare(1) {
        Some(ViewerAction::CompareBeside(input, _)) => {
            assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("server", "mine"));
            assert_eq!(input.read_only, [true, false]);
            assert_eq!(input.right.0, PathBuf::from("/p/both.php"));
        }
        other => panic!("expected a comparison, got {other:?}"),
    }
    assert!(view.compare(0).is_none(), "not a conflict");
    // Esc closes.
    view.edit_key(ViewerKey::Escape);
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
}

#[test]
fn every_change_or_every_removal_is_checked_at_once() {
    let mut view = review(vec![
        change("new.php", Action::Download, false, true, false),
        change("gone.php", Action::DeleteLocal, true, false, false),
        change("old.php", Action::DeleteRemote, false, true, false),
        change("both.php", Action::Download, true, true, true),
    ]);
    // The removals only: both checked, the rest as it was.
    assert_eq!(view.checked_of(is_removal), (0, 2));
    view.check_all(is_removal);
    assert_eq!(view.checked_of(is_removal), (2, 2));
    assert_eq!(view.chosen(), 3, "the download stays checked, the conflict undecided");
    // Again: unchecked.
    view.check_all(is_removal);
    assert_eq!(view.chosen(), 1);
    // Everything: partly checked → all; once all are → none. A conflict is never decided.
    view.check_all(|_| true);
    assert_eq!((view.checked_of(|_| true), view.chosen()), ((3, 3), 3));
    assert!(!view.rows[3].change.chosen, "a conflict waits for its side");
    view.check_all(|_| true);
    assert_eq!(view.chosen(), 0);
    // A decided conflict stays as decided.
    view.toggle(3);
    view.check_all(|_| true);
    view.check_all(|_| true);
    assert!(view.rows[3].change.chosen);
    // Clicked: the hit does the same.
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::CheckRemovals)];
    view.click(Point::new(5.0, 5.0));
    assert_eq!(view.checked_of(is_removal), (2, 2));
}

/// Against the demo servers: `EDITOR_REMOTE_DEMO=1`, run with `HOME` in a scratch folder (the
/// servers' keys are learned there). Pulls the project, then compares only `public/` after a
/// change on each side and one on both, decides the conflict and applies; the server's files
/// are put back as they were.
#[test]
#[ignore = "needs the demo servers"]
fn a_folder_is_compared_and_synced_on_real_servers() {
    use remote::{Auth, Connection, Mode, Protocol, SavedIn, SecretsIn};
    if std::env::var("EDITOR_REMOTE_DEMO").is_err() {
        return;
    }
    for (protocol, port, root) in [
        (Protocol::Sftp, 2222, "/www/shop"),
        (Protocol::Ftps { implicit: false }, 2121, "/home/demo/www/shop"),
    ] {
        let local = std::env::temp_dir().join(format!(
            "remote-review-{}-{}",
            protocol.id(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&local);
        let saved = crate::config::Saved {
            connection: Connection {
                id: "demo".into(),
                name: "Demo".into(),
                protocol,
                host: "127.0.0.1".into(),
                port,
                user: "demo".into(),
                auth: Auth::Password,
                root: root.into(),
                mode: Mode::Mirror,
                excluded: vec!["vendor".into()],
                saved_in: SavedIn::Project,
                secrets_in: SecretsIn::Keychain,
                retry: None,
                uploads: None,
                downloads: None,
                moves: false,
                auto_upload: true,
                write_as: Default::default(),
            },
            local: local.clone(),
            trusted: Vec::new(),
        };
        let probe = session::connector(&saved, Some("demo123".into()), None);
        let accepted = match probe.connect() {
            Ok(_) => None,
            Err(RemoteError::HostKey(f)) => Some(f),
            Err(e) => panic!("{protocol:?}: {e}"),
        };
        let connector = session::connector(&saved, Some("demo123".into()), accepted);
        let state = State::detached(saved, Arc::clone(&connector));
        let never = AtomicBool::new(false);
        let run = |changes: &[Change]| {
            let copy = session::local_copy(&local);
            let mut manifest = state.manifest.lock().unwrap().clone();
            let run = Run {
                workers: crate::settings::defaults().limits.downloads,
                retry: state.retry(),
                now: &session::now,
                on_transfer: &|t| state.record(t),
                on_bytes: &|_, _| {},
                cancelled: &|| false,
                parallel: &crate::session::threads,
                pause: &std::thread::sleep,
            };
            let done = apply(changes, state.connector.as_ref(), &copy, &mut manifest, &run);
            assert!(done.iter().all(|t| t.error.is_none()), "{done:?}");
            *state.manifest.lock().unwrap() = manifest;
        };
        // The first pull.
        let rows = compare_with_server(&state, Direction::Pull, "", false, &never).unwrap();
        assert!(rows.iter().all(|r| r.change.action == Action::Download && r.change.chosen));
        run(&rows.iter().map(|r| r.change.clone()).collect::<Vec<_>>());
        let left = compare_with_server(&state, Direction::Sync, "", false, &never).unwrap();
        let left: Vec<(String, Action)> =
            left.iter().map(|r| (r.change.path.clone(), r.change.action)).collect();
        assert!(left.is_empty(), "{protocol:?}: still to sync after a pull: {left:?}");

        // Changed there (style.css), here (a new file), on both sides (index.php); and
        // outside the scope (composer.json).
        let mut fs = connector.connect().unwrap();
        let originals: Vec<(String, Vec<u8>)> =
            ["public/style.css", "public/index.php", "composer.json"]
                .into_iter()
                .map(|p| (p.to_owned(), fs.read(p).unwrap()))
                .collect();
        // The demo as it was, whatever happens below.
        let _restore = Restore {
            connector: Arc::clone(&connector),
            files: originals,
            created: vec!["public/new.php".into()],
        };
        // Different sizes, so the change shows whatever the server's clock does.
        fs.write("public/style.css", b"body { color: red; } /* changed on the server */").unwrap();
        fs.write("public/index.php", b"<?php // changed on the server, longer").unwrap();
        fs.write("composer.json", b"{\"changed\": \"on the server\"}").unwrap();
        std::fs::write(local.join("public/new.php"), "<?php // new here").unwrap();
        std::fs::write(local.join("public/index.php"), "<?php // mine").unwrap();

        let mut rows =
            compare_with_server(&state, Direction::Sync, "public", false, &never).unwrap();
        let find = |rows: &[Row], p: &str| rows.iter().position(|r| r.change.path == p).unwrap();
        assert_eq!(rows.len(), 3, "{protocol:?}: only public/: {rows:?}");
        let style = &rows[find(&rows, "public/style.css")];
        assert_eq!((style.change.action, style.change.conflict), (Action::Download, false));
        assert_eq!(rows[find(&rows, "public/new.php")].change.action, Action::Upload);
        let both = find(&rows, "public/index.php");
        assert!(rows[both].change.conflict && !rows[both].change.chosen);
        let (server, mine) = rows[both].texts.clone().expect("both copies read to compare");
        assert!(server.contains("changed on the server") && mine.contains("mine"));
        // Ours wins.
        rows[both].change.take(false);
        run(&rows.iter().map(|r| r.change.clone()).collect::<Vec<_>>());
        assert!(std::fs::read_to_string(local.join("public/style.css")).unwrap().contains("red"));
        assert_eq!(fs.read("public/index.php").unwrap(), b"<?php // mine");
        assert_eq!(fs.read("public/new.php").unwrap(), b"<?php // new here");
        assert!(state.log.lock().unwrap().len() >= 3, "each transfer logged");
        // The file alone: up to date.
        assert!(
            compare_with_server(&state, Direction::Sync, "public/index.php", false, &never)
                .unwrap()
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(&local);
    }
}

/// Puts the server's files back as they were when dropped (even when a test fails).
struct Restore {
    connector: Arc<dyn remote::application::Connector>,
    files: Vec<(String, Vec<u8>)>,
    created: Vec<String>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        let Ok(mut fs) = self.connector.connect() else { return };
        for (path, bytes) in &self.files {
            let _ = fs.write(path, bytes);
        }
        for path in &self.created {
            let _ = fs.remove(path);
        }
    }
}

/// Against the demo servers: on demand, Pull / Sync keep to the files brought (the others
/// stay on the server); "Bring here" takes everything under the folder.
#[test]
#[ignore = "needs the demo servers"]
fn on_demand_reviews_keep_to_what_was_brought() {
    use remote::{Auth, Connection, Mode, Protocol, SavedIn, SecretsIn};
    if std::env::var("EDITOR_REMOTE_DEMO").is_err() {
        return;
    }
    let local = std::env::temp_dir().join(format!("remote-brought-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&local);
    std::fs::create_dir_all(&local).unwrap();
    let saved = crate::config::Saved {
        connection: Connection {
            id: "demo".into(),
            name: "Demo".into(),
            protocol: Protocol::Sftp,
            host: "127.0.0.1".into(),
            port: 2222,
            user: "demo".into(),
            auth: Auth::Password,
            root: "/www/shop".into(),
            mode: Mode::OnDemand,
            excluded: vec!["vendor".into()],
            saved_in: SavedIn::Project,
            secrets_in: SecretsIn::Keychain,
            retry: None,
            uploads: None,
            downloads: None,
            moves: false,
            auto_upload: true,
            write_as: Default::default(),
        },
        local: local.clone(),
        trusted: Vec::new(),
    };
    let accepted = match session::connector(&saved, Some("demo123".into()), None).connect() {
        Ok(_) => None,
        Err(RemoteError::HostKey(f)) => Some(f),
        Err(e) => panic!("{e}"),
    };
    let state = State::detached(
        saved.clone(),
        session::connector(&saved, Some("demo123".into()), accepted),
    );
    let never = AtomicBool::new(false);
    // One file brought (as when opened).
    let rows =
        compare_with_server(&state, Direction::Pull, "app/Models/User.php", false, &never).unwrap();
    let copy = session::local_copy(&local);
    let mut manifest = state.manifest.lock().unwrap().clone();
    let run = Run {
        workers: 1,
        retry: state.retry(),
        now: &session::now,
        on_transfer: &|_| {},
        on_bytes: &|_, _| {},
        cancelled: &|| false,
        parallel: &crate::session::threads,
        pause: &std::thread::sleep,
    };
    apply(
        &rows.iter().map(|r| r.change.clone()).collect::<Vec<_>>(),
        state.connector.as_ref(),
        &copy,
        &mut manifest,
        &run,
    );
    *state.manifest.lock().unwrap() = manifest;
    // Pull / Sync on the project: up to date (the server's other files aren't changes).
    assert!(compare_with_server(&state, Direction::Pull, "", true, &never).unwrap().is_empty());
    assert!(compare_with_server(&state, Direction::Sync, "", true, &never).unwrap().is_empty());
    // Bring "app" here: its other files come.
    let bring = compare_with_server(&state, Direction::Pull, "app", false, &never).unwrap();
    let paths: Vec<_> = bring.iter().map(|r| r.change.path.as_str()).collect();
    assert!(
        paths.contains(&"app/Models/Model.php") && paths.contains(&"app/Http/HomeController.php"),
        "{paths:?}"
    );
    assert!(!paths.contains(&"app/Models/User.php"), "already here");
    let _ = std::fs::remove_dir_all(&local);
}

#[test]
fn while_transferring_the_list_is_locked_and_esc_stops() {
    let mut view = review(vec![
        change("a.php", Action::Upload, true, true, false),
        change("b.php", Action::Download, false, true, false),
    ]);
    view.phase = Phase::Applying;
    view.hits = vec![(Rect::new(0.0, 0.0, 100.0, 30.0), Hit::Check(0))];
    view.click(Point::new(10.0, 10.0));
    view.text_input(" ");
    view.edit_key(ViewerKey::Down);
    assert!(view.rows.iter().all(|r| r.change.chosen), "nothing unchecked meanwhile");
    assert!(!view.wants_text());
    assert!(!view.cancel.load(Ordering::Relaxed));
    view.edit_key(ViewerKey::Escape);
    assert!(view.cancel.load(Ordering::Relaxed), "Esc stops it");
    assert!(view.take_action().is_none(), "the review stays open, to say how it went");
}

/// The list's scrollbar: its thumb pressed and dragged down scrolls the list.
#[test]
fn the_list_scrolls_by_its_thumb() {
    let mut view = review(Vec::new());
    let area = Rect::new(0.0, 0.0, 400.0, 200.0);
    let palette = ui_components::Palette::dark();
    view.max_scroll = 800.0;
    view.bar.paint(&mut render_api::Scene::default(), &palette, 1.0, area, 1000.0, 0.0);
    let edge = area.max_x() - 4.0;
    assert!(view.click(Point::new(edge, 6.0)).is_none());
    assert!(view.drag(Point::new(edge, 80.0)), "the thumb followed");
    assert!(view.scroll > 0.0, "the list scrolled");
    assert!(view.release());
}
