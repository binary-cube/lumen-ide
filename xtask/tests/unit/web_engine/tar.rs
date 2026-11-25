use std::sync::atomic::AtomicBool;

use super::*;
use crate::web_engine::tests::Scratch;

/// A path longer than a ustar header's name field.
fn long_path() -> String {
    format!("app/Contents/Frameworks/{}/Resources/deep.pak", "Some Framework".repeat(6))
}

/// A small bundle: a program, a library, a long path and bytes made in memory.
fn sample(dir: &Path) -> Vec<Entry> {
    let program = dir.join("program");
    std::fs::write(&program, b"#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let library = dir.join("library.bin");
    std::fs::write(&library, vec![7u8; 1500]).unwrap();
    let file =
        |name: &Path| Body::File { source: name.to_path_buf(), exec: executable(name).unwrap() };
    vec![
        Entry { path: long_path(), body: file(&library) },
        Entry { path: "program".into(), body: file(&program) },
        Entry { path: "app".into(), body: Body::Folder },
        Entry { path: "engine.json".into(), body: Body::Bytes(b"{\"version\":\"1\"}".to_vec()) },
        Entry { path: "app/library.bin".into(), body: file(&library) },
    ]
}

/// A tar header's octal field.
fn octal_at(header: &[u8], from: usize, len: usize) -> u64 {
    let text = String::from_utf8_lossy(&header[from..from + len]);
    u64::from_str_radix(text.trim_matches(|c: char| c == '\0' || c == ' '), 8).unwrap()
}

/// The archive's entries as written: (name, kind, mode, uid, gid, mtime), long names read.
fn headers(archive: &Path) -> Vec<(String, u8, u64, u64, u64, u64)> {
    let mut tarball = Vec::new();
    lzma_rust2::XzReader::new(std::fs::File::open(archive).unwrap(), true)
        .read_to_end(&mut tarball)
        .unwrap();
    let mut out = Vec::new();
    let mut at = 0;
    let mut long: Option<String> = None;
    while at + BLOCK <= tarball.len() && tarball[at..at + BLOCK].iter().any(|b| *b != 0) {
        let header = &tarball[at..at + BLOCK];
        let size = octal_at(header, 124, 12) as usize;
        let body = &tarball[at + BLOCK..at + BLOCK + size];
        let kind = header[156];
        if kind == b'L' {
            long = Some(String::from_utf8_lossy(&body[..size - 1]).into_owned());
        } else {
            let end = header[..100].iter().position(|b| *b == 0).unwrap_or(100);
            let name =
                long.take().unwrap_or_else(|| String::from_utf8_lossy(&header[..end]).into_owned());
            out.push((
                name,
                kind,
                octal_at(header, 100, 8),
                octal_at(header, 108, 8),
                octal_at(header, 116, 8),
                octal_at(header, 136, 12),
            ));
        }
        at += BLOCK + size.div_ceil(BLOCK) * BLOCK;
    }
    out
}

/// Sorted by path, owned by 0:0 at the epoch, folders and executables 0755, the rest 0644,
/// a long path whole.
#[test]
fn the_archive_is_sorted_and_bare() {
    let scratch = Scratch::new("web-engine-tar");
    let mut entries = sample(&scratch.0);
    let archive = scratch.0.join("out.tar.xz");
    pack(&mut entries, &archive).unwrap();
    let written = headers(&archive);
    let names: Vec<&str> = written.iter().map(|h| h.0.as_str()).collect();
    let long = long_path();
    assert_eq!(names, ["app/", long.as_str(), "app/library.bin", "engine.json", "program"]);
    for (name, kind, mode, uid, gid, mtime) in &written {
        assert_eq!((*uid, *gid, *mtime), (0, 0, 0), "{name}");
        let wanted = match (*kind, name.as_str()) {
            (b'5', _) => 0o755,
            (_, "program") if cfg!(unix) => 0o755,
            _ => 0o644,
        };
        assert_eq!(*mode, wanted, "{name}");
    }
}

/// The same files make the same bytes.
#[test]
fn the_archive_is_the_same_each_time() {
    let scratch = Scratch::new("web-engine-tar-same");
    let first = pack(&mut sample(&scratch.0), &scratch.0.join("a.tar.xz")).unwrap();
    let mut reversed = sample(&scratch.0);
    reversed.reverse();
    let second = pack(&mut reversed, &scratch.0.join("b.tar.xz")).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.size, std::fs::metadata(scratch.0.join("a.tar.xz")).unwrap().len());
}

/// The app's own unpacking reads it back whole: every file, its bytes, its executable bit.
#[test]
fn the_apps_unpacking_reads_it_back() {
    let scratch = Scratch::new("web-engine-tar-unpack");
    let archive = scratch.0.join("out.tar.xz");
    pack(&mut sample(&scratch.0), &archive).unwrap();
    let into = scratch.0.join("unpacked");
    let written = pinned_downloads::archive::unpack_path(
        &archive,
        "out.tar.xz",
        &into,
        &pinned_downloads::archive::Rules::default(),
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(written.files, 4);
    assert_eq!(std::fs::read(into.join(long_path())).unwrap(), vec![7u8; 1500]);
    assert_eq!(std::fs::read(into.join("app/library.bin")).unwrap(), vec![7u8; 1500]);
    assert_eq!(std::fs::read(into.join("engine.json")).unwrap(), b"{\"version\":\"1\"}");
    assert_eq!(std::fs::read(into.join("program")).unwrap(), b"#!/bin/sh\n");
    assert!(executable(&into.join("program")).unwrap() == cfg!(unix));
    assert!(!executable(&into.join("engine.json")).unwrap());
}

/// The system's own companions are never packed.
#[test]
fn the_systems_files_are_left_out() {
    assert!(system_file("._web-host"));
    assert!(system_file(".DS_Store"));
    assert!(!system_file("web-host"));
    assert!(!system_file(".hidden"));
}
