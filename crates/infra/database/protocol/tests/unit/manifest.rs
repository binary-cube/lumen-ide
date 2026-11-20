use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

const DUCK: &str = r#"
protocol = { min = 1, max = 2 }
command = "bin/duck-driver"
args = ["--stdio", "two words"]
timeout = 30

[env]
DUCK_HOME = "/opt/duck"

[driver]
id = "duck.db"
name = "DuckDB"
version = "1.1"
kind = "relational"
default_port = 4000
sql = "postgres"

[driver.capabilities]
edit_rows = true
file_based = true
sorting = { up_to = 1000 }
schema_changes = { columns = true, renames = true }

[driver.files]
extensions = ["duckdb", "DDB"]
header_hex = "4455 434b"

[driver.connection]
fields = [
    { id = "file", label = "File", kind = { type = "file", extensions = ["duckdb"] }, binding = "file", required = true },
    { id = "mode", label = "Mode", kind = { type = "choice", options = [{ value = "ro", label = "Read only" }] }, binding = { option = "mode" }, tab = "options" },
]
url = { schemes = ["duckdb"], style = "file" }

[[driver.value_editors]]
id = "duck-uuid"
types = ["uuid"]
format = "text"
priority = 3
"#;

#[test]
fn a_manifest_says_how_to_start_the_driver_and_what_it_is() {
    let m = Manifest::parse(DUCK).unwrap();
    assert_eq!(m.protocol, dto::Versions { min: 1, max: 2 });
    assert_eq!((m.command.as_str(), m.args.len()), ("bin/duck-driver", 2));
    assert_eq!(m.timeout(), Duration::from_secs(30));
    assert_eq!(m.env.get("DUCK_HOME").map(String::as_str), Some("/opt/duck"));
    let d = &m.driver;
    assert_eq!((d.id.as_str(), d.sql, d.default_port), ("duck.db", dto::Sql::Postgres, Some(4000)));
    assert!(d.capabilities.edit_rows && d.capabilities.schema_changes.renames);
    assert!(!d.capabilities.transactions, "what it doesn't say it doesn't do");
    assert_eq!(d.capabilities.sorting, dto::Sorting::UpTo(1000));
    assert_eq!(m.header(), b"DUCK");
    let spec = database::ConnectionSpec::from(d.connection.clone().unwrap());
    assert_eq!(spec.fields.len(), 2);
    assert_eq!(spec.field("mode").unwrap().binding, database::Binding::Option("mode".into()));
    assert_eq!(d.value_editors[0].format, dto::ValueFormat::Text);
    // A server's fields when none are given; the default request timeout.
    let plain = Manifest::parse(
        "protocol = { min = 1, max = 1 }\ncommand = \"x\"\n[driver]\nid = \"x\"\nname = \"X\"",
    )
    .unwrap();
    assert_eq!(plain.timeout(), REQUEST_TIMEOUT);
    let spec = crate::driver::connection(&plain.driver);
    assert!(spec.field("host").is_some() && spec.field("password").is_some());
}

#[test]
fn a_manifest_that_cant_be_right_is_refused_with_why() {
    let with = |change: &str, by: &str| Manifest::parse(&DUCK.replace(change, by)).unwrap_err();
    assert!(with("id = \"duck.db\"", "id = \"Duck DB\"").contains("driver id"));
    assert!(with("name = \"DuckDB\"", "name = \" \"").contains("no name"));
    assert!(with("command = \"bin/duck-driver\"", "command = \"\"").contains("no command"));
    assert!(with("min = 1, max = 2", "min = 3, max = 2").contains("backwards"));
    assert!(with("DUCK_HOME", "DUCK_PASSWORD").contains("password"));
    assert!(with("4455 434b", "44X").contains("hexadecimal"));
    assert!(with("extensions = [\"duckdb\", \"DDB\"]", "extensions = []").contains("extensions"));
    assert!(Manifest::parse("protocol = 1").is_err(), "not a manifest");
}

struct Never(AtomicUsize);

impl Launch for Never {
    fn launch(&self) -> Result<Transport, DbError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(DbError::Connect("not in this test".into()))
    }
}

