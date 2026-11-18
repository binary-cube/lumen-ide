use lsp_client::fake::Behaviour;

use super::*;
use crate::harness::{self, Harness, wait};

#[test]
fn a_secret_is_looked_up_off_the_caller_and_kept_known() {
    let _h = Harness::new(Behaviour::default());
    harness::fresh_installs("known");
    // Unknown at first: looked up in the background (`None` meanwhile).
    assert_eq!(is_set("installable-licence", None), None);
    wait("known", || is_set("installable-licence", None).is_some());
    assert_eq!(is_set("installable-licence", None), Some(false));
    save("installable-licence", "KEY").unwrap();
    assert_eq!(is_set("installable-licence", None), Some(true));
    save("installable-licence", " ").unwrap();
    assert_eq!(is_set("installable-licence", None), Some(false));
    assert!(harness::secrets().0.lock().unwrap().is_empty());
}

#[test]
fn a_licence_file_is_only_read_and_its_key_kept_in_the_secrets() {
    let _h = Harness::new(Behaviour::default());
    harness::fresh_installs("file");
    let dir = harness::temp().join("licence-home");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("licence.txt");
    // Not left by an earlier run whose process had the same id.
    let _ = std::fs::remove_file(&file);
    use_licence_files(vec![dir.join("absent.txt"), file.clone()]);
    assert_eq!(licence_file(), None);
    std::fs::write(&file, "FILE-KEY-7\n").unwrap();
    assert_eq!(licence_file(), Some(file.clone()));
    import("installable-licence", &file).unwrap();
    assert_eq!(
        harness::secrets().0.lock().unwrap().get("installable-licence").map(String::as_str),
        Some("FILE-KEY-7")
    );
    // The file is left as it was.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "FILE-KEY-7\n");
    std::fs::write(&file, "\n").unwrap();
    assert!(import("installable-licence", &file).is_err());
    use_licence_files(vec![harness::temp().join("no-licence.txt")]);
}
