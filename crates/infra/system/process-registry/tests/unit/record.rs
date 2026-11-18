use super::*;

fn record(pid: u32, kind: &str, project: Option<&str>) -> Record {
    Record {
        pid,
        started: 1_700_000_000,
        kind: kind.to_owned(),
        program: "zsh".to_owned(),
        project: project.map(str::to_owned),
    }
}

/// A file read back is the file written; anything else isn't one of ours.
#[test]
fn a_file_reads_back_as_written() {
    let file = InstanceFile {
        app_pid: 42,
        app_started: 1_700_000_001,
        programs: vec![
            record(7, "terminal", Some("/work/shop")),
            record(8, "server", None),
            Record { program: "a \"quoted\"\nname".into(), ..record(9, "agent", Some("/w")) },
        ],
    };
    let text = file.to_text();
    assert_eq!(InstanceFile::parse(&text), Some(file));
    assert_eq!(InstanceFile::parse("not = [toml"), None);
    assert_eq!(InstanceFile::parse("app_pid = \"x\""), None);
}

/// At most so many programs kept, a big file never read.
#[test]
fn files_are_bounded() {
    let file = InstanceFile {
        app_pid: 1,
        app_started: 1,
        programs: (0..MAX_RECORDS as u32 + 10).map(|p| record(p, "task", None)).collect(),
    };
    assert_eq!(InstanceFile::parse(&file.to_text()).unwrap().programs.len(), MAX_RECORDS);
    let dir = std::env::temp_dir().join(kernel::id::uuid_text(kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("big.toml");
    std::fs::write(&big, "#".repeat(MAX_FILE as usize + 1)).unwrap();
    assert_eq!(InstanceFile::read(&big), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A program belongs to the project it ran in, or in one of its folders.
#[test]
fn a_program_belongs_to_its_projects_folder() {
    let r = record(7, "terminal", Some("/work/shop/src"));
    assert!(r.belongs_to(Path::new("/work/shop")));
    assert!(!r.belongs_to(Path::new("/work/sho")));
    assert!(!record(7, "terminal", None).belongs_to(Path::new("/")));
    assert_eq!(r.kind(), Kind::Terminal);
}
