use super::*;
use app_updates::{Manifest, ManifestFile, Release, Version};
use pinned_downloads::fetch::Memory;
use pinned_downloads::samples::{ZipEntry, zip};

const URL: &str = "https://github.com/o/r/releases/download/v2.0.0/app-2.0.0-windows-x64.zip";

fn temp() -> PathBuf {
    std::env::temp_dir()
        .join(format!("self-update-{}", kernel::id::uuid_text(kernel::id::new_uuid())))
}

fn available(bytes: &[u8], sha256: String) -> Available {
    let version = Version::parse("2.0.0").unwrap();
    let file = ManifestFile {
        platform: "windows-x64".into(),
        name: "app-2.0.0-windows-x64.zip".into(),
        size: bytes.len() as u64,
        sha256,
    };
    Available {
        release: Release {
            version: version.clone(),
            tag: "v2.0.0".into(),
            name: "App 2.0.0".into(),
            notes: String::new(),
            prerelease: false,
            published: String::new(),
            page: String::new(),
            assets: Vec::new(),
        },
        manifest: Manifest { version, date: String::new(), files: vec![file.clone()] },
        file,
        url: URL.into(),
    }
}

/// The file downloaded, checked, unpacked into the version's folder, the new app found and
/// noted; asked again, it's checked, not fetched again.
#[test]
fn an_update_is_downloaded_checked_unpacked_and_noted() {
    let bytes = zip(&[ZipEntry("app.exe", Some(0o755), b"new program", true)]);
    let memory = Memory::default();
    memory.serve(URL, bytes.clone());
    let root = temp();
    let stop = AtomicBool::new(false);
    let ok = available(&bytes, pinned_downloads::digest::sha256_hex(&bytes));
    let staged = stage(&memory, &ok, &root, "app.exe", &|_, _| {}, &stop).unwrap();
    assert_eq!(staged.version, "2.0.0");
    assert_eq!(std::fs::read(&staged.app).unwrap(), b"new program");
    assert_eq!(read(&staged.dir), Some(staged.clone()), "noted for the helper");
    memory.forget_asked();
    stage(&memory, &ok, &root, "app.exe", &|_, _| {}, &stop).unwrap();
    assert!(memory.asked().is_empty(), "downloaded before: not fetched again");
    std::fs::remove_dir_all(&root).unwrap();
}

/// A download that doesn't match the manifest is refused (nothing staged); an archive
/// without the app either.
#[test]
fn what_doesnt_match_or_has_no_app_is_refused() {
    let bytes = zip(&[ZipEntry("other.txt", None, b"x", false)]);
    let memory = Memory::default();
    memory.serve(URL, bytes.clone());
    let root = temp();
    let stop = AtomicBool::new(false);
    let wrong = available(&bytes, "0".repeat(64));
    assert!(stage(&memory, &wrong, &root, "app.exe", &|_, _| {}, &stop).is_err());
    let right = available(&bytes, pinned_downloads::digest::sha256_hex(&bytes));
    let missing = stage(&memory, &right, &root, "app.exe", &|_, _| {}, &stop).unwrap_err();
    assert!(missing.contains("no app.exe"), "{missing}");
    assert_eq!(read(&root.join("2.0.0")), None);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A real release's archive (`EDITOR_RELEASE_ARCHIVE=<file>`) unpacked as an update is:
/// its app there, its program still executable. Run by hand.
#[test]
#[ignore = "a real release's archive, given by hand"]
fn a_real_release_archive_unpacks() {
    let Some(archive) = std::env::var_os("EDITOR_RELEASE_ARCHIVE").map(PathBuf::from) else {
        return;
    };
    let name = archive.file_name().unwrap().to_string_lossy().into_owned();
    let into = temp();
    let stop = AtomicBool::new(false);
    pinned_downloads::archive::unpack_path(&archive, &name, &into, &Rules::default(), None, &stop)
        .unwrap();
    let app = into.join(app_name());
    assert!(app.exists(), "{} in {}", app_name(), into.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bundled = app.join("Contents").join("MacOS").join(kernel::brand::ID);
        let program = if cfg!(target_os = "macos") { &bundled } else { &app };
        let mode = std::fs::metadata(program).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "{} executable: {mode:o}", program.display());
    }
    // Its signature still holds as unpacked (Apple Silicon runs nothing whose doesn't).
    #[cfg(target_os = "macos")]
    {
        let verified = std::process::Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&app)
            .status()
            .unwrap();
        assert!(verified.success(), "codesign --verify {}", app.display());
    }
    std::fs::remove_dir_all(&into).unwrap();
}

/// A note pointing outside its folder (a root, a drive, `..`) is no update; other versions'
/// downloads go when one is staged.
#[test]
fn a_note_points_inside_its_folder_and_old_downloads_go() {
    let root = temp();
    let dir = root.join("2.0.0");
    std::fs::create_dir_all(dir.join("app")).unwrap();
    std::fs::write(dir.join("app").join("app.exe"), b"x").unwrap();
    for bad in ["../evil", "/etc/app", "C:app", "app/../../x", ""] {
        std::fs::write(dir.join(STAGED), format!("version = \"2.0.0\"\napp = \"{bad}\"\n"))
            .unwrap();
        assert_eq!(read(&dir), None, "{bad:?}");
    }
    std::fs::create_dir_all(root.join("1.9.0")).unwrap();
    let bytes = zip(&[ZipEntry("app.exe", Some(0o755), b"new", true)]);
    let memory = Memory::default();
    memory.serve(URL, bytes.clone());
    let ok = available(&bytes, pinned_downloads::digest::sha256_hex(&bytes));
    stage(&memory, &ok, &root, "app.exe", &|_, _| {}, &AtomicBool::new(false)).unwrap();
    assert!(!root.join("1.9.0").exists(), "another version's download gone");
    std::fs::remove_dir_all(&root).unwrap();
}
