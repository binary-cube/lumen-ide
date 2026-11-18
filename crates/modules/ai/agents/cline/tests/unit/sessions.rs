use super::*;

/// A session's file as Cline writes it: its id, its folder, its title (else its first
/// request, its markup taken out), its times.
#[test]
fn a_session_s_file_is_read() {
    let text = br#"{"version":"1","session_id":"1785502698506_ug05k","started_at":"2026-07-31T12:58:59.557Z",
        "ended_at":"2026-07-31T13:04:21.362Z","cwd":"/p/app","workspace_root":"/p/app",
        "prompt":"<user_input mode=\"act\">make a calculator</user_input>","metadata":{"title":"Calculator"}}"#;
    let f = facts(text).unwrap();
    assert_eq!(f.id, "1785502698506_ug05k");
    assert_eq!(f.cwd.as_deref(), Some("/p/app"));
    assert_eq!(f.title.as_deref(), Some("Calculator"));
    assert_eq!(f.ended.unwrap() - f.started.unwrap(), 322);
    let untitled = br#"{"session_id":"a1","cwd":"/p","prompt":"<user_input mode=\"act\">fix\nlogin</user_input>"}"#;
    assert_eq!(facts(untitled).unwrap().title.as_deref(), Some("fix login"));
    assert!(facts(br#"{"session_id":"../x"}"#).is_none(), "an id that can't name a file");
    assert!(facts(b"not json").is_none());
}

/// The project's sessions only, newest first, at most as many as asked; a link to a folder
/// not followed.
#[test]
fn the_project_s_sessions_are_listed() {
    let dir = std::env::temp_dir().join(format!("cline-sessions-{}", uuid::Uuid::new_v4()));
    let write = |id: &str, cwd: &str| {
        std::fs::create_dir_all(dir.join(id)).unwrap();
        let text = format!(r#"{{"session_id":"{id}","cwd":"{cwd}","prompt":"task {id}"}}"#);
        std::fs::write(dir.join(id).join(format!("{id}.json")), text).unwrap();
    };
    write("a1", "/p/app");
    write("b2", "/p/other");
    write("c3", "/p/app/");
    let sessions = Sessions::default();
    let found = sessions.of(&dir, Path::new("/p/app"), 10);
    let mut ids: Vec<&str> = found.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["a1", "c3"]);
    assert!(found.iter().all(|s| s.agent == crate::ID && s.title.starts_with("task")));
    assert_eq!(sessions.of(&dir, Path::new("/p/app"), 1).len(), 1);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("a1"), dir.join("d4")).unwrap();
        assert_eq!(sessions.of(&dir, Path::new("/p/app"), 10).len(), 2, "a link isn't followed");
    }
    let _ = std::fs::remove_dir_all(dir);
}
