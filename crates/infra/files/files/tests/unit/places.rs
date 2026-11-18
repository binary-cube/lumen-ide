//! A file kept before in the old folder is used where it is while the new folder has none;
//! asking never moves or writes anything.

use super::*;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("places-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn an_old_file_is_used_where_it_is_until_the_new_folder_has_one() {
    let d = dir("resolve");
    let legacy = d.join("old").join("known_hosts");
    let file = d.join("new").join("known_hosts");
    // Neither: the new one (nothing made).
    assert_eq!(resolved(Some(legacy.clone()), file.clone()), file);
    assert!(!file.exists() && !legacy.exists());
    // Only the old one: it, left where it is.
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, "host key").unwrap();
    assert_eq!(resolved(Some(legacy.clone()), file.clone()), legacy);
    assert!(legacy.exists() && !file.exists(), "never moved");
    // The new one there: it.
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "new key").unwrap();
    assert_eq!(resolved(Some(legacy), file.clone()), file);
    assert_eq!(resolved(None, file.clone()), file);
    let _ = std::fs::remove_dir_all(&d);
}
