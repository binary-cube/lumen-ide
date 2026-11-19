use super::*;

/// A folder of its own (removed at the end).
fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agent-claude-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn session(id: &str, cwd: &str, title: &str) -> String {
    format!(
        "{}\n{}\n",
        serde_json::json!({"type": "user", "sessionId": id, "cwd": cwd,
            "timestamp": "2026-11-12T08:00:00Z", "message": {"content": "hi"}}),
        serde_json::json!({"type": "ai-title", "aiTitle": title}),
    )
}

/// A project's sessions: the newest first, at most as many as asked, only its own (not
/// another path named the same way), files only; read again only when one changes.
#[test]
fn a_project_s_sessions_are_listed_newest_first() {
    let projects = scratch();
    let root = Path::new("/work/api.shop");
    let folder = projects.join(format::folder_name("/work/api.shop"));
    std::fs::create_dir_all(folder.join("a-folder.jsonl")).unwrap();
    std::fs::write(folder.join("old.jsonl"), session("old", "/work/api.shop", "Old one")).unwrap();
    std::fs::write(folder.join("other.jsonl"), session("other", "/work/api-shop", "Elsewhere"))
        .unwrap();
    std::fs::write(folder.join("notes.txt"), "not a session").unwrap();
    // A second later: newer.
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    std::fs::write(folder.join("new.jsonl"), session("new", "/work/api.shop", "New one")).unwrap();
    let sessions = Sessions::default();
    let found = sessions.of(&projects, root, 10);
    let titles: Vec<&str> = found.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["New one", "Old one"]);
    assert_eq!(found[0].id, "new");
    assert_eq!(found[0].agent, crate::ID);
    assert_eq!(found[0].folder, root);
    assert_eq!(sessions.of(&projects, root, 1).len(), 1);
    // Renamed in Claude Code: the file changed, its new name read.
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    let renamed = session("new", "/work/api.shop", "New one")
        + &format!(
            "{}\n",
            serde_json::json!({"type": "custom-title", "customTitle": "matching-system"})
        );
    std::fs::write(folder.join("new.jsonl"), renamed).unwrap();
    assert_eq!(sessions.of(&projects, root, 10)[0].title, "matching-system");
    // No folder for a project: none.
    assert!(sessions.of(&projects, Path::new("/nothing/here"), 10).is_empty());
    let _ = std::fs::remove_dir_all(projects);
}

/// Renamed as `/rename` does: its record added at the end of the session's own file (a line
/// of its own, the title escaped), read back as its title; no file, a bad id or a link:
/// nothing written.
#[test]
fn a_session_is_renamed_in_its_file() {
    let projects = scratch();
    let root = Path::new("/work/api.shop");
    let folder = projects.join(format::folder_name("/work/api.shop"));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("s1.jsonl");
    // Its last line not ended: the record still starts one of its own.
    std::fs::write(&file, session("s1", "/work/api.shop", "Old").trim_end()).unwrap();
    rename(&projects, root, "s1", "Fix \"login\" — ünïcode").unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    let last = text.lines().last().unwrap();
    let record: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(record["type"], "custom-title");
    assert_eq!(record["customTitle"], "Fix \"login\" — ünïcode");
    assert_eq!(record["sessionId"], "s1");
    assert!(text.ends_with('\n'));
    assert_eq!(Sessions::default().of(&projects, root, 10)[0].title, "Fix \"login\" — ünïcode");
    assert!(rename(&projects, root, "missing", "x").is_err(), "no file: none made");
    assert!(!folder.join("missing.jsonl").exists());
    assert!(rename(&projects, root, "../s1", "x").is_err(), "not an id");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&file, folder.join("link.jsonl")).unwrap();
        assert!(rename(&projects, root, "link", "x").is_err(), "a link isn't followed");
    }
    let _ = std::fs::remove_dir_all(projects);
}

/// A session resumed after its project moved: it started in the old folder and works in
/// the new one from then on (its end says so) — listed with the project where it is now.
#[test]
fn a_session_that_moved_with_its_project_is_listed_there() {
    let projects = scratch();
    let root = Path::new("/work/new/app");
    let folder = projects.join(format::folder_name("/work/new/app"));
    std::fs::create_dir_all(&folder).unwrap();
    // Longer than the bytes read from its start: its end read apart.
    let mut moved = session("moved", "/work/old/app", "Moved one");
    let filler = serde_json::json!({"type": "user", "cwd": "/work/old/app",
        "message": {"content": "x".repeat(1024)}})
    .to_string();
    while moved.len() < format::HEAD + format::TAIL {
        moved.push_str(&filler);
        moved.push('\n');
    }
    moved.push_str(
        &serde_json::json!({"type": "user", "cwd": "/work/new/app/crates",
            "message": {"content": "here"}})
        .to_string(),
    );
    moved.push('\n');
    moved.push_str(
        &serde_json::json!({"type": "user", "cwd": "/work/new/app", "message": {"content": "here"}})
            .to_string(),
    );
    moved.push('\n');
    std::fs::write(folder.join("moved.jsonl"), moved).unwrap();
    std::fs::write(folder.join("old.jsonl"), session("old", "/work/old/app", "Old only")).unwrap();
    let found = Sessions::default().of(&projects, root, 10);
    let titles: Vec<&str> = found.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Moved one"]);
    let _ = std::fs::remove_dir_all(projects);
}
