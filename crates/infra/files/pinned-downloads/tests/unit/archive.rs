use std::sync::atomic::AtomicBool;

use super::*;
use crate::samples::{Entry, ZipEntry, gz, tar, temp, xz, zip};

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
}

#[test]
fn formats_are_told_by_name() {
    assert_eq!(Format::of("node-v1-darwin-arm64.tar.xz"), Format::TarXz);
    assert_eq!(Format::of("pkg.tgz"), Format::TarGz);
    assert_eq!(Format::of("php-cli.tar.gz"), Format::TarGz);
    assert_eq!(Format::of("clangd.zip"), Format::Zip);
    assert_eq!(Format::of("taplo-darwin-aarch64.gz"), Format::Gz);
    assert_eq!(Format::of("phpactor.phar"), Format::File);
}

#[test]
fn a_tarball_keeps_its_files_folders_and_executables_but_no_links() {
    let long = format!("top/{}/deep.txt", "d".repeat(120));
    let bytes = tar(&[
        Entry("top/", b'5', 0o755, b""),
        Entry("top/bin/node", b'0', 0o755, b"#!node"),
        Entry("top/LICENSE", b'0', 0o644, b"MIT"),
        Entry(&long, b'0', 0o644, b"long"),
        Entry("top/bin/npm", b'2', 0o777, b""),
        Entry("top/lib/x.js", b'0', 0o644, b"x"),
    ]);
    for (name, archive) in [("t.tar.gz", gz(&bytes)), ("t.tar.xz", xz(&bytes))] {
        let dir = temp(name);
        let file = dir.join(name);
        std::fs::write(&file, &archive).unwrap();
        let into = dir.join("out");
        let rules = Rules { strip: Strip::Folder("top"), ..Rules::default() };
        let written = unpack_path(&file, name, &into, &rules, None, &never()).unwrap();
        assert_eq!(written.files, 4, "{name}");
        assert_eq!(std::fs::read(into.join("bin/node")).unwrap(), b"#!node");
        assert!(into.join(long.trim_start_matches("top/")).is_file());
        assert!(!into.join("bin/npm").exists(), "links are never made");
        #[cfg(unix)]
        {
            assert!(executable(&into.join("bin/node")));
            assert!(!executable(&into.join("LICENSE")));
        }
        // Only what's kept, when it's said.
        let kept = dir.join("kept");
        let keep = vec!["bin/node".to_owned(), "LICENSE".to_owned()];
        let rules = Rules { strip: Strip::Folder("top"), keep: &keep, ..Rules::default() };
        unpack_path(&file, name, &kept, &rules, None, &never()).unwrap();
        assert!(kept.join("bin/node").is_file() && kept.join("LICENSE").is_file());
        assert!(!kept.join("lib").exists());
    }
}

#[test]
fn what_is_pruned_and_where_a_package_goes() {
    let bytes = gz(&tar(&[
        Entry("package/package.json", b'0', 0o644, b"{}"),
        Entry("package/lib/tsc.js", b'0', 0o644, b"big"),
        Entry("package/lib/keep.js", b'0', 0o644, b"ok"),
        Entry("package/test/a.js", b'0', 0o644, b"t"),
    ]));
    let into = temp("pruned");
    let prune = vec!["node_modules/ts/lib/tsc.js".to_owned(), "node_modules/ts/test".to_owned()];
    let rules =
        Rules { strip: Strip::First, prune: &prune, prefix: "node_modules/ts", ..Rules::default() };
    unpack_tar(flate2::read::GzDecoder::new(&bytes[..]), &into, &rules, &never()).unwrap();
    assert!(into.join("node_modules/ts/package.json").is_file());
    assert!(into.join("node_modules/ts/lib/keep.js").is_file());
    assert!(!into.join("node_modules/ts/lib/tsc.js").exists());
    assert!(!into.join("node_modules/ts/test").exists());
}

/// The caller's filter leaves out what it refuses, by the path in the folder.
#[test]
fn a_filter_leaves_out_what_it_refuses() {
    let bytes = zip(&[
        ZipEntry("index.html", Some(0o644), b"<p>", true),
        ZipEntry("WEB-INF/web.xml", Some(0o644), b"<x/>", false),
        ZipEntry("js/app.js", Some(0o644), b"app", true),
        ZipEntry("js/big.js", Some(0o644), b"big", true),
    ]);
    let into = temp("filtered");
    let rules = Rules {
        filter: Some(|path| !path.starts_with("WEB-INF/") && path != "js/big.js"),
        ..Rules::default()
    };
    let written = unpack_zip(&mut std::io::Cursor::new(bytes), &into, &rules, &never()).unwrap();
    assert_eq!(written.files, 2);
    assert!(into.join("index.html").is_file());
    assert!(into.join("js/app.js").is_file());
    assert!(!into.join("WEB-INF").exists());
    assert!(!into.join("js/big.js").exists());
    assert!(pruned_any(&["js/*".to_owned()], "js/app.js"));
    assert!(!pruned("js", "js/app.js"));
}