#[test]
fn an_installed_driver_is_listed_and_its_files_known_without_starting_it() {
    let m = Manifest::parse(DUCK).unwrap();
    let launch = Arc::new(Never(AtomicUsize::new(0)));
    let f = factory(&m, Arc::clone(&launch) as Arc<dyn Launch>);
    assert_eq!(
        (&*f.id, &*f.name, f.external, f.default_port),
        ("duck.db", "DuckDB", true, Some(4000))
    );
    let driver = (f.create)();
    assert!(driver.capabilities().file_based);
    assert!(driver.dialect().is_some(), "the editor writes its SQL");
    assert_eq!(driver.connection().fields.len(), 2);
    assert_eq!(launch.0.load(Ordering::SeqCst), 0, "nothing started");
    let files = files(&m).unwrap();
    assert!(files.recognizes(Path::new("/d/a.ddb"), b"DUCK and more"));
    assert!(!files.recognizes(Path::new("/d/a.ddb"), b"SQLite"));
    assert!(!files.recognizes(Path::new("/d/a.db"), b"DUCK"));
    assert!(driver.connect(&database::Target::default(), &database::Secret::default()).is_err());
    assert_eq!(launch.0.load(Ordering::SeqCst), 1, "started when a session opens");
}

#[test]
fn drivers_are_found_each_in_its_folder_and_a_broken_one_said() {
    let dir = std::env::temp_dir().join(format!("driver-manifests-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (folder, text) in
        [("b-duck", Some(DUCK)), ("a-broken", Some("protocol = 1")), ("c-empty", None)]
    {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
        if let Some(text) = text {
            std::fs::write(dir.join(folder).join(FILE), text).unwrap();
        }
    }
    std::fs::write(dir.join(FILE), DUCK).unwrap();
    let found = discover(&dir);
    assert_eq!(found.len(), 2, "only folders with a manifest");
    let (broken, why) = found[0].clone().unwrap_err();
    assert!(broken.ends_with("a-broken/driver.toml") || broken.ends_with("a-broken\\driver.toml"));
    assert!(!why.is_empty());
    let duck = found[1].clone().unwrap();
    assert_eq!(duck.dir, dir.join("b-duck"));
    assert!(discover(&dir.join("missing")).is_empty());
    // Its program: next to its manifest when there, else as written there.
    std::fs::create_dir_all(dir.join("b-duck/bin")).unwrap();
    std::fs::write(dir.join("b-duck/bin/duck-driver"), "").unwrap();
    let launch = ProgramLaunch::of(&duck);
    assert_eq!(launch.program, dir.join("b-duck/bin/duck-driver"));
    assert_eq!(launch.dir, dir.join("b-duck"));
    assert_eq!(program(&dir, "surely-missing-driver"), dir.join("surely-missing-driver"));
    let missing = ProgramLaunch { program: dir.join("nothing-here"), ..launch }.launch();
    assert!(matches!(
        missing.err(),
        Some(DbError::Driver(DriverFailure::NotStarted { driver, .. })) if driver == "DuckDB"
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A manifest's console says what its profiles hold (protocol 3); one that doesn't, what its
/// plans give.
#[test]
fn a_manifest_says_what_its_profiles_hold() {
    let manifest = |profile: &str| {
        format!(
            "protocol = {{ min = 1, max = 3 }}\ncommand = \"x\"\n[driver]\nid = \"x\"\nname = \"X\"\n\
             [driver.capabilities.console]\nexplain = {{ plan = true, analyze = true }}\n{profile}"
        )
    };
    let m = Manifest::parse(&manifest(
        "[driver.capabilities.console.profile]\nestimated = true\nactual = true\ntable_stats = true\nundo_writes = true",
    ))
    .unwrap();
    let console: database::ConsoleCaps = m.driver.capabilities.console.unwrap().into();
    assert!(console.profile.actual && console.profile.table_stats && !console.profile.buffers);
    let m = Manifest::parse(&manifest("")).unwrap();
    let console: database::ConsoleCaps = m.driver.capabilities.console.unwrap().into();
    assert_eq!(console.profile, database::ProfileCaps::from_explain(console.explain));
}

/// A driver's icon: one of the editor's it names, in its tone; one the editor hasn't (or a
/// path to a drawing of its own) isn't taken: its sources keep the icon of their kind.
#[test]
fn a_driver_s_icon_is_one_of_the_editor_s_in_its_tone() {
    let with = |icon: &str, tone: &str| {
        let text = format!(
            "protocol = {{ min = 1, max = 1 }}\ncommand = \"x\"\n[driver]\nid = \"x\"\nname = \"X\"\nicon = \"{icon}\"\nicon_tone = \"{tone}\""
        );
        let m = Manifest::parse(&text).unwrap();
        factory(&m, Arc::new(Never(AtomicUsize::new(0)))).icon
    };
    assert_eq!(
        with("hard-drive", "yellow"),
        Some(database::DriverIcon { name: "hard-drive", tone: database::IconTone::Yellow })
    );
    assert_eq!(with("cloud", "sepia").map(|i| i.tone), Some(database::IconTone::Neutral));
    assert_eq!(with("icons/duck.svg", "blue"), None, "nothing of its folder is drawn");
    assert_eq!(with("engine-postgres", "blue"), None, "another driver's isn't its");
}
