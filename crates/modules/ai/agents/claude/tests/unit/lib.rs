use super::*;

/// It offers itself as an agent and brings its icon; a new session starts in the project,
/// one taken up again names its id, in its own folder.
#[test]
fn it_offers_itself_and_says_how_sessions_start() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(ClaudeModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let agents = contributions.take::<Arc<dyn AgentProvider>>();
    assert_eq!(agents.iter().map(|a| a.id()).collect::<Vec<_>>(), [ID]);
    let icons = contributions.take::<languages::IconSvg>();
    assert_eq!(icons.iter().map(|i| i.name).collect::<Vec<_>>(), [ICON]);
    let claude = Claude::default();
    let root = Path::new("/work/p");
    let new = claude.new_session(root);
    assert_eq!(new.cwd.as_deref(), Some(root));
    assert!(new.args.is_empty());
    let session = AgentSession {
        agent: ID,
        id: "abc-1".into(),
        title: "t".into(),
        folder: PathBuf::from("/work/q"),
        started: None,
        active: 0,
        file: None,
    };
    let resumed = claude.resume(&session);
    assert_eq!(resumed.args, ["--resume", "abc-1"]);
    assert_eq!(resumed.cwd.as_deref(), Some(Path::new("/work/q")));
}

/// Its sessions are read from the folder it's given (the user's, else a test's).
#[test]
fn its_sessions_come_from_its_config_folder() {
    let config = std::env::temp_dir().join(format!("agent-claude-cfg-{}", uuid::Uuid::new_v4()));
    let folder = config.join("projects").join(format::folder_name("/work/p"));
    std::fs::create_dir_all(&folder).unwrap();
    let row = serde_json::json!({"type": "custom-title", "customTitle": "named", "sessionId": "x"});
    std::fs::write(folder.join("x.jsonl"), format!("{row}\n")).unwrap();
    let claude = Claude::with_config(config.clone());
    let found = claude.sessions(Path::new("/work/p"), 5);
    assert_eq!(found.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["named"]);
    let _ = std::fs::remove_dir_all(config);
}

/// What listing a real project's sessions costs, cold and then kept (`AGENT_ROOT`: the
/// project's folder): `cargo test -p agent-claude --release measure_sessions -- --ignored
/// --nocapture`. Reads only; prints counts and times, no names.
#[test]
#[ignore = "a measurement on the user's own sessions"]
fn measure_sessions() {
    let root = PathBuf::from(std::env::var("AGENT_ROOT").expect("AGENT_ROOT"));
    let claude = Claude::default();
    for pass in ["cold", "kept"] {
        let started = std::time::Instant::now();
        let found = claude.sessions(&root, 50);
        let titled = found.iter().filter(|s| !s.title.is_empty()).count();
        println!("{pass}: {} sessions ({titled} titled) in {:?}", found.len(), started.elapsed());
    }
}

/// Its graphical face: the adapter told which Claude Code to run and which protocol to
/// speak; installed with npm, as a command shown.
#[test]
fn its_adapter_starts_with_its_claude() {
    let claude = Claude::default();
    if let Some(install) = claude.acp_install() {
        assert_eq!(install.args, ["install", "-g", ACP_PACKAGE]);
    }
    if let Some(launch) = claude.acp(Path::new("/work/p")) {
        assert!(launch.program.ends_with(ACP_ADAPTER) || launch.program.contains(ACP_ADAPTER));
        assert!(launch.env.iter().any(|(k, v)| k == "CLAUDE_AGENT_ACP_PROTOCOL" && v == "v1"));
        assert_eq!(launch.cwd.as_deref(), Some(Path::new("/work/p")));
    }
}
