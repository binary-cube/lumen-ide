use super::*;

/// Started in its terminal interface in the project; taken up again by its id, in the
/// session's folder; listed from its own data folder.
#[test]
fn cline_starts_and_resumes_in_its_terminal_interface() {
    let data = std::env::temp_dir().join(format!("cline-{}", uuid::Uuid::new_v4()));
    let cline = Cline::with_data(data);
    let new = cline.new_session(Path::new("/p"));
    assert_eq!(new.args, ["--tui"]);
    assert_eq!(new.cwd.as_deref(), Some(Path::new("/p")));
    let session = AgentSession {
        agent: ID,
        id: "1785_x".into(),
        title: "t".into(),
        folder: PathBuf::from("/p"),
        started: None,
        active: 0,
        file: None,
    };
    assert_eq!(cline.resume(&session).args, ["--tui", "--id", "1785_x"]);
    assert!(cline.sessions(Path::new("/p"), 10).is_empty(), "none yet");
    assert!(!cline.can_rename(), "its sessions aren't renamed by the editor");
}