#[test]
fn an_entry_leaving_its_folder_refuses_the_archive() {
    for name in ["package/../../evil.txt", "/etc/evil", "C:/evil.txt", "package/..\\..\\evil"] {
        let bytes = gz(&tar(&[Entry(name, b'0', 0o644, b"x")]));
        let into = temp("escape");
        let rules = Rules { strip: Strip::First, ..Rules::default() };
        assert!(
            unpack_tar(
                flate2::read::GzDecoder::new(&bytes[..]),
                &into.join("out"),
                &rules,
                &never()
            )
            .is_err(),
            "{name}"
        );
        assert!(!into.join("evil.txt").exists());
    }
    let archive = zip(&[ZipEntry("../evil.txt", Some(0o644), b"x", false)]);
    let into = temp("zip-escape");
    assert!(
        unpack_zip(
            &mut std::io::Cursor::new(archive),
            &into.join("out"),
            &Rules::default(),
            &never()
        )
        .is_err()
    );
    assert!(!into.join("evil.txt").exists());
}

#[test]
fn a_zip_is_unpacked_stored_and_deflated_without_its_links() {
    let big = "abc".repeat(5000);
    let archive = zip(&[
        ZipEntry("clangd_23/", Some(0o40755), b"", false),
        ZipEntry("clangd_23/bin/clangd", Some(0o100755), b"\x7fELF", false),
        ZipEntry("clangd_23/lib/clang/23/include/stddef.h", Some(0o100644), big.as_bytes(), true),
        ZipEntry("clangd_23/lib/clang/23/lib/rt.a", Some(0o100644), b"rt", true),
        ZipEntry("clangd_23/bin/link", Some(0o120777), b"clangd", false),
        ZipEntry("clangd_23/node.exe", None, b"MZ", true),
    ]);
    let into = temp("zip");
    let prune = vec!["lib/clang/*/lib".to_owned()];
    let rules = Rules { strip: Strip::Folder("clangd_23"), prune: &prune, ..Rules::default() };
    let written = unpack_zip(&mut std::io::Cursor::new(archive), &into, &rules, &never()).unwrap();
    assert_eq!(written.files, 3);
    assert_eq!(std::fs::read_to_string(into.join("lib/clang/23/include/stddef.h")).unwrap(), big);
    assert!(!into.join("lib/clang/23/lib").exists());
    assert!(!into.join("bin/link").exists());
    assert_eq!(std::fs::read(into.join("node.exe")).unwrap(), b"MZ");
    #[cfg(unix)]
    assert!(executable(&into.join("bin/clangd")));
}

#[test]
fn one_gzipped_program_is_named_and_executable() {
    let dir = temp("gz");
    let file = dir.join("taplo-darwin-aarch64.gz");
    std::fs::write(&file, gz(b"\xcf\xfa\xed\xfe")).unwrap();
    let into = dir.join("out");
    unpack_path(
        &file,
        "taplo-darwin-aarch64.gz",
        &into,
        &Rules::default(),
        Some("taplo"),
        &never(),
    )
    .unwrap();
    assert_eq!(std::fs::read(into.join("taplo")).unwrap(), b"\xcf\xfa\xed\xfe");
    #[cfg(unix)]
    assert!(executable(&into.join("taplo")));
    assert!(
        unpack_path(&file, "x.gz", &into, &Rules::default(), Some("../taplo"), &never()).is_err()
    );
}

#[test]
fn a_broken_archive_is_an_error() {
    let into = temp("broken");
    assert!(
        unpack_tar(
            flate2::read::GzDecoder::new(&b"not gzip"[..]),
            &into,
            &Rules::default(),
            &never()
        )
        .is_err()
    );
    assert!(
        unpack_zip(
            &mut std::io::Cursor::new(b"not a zip".to_vec()),
            &into,
            &Rules::default(),
            &never()
        )
        .is_err()
    );
    // A tarball cut in the middle of a body.
    let mut cut = tar(&[Entry("a/b.txt", b'0', 0o644, &[1u8; 2000])]);
    cut.truncate(1200);
    assert!(unpack_tar(&cut[..], &into, &Rules::default(), &never()).is_err());
}
