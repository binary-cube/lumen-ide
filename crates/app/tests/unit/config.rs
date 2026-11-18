//! The app's tests never see the user's folders: the config and data folders are a
//! temporary one of the process, the settings a file per thread in it.

use super::*;

#[test]
fn the_tests_folders_are_temporary_never_the_users() {
    let temp = std::env::temp_dir();
    let config = config_dir().unwrap();
    let data = data_dir().unwrap();
    assert!(config.starts_with(&temp) && data.starts_with(&temp), "{config:?} {data:?}");
    assert_eq!(storage::config_dir(), Some(config), "the modules' too, through storage");
    assert_ne!(storage::data_dir(), storage::config_dir());
    // The modules' own files too (remote connections, known hosts, git's trusted list),
    // never moved from the user's old folder in a test; the caches.
    for name in ["remote-connections.toml", "known_hosts", "trusted-repositories"] {
        let file = files::places::config_file(name).unwrap();
        assert!(file.starts_with(&temp), "{file:?}");
    }
    assert!(files::places::cache_dir().unwrap().starts_with(&temp));
    if let Some(home) = dirs::home_dir() {
        assert!(!storage::config_dir().unwrap().starts_with(home.join(".config")));
    }
}

#[test]
fn each_test_thread_has_its_own_settings() {
    let here = settings();
    here.set(&crate::settings::AUTOSAVE, false).unwrap();
    assert!(!here.get(&crate::settings::AUTOSAVE));
    let other = std::thread::spawn(|| {
        let there = settings();
        (there.get(&crate::settings::AUTOSAVE), std::ptr::from_ref(there) as usize)
    })
    .join()
    .unwrap();
    assert!(other.0, "another thread's file: the default");
    assert_ne!(other.1, std::ptr::from_ref(here) as usize);
    assert!(std::ptr::eq(here, settings()), "the same service on the same thread");
}

/// Earlier processes' tests' folders are swept once old (one running keeps its own).
#[test]
fn old_test_folders_are_swept() {
    let temp = std::env::temp_dir().join(format!("sweep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(temp.join(format!("{TEST_HOME}1")).join("config")).unwrap();
    std::fs::create_dir_all(temp.join("other")).unwrap();
    sweep_test_homes(&temp, std::time::Duration::from_secs(3600));
    assert!(temp.join(format!("{TEST_HOME}1")).exists(), "recent: kept");
    sweep_test_homes(&temp, std::time::Duration::ZERO);
    assert!(!temp.join(format!("{TEST_HOME}1")).exists(), "old: swept");
    assert!(temp.join("other").exists(), "not a tests' folder: left alone");
    let _ = std::fs::remove_dir_all(&temp);
}
