//! The data sources' lists: a project's and the config folder's written and read back the
//! same, never a secret written, written through a temporary file, a broken file an error
//! (never read as empty: it would be written over).

use std::collections::BTreeMap;

use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("data-sources-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn server() -> DataSource {
    let mut source = DataSource::new("prod", "postgres", Scope::Global);
    source.target.host = "db.example.com".into();
    source.target.port = Some(5432);
    source.target.user = "app".into();
    source.target.database = "shop".into();
    source.target.tls = Tls::Verify;
    source.target.options = BTreeMap::from([("sslmode".to_owned(), "require".to_owned())]);
    source.read_only = true;
    source.secrets = SecretPolicy::Keychain;
    source.comment = "the live \"shop\"".into();
    source.schemas = vec!["public".into(), "billing".into()];
    source.databases = vec!["shop".into(), "analytics".into()];
    source.tags =
        vec![Tag { name: "prod".into(), color: kernel::Rgba::from_hex("#e5484d").unwrap() }];
    source
}

fn file() -> DataSource {
    let mut source = DataSource::new("shop", "sqlite", Scope::Project);
    source.file = Some(PathBuf::from("data").join("shop.sqlite"));
    source.secrets = SecretPolicy::Never;
    source
}

#[test]
fn both_lists_are_written_and_read_back_apart() {
    let dir = temp("round");
    let (project, config) = (dir.join("project"), dir.join("config"));
    let store = TomlDataSources::at(Some(&project), Some(config.clone()));
    assert_eq!(store.load(Scope::Project), Ok(SourceList::default()), "no file: none");
    let (global, local) = (SourceList::of(vec![server()]), SourceList::of(vec![file()]));
    let saved = (store.save(Scope::Global, &global), store.save(Scope::Project, &local));
    let loaded = (store.load(Scope::Global), store.load(Scope::Project));
    let project_file = std::fs::read_to_string(project.join(kernel::brand::PROJECT_DIR).join(FILE));
    let config_file = std::fs::read_to_string(config.join(FILE));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(saved, (Ok(()), Ok(())));
    assert_eq!(loaded, (Ok(global), Ok(local)));
    // The project's file names its file with `/` (it moves with the project), the global
    // one holds only the server.
    assert!(project_file.unwrap().contains("file = \"data/shop.sqlite\""));
    assert!(!config_file.unwrap().contains("sqlite"));
}

#[test]
fn no_secret_is_ever_written() {
    let dir = temp("secrets");
    let store = TomlDataSources::at(None, Some(dir.clone()));
    let mut source = server();
    source.target.options.insert("password".into(), "hunter2".into());
    source.target.options.insert("ssh_passphrase".into(), "hunter3".into());
    let saved = store.save(Scope::Global, &SourceList::of(vec![source]));
    let text = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
    let loaded = store.load(Scope::Global);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(saved.is_ok());
    assert!(!text.contains("hunter"), "{text}");
    assert!(!text.contains("password") && !text.contains("passphrase"), "{text}");
    assert_eq!(loaded.unwrap().sources[0].target.options.len(), 1, "only sslmode");
}

#[test]
fn a_write_replaces_the_file_whole_and_leaves_no_temporary() {
    let dir = temp("atomic");
    let store = TomlDataSources::at(Some(&dir), None);
    let _ = store.save(Scope::Project, &SourceList::of(vec![file(), file()]));
    let _ = store.save(Scope::Project, &SourceList::of(vec![file()]));
    let folder = dir.join(kernel::brand::PROJECT_DIR);
    let names: Vec<String> = std::fs::read_dir(&folder)
        .map(|d| {
            d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()
        })
        .unwrap_or_default();
    let loaded = store.load(Scope::Project);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(names, [FILE]);
    assert_eq!(loaded.map(|l| l.sources.len()), Ok(1));
}

