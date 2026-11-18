use super::*;
use crate::fixture::{TempDir, language_of, register_reads};

#[test]
fn the_keys_the_projects_code_reads() {
    register_reads();
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        ..Default::default()
    });
    let dir = TempDir::new("used");
    dir.write("src/a.php", "<?php getenv('A'); $_ENV['B'];");
    dir.write("config/x.yaml", "k: '%env(int:C)%'");
    dir.write("vendor/lib/z.php", "<?php getenv('LIBRARY');");
    dir.write("notes.txt", "getenv('TEXT')");
    let mut used = UsedKeys::default();
    used.refresh(dir.path(), &Default::default(), &language_of(), &code_intelligence::env::reads());
    let mut keys: Vec<&str> = used.keys().into_iter().collect();
    keys.sort();
    assert_eq!(keys, ["A", "B", "C"]);
    // A file changed: read again; one gone: dropped.
    std::fs::write(dir.path().join("src/a.php"), "<?php getenv('D');").unwrap();
    std::fs::remove_file(dir.path().join("config/x.yaml")).unwrap();
    used.refresh(dir.path(), &Default::default(), &language_of(), &code_intelligence::env::reads());
    assert_eq!(used.keys().into_iter().collect::<Vec<_>>(), ["D"]);
}
