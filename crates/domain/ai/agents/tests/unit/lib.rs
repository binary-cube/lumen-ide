use super::*;

/// A launch shown as typed: the parts with spaces quoted, an empty one too.
#[test]
fn a_launch_reads_as_its_command() {
    let launch = Launch::new("claude", &["--resume", "abc"], Some(Path::new("/p")));
    assert_eq!(launch.display(), "claude --resume abc");
    assert_eq!(launch.cwd.as_deref(), Some(Path::new("/p")));
    let spaced = Launch::new("/Applications/My Agent/bin", &["--name", "two words", ""], None);
    assert_eq!(spaced.display(), "\"/Applications/My Agent/bin\" --name \"two words\" \"\"");
}

/// A planned agent is listed until a provider offers it; registered again, it's replaced.
#[test]
fn a_planned_agent_gives_way_to_its_provider() {
    #[derive(Debug)]
    struct Hosted;
    impl AgentProvider for Hosted {
        fn id(&self) -> &'static str {
            "planned-test-hosted"
        }
        fn name(&self) -> String {
            "Hosted".into()
        }
        fn icon(&self) -> &'static str {
            "x"
        }
        fn detect(&self) -> Option<AgentInstall> {
            None
        }
        fn sessions(&self, _root: &Path, _limit: usize) -> Vec<AgentSession> {
            Vec::new()
        }
        fn new_session(&self, root: &Path) -> Launch {
            Launch::new("x", &[], Some(root))
        }
        fn resume(&self, session: &AgentSession) -> Launch {
            Launch::new("x", &[], Some(&session.folder))
        }
    }
    let id = "planned-test-hosted";
    register_planned(PlannedAgent { id, name: "Old", icon: "x" });
    register_planned(PlannedAgent { id, name: "New", icon: "x" });
    let mine: Vec<_> = planned().into_iter().filter(|p| p.id == id).collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].name, "New");
    register(Arc::new(Hosted));
    assert!(planned().iter().all(|p| p.id != id), "offered by a provider: not planned");
    unregister(id);
}