#[test]
fn a_broken_file_is_an_error_not_an_empty_list() {
    let dir = temp("broken");
    let store = TomlDataSources::at(Some(&dir), None);
    let path = dir.join(kernel::brand::PROJECT_DIR).join(FILE);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let mut outcomes = Vec::new();
    for text in [
        "[[source]\nname = ",
        "source = 3",
        "[[source]]\nname = \"no id\"\ndriver = \"sqlite\"",
        "[[source]]\nid = \"0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10\"\nname = \"no driver\"",
        "[[source]]\nid = \"0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10\"\ndriver = \"x\"\nport = 99999",
    ] {
        let _ = std::fs::write(&path, text);
        outcomes.push(store.load(Scope::Project).is_err());
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(outcomes, [true; 5]);
    // A missing config folder (none known) is simply no list.
    assert_eq!(TomlDataSources::at(None, None).load(Scope::Global), Ok(SourceList::default()));
}

#[test]
fn virtual_keys_and_relations_set_aside_are_kept_with_their_source() {
    let dir = temp("relations");
    let store = TomlDataSources::at(Some(&dir), None);
    let mut source = file();
    let key = |table: TableRef, column: &str, target: TableRef| VirtualKey {
        table,
        columns: vec![column.to_owned()],
        target,
        references: vec!["id".to_owned()],
    };
    source.relations.accept(key(TableRef::new("reviews"), "productId", TableRef::new("products")));
    source.relations.accept(key(
        TableRef::in_schema("billing", "payments"),
        "order_id",
        TableRef::in_schema("shop", "orders"),
    ));
    source.relations.ignore(key(TableRef::new("shipments"), "fk_order", TableRef::new("orders")));
    source.relations.show = RelationKinds::Inferred;
    store.save(Scope::Project, &SourceList::of(vec![source.clone()])).unwrap();
    let text = std::fs::read_to_string(store.file(Scope::Project).unwrap()).unwrap();
    assert!(
        text.contains("[[source.virtual_key]]") && text.contains("[[source.ignored_relation]]"),
        "{text}"
    );
    assert!(text.contains("relations = \"inferred\""));
    assert_eq!(store.load(Scope::Project).unwrap().sources, [source.clone()]);
    // Nothing decided: nothing written.
    let plain = file();
    store.save(Scope::Project, &SourceList::of(vec![plain])).unwrap();
    let text = std::fs::read_to_string(store.file(Scope::Project).unwrap()).unwrap();
    assert!(!text.contains("virtual_key") && !text.contains("relations"), "{text}");
    // A key without its table can't be read: the file is an error, never written over.
    std::fs::write(
        store.file(Scope::Project).unwrap(),
        "[[source]]\nid = \"0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10\"\ndriver = \"sqlite\"\n[[source.virtual_key]]\ncolumns = [\"a\"]\ntarget = \"t\"\nreferences = [\"id\"]\n",
    )
    .unwrap();
    assert!(store.load(Scope::Project).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn folders_and_colors_are_kept_with_their_list() {
    let dir = temp("folders");
    let store = TomlDataSources::at(Some(&dir), None);
    let mut prod = SourceFolder::new("Production", Scope::Project);
    prod.color = Some(SourceColor::Red);
    let mut eu = SourceFolder::new("EU", Scope::Project);
    eu.parent = Some(prod.id);
    eu.color = Some(SourceColor::Custom(0x3366cc));
    let mut shop = file();
    shop.folder = Some(eu.id);
    let mut loose = file();
    loose.id = DataSourceId::new();
    loose.color = Some(SourceColor::Green);
    let list = SourceList::new(vec![shop, loose], vec![prod, eu]);
    store.save(Scope::Project, &list).unwrap();
    let text = std::fs::read_to_string(store.file(Scope::Project).unwrap()).unwrap();
    assert!(text.contains("[[folder]]") && text.contains("color = \"red\""), "{text}");
    assert!(text.contains("color = \"#3366cc\""), "{text}");
    assert_eq!(store.load(Scope::Project).unwrap(), list);
    // A source in a folder that isn't there (edited by hand): at the top, never lost.
    let mut lost = file();
    lost.folder = Some(database::FolderId::new());
    store.save(Scope::Project, &SourceList::of(vec![lost])).unwrap();
    let read = store.load(Scope::Project).unwrap();
    assert_eq!(read.sources.len(), 1);
    assert_eq!(read.sources[0].folder, None);
    // A color that isn't one: the file is an error, never written over.
    std::fs::write(
        store.file(Scope::Project).unwrap(),
        "[[source]]\nid = \"0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10\"\ndriver = \"sqlite\"\ncolor = \"crimson\"\n",
    )
    .unwrap();
    assert!(store.load(Scope::Project).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_from_before_folders_reads_as_it_did() {
    let dir = temp("old");
    let store = TomlDataSources::at(Some(&dir), None);
    let path = store.file(Scope::Project).unwrap().to_path_buf();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    std::fs::write(
        &path,
        "[[source]]\nid = \"0b6f2c7e-6a4e-4c1f-9d55-3e1f0c2b7a10\"\nname = \"shop\"\ndriver = \"sqlite\"\nfile = \"shop.sqlite\"\n",
    )
    .unwrap();
    let read = store.load(Scope::Project).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(read.folders.is_empty());
    assert_eq!(read.sources.len(), 1);
    assert_eq!((read.sources[0].folder, read.sources[0].color), (None, None));
    assert!(!read.sources[0].production, "no mark: not production");
}

#[test]
fn the_production_mark_is_kept_on_sources_and_folders() {
    let dir = temp("production");
    let store = TomlDataSources::at(Some(&dir), None);
    let mut prod = SourceFolder::new("Production", Scope::Project);
    prod.production = true;
    let mut shop = file();
    shop.folder = Some(prod.id);
    let mut live = file();
    live.id = DataSourceId::new();
    live.production = true;
    let calm = {
        let mut s = file();
        s.id = DataSourceId::new();
        s
    };
    let list = SourceList::new(vec![shop, live, calm], vec![prod]);
    store.save(Scope::Project, &list).unwrap();
    let text = std::fs::read_to_string(store.file(Scope::Project).unwrap()).unwrap();
    assert_eq!(text.matches("production = true").count(), 2, "only where marked: {text}");
    assert_eq!(store.load(Scope::Project).unwrap(), list);
    let _ = std::fs::remove_dir_all(dir);
}
