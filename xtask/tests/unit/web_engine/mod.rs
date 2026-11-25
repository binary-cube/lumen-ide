use super::*;

/// A folder of its own under the system's temporary one, removed when dropped (the packing's
/// tests use it too).
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(what: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{what}-{:032x}", kernel::id::new_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Only our languages and Chromium's fallback are kept; anything not a language stays.
#[test]
fn only_our_languages_are_kept() {
    for kept in ["en.lproj", "ro.lproj", "resources.pak", "icudtl.dat"] {
        assert!(kept_language(kept, Languages::Folders), "{kept}");
    }
    assert!(!kept_language("de.lproj", Languages::Folders));
    for kept in ["en-US.pak", "ro.pak"] {
        assert!(kept_language(kept, Languages::Files), "{kept}");
    }
    assert!(!kept_language("zh-CN.pak", Languages::Files));
    assert!(kept_language("de.lproj", Languages::None), "only where languages are");
}

/// A bundle's entries: refused while it holds languages but ours (it's trimmed before it's
/// signed); once trimmed, everything but the system's companions, `engine.json` added.
#[test]
fn a_bundles_entries_need_it_trimmed() {
    let scratch = Scratch::new("web-engine-entries");
    let bundle = &scratch.0;
    for file in ["web-host", "locales/ro.pak", "locales/de.pak", "._web-host", "Resources/x"] {
        let path = bundle.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    std::fs::create_dir_all(bundle.join("Resources/de.lproj")).unwrap();
    std::fs::create_dir_all(bundle.join("Resources/en.lproj")).unwrap();
    let refused = entries_of(bundle, b"{}").unwrap_err();
    assert!(refused.contains("web-engine-trim"), "{refused}");
    trim(bundle).unwrap();
    let mut paths: Vec<String> =
        entries_of(bundle, b"{}").unwrap().into_iter().map(|e| e.path).collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "Resources",
            "Resources/en.lproj",
            "Resources/x",
            "engine.json",
            "locales",
            "locales/ro.pak",
            "web-host"
        ]
    );
}

/// A link in a bundle refuses it: the app's unpacking writes none.
#[cfg(unix)]
#[test]
fn a_link_refuses_the_bundle() {
    let scratch = Scratch::new("web-engine-link");
    std::fs::write(scratch.0.join("real"), b"x").unwrap();
    std::os::unix::fs::symlink(scratch.0.join("real"), scratch.0.join("link")).unwrap();
    assert!(entries_of(&scratch.0, b"{}").unwrap_err().contains("a link"));
}

/// A key of the test's own: its public half and its secret.
fn keyed() -> (String, String) {
    let (secret, public) = crate::release::new_key().unwrap();
    (public, secret)
}

fn describe(dir: &Path, platform: &str, version: &str, protocol: u32) {
    let name = format!("web-engine-{platform}-{version}.tar.xz");
    let described = Description {
        version: version.into(),
        protocol,
        platform: platform.into(),
        name: name.clone(),
        size: 5,
        sha256: "ab".repeat(32),
    };
    std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string(&described).unwrap())
        .unwrap();
}

/// The new list merges the published one (its signature checked), keeps its minimum unless
/// one is given, is signed by the project's key; a description of another version than the
/// release's is refused.
#[test]
fn the_list_merges_the_published_one() {
    let scratch = Scratch::new("web-engine-manifest");
    let (public, secret) = keyed();
    let published = scratch.0.join("published");
    std::fs::create_dir_all(&published).unwrap();
    describe(&published, "linux-x64", "156.0.8078.100-1", 1);
    let base = "https://example.test/web-engine";
    let ask = |dir: &Path, version: Option<&'static str>, previous: Option<&Path>| {
        let previous = previous.map(Path::to_path_buf);
        let dir = dir.to_path_buf();
        let public = public.clone();
        let secret = secret.clone();
        move |minimum: Option<&str>| {
            manifest(&ManifestAsk {
                public: &public,
                dir: &dir,
                base_url: base,
                version,
                previous: previous.as_deref(),
                minimum,
                relist: false,
                secret: Some(&secret),
                signed: true,
            })
        }
    };
    ask(&published, None, None)(Some("156.0.8078.100-1")).unwrap();

    let draft = scratch.0.join("draft");
    std::fs::create_dir_all(&draft).unwrap();
    describe(&draft, "linux-x64", "157.0.1.1-1", 2);
    let previous = published.join(list::LIST);
    assert!(
        ask(&draft, Some("157.0.1.1-2"), Some(&previous))(None).unwrap_err().contains("version"),
        "another version than the release's"
    );
    assert!(
        ask(&draft, Some("157.0.1.1-1"), Some(&previous))(Some("157.0.1.1-2"))
            .unwrap_err()
            .contains("newer than the release"),
        "a minimum no package of the release would meet"
    );
    assert!(ask(&draft, Some("157.0.1.1-1"), Some(&previous))(Some("157")).is_err());
    ask(&draft, Some("157.0.1.1-1"), Some(&previous))(None).unwrap();
    let made = list::read_signed(&draft.join(list::LIST), &public).unwrap();
    assert_eq!(made.minimum.as_deref(), Some("156.0.8078.100-1"), "the published minimum kept");
    let kept: Vec<(u32, &str)> =
        made.packages.iter().map(|p| (p.protocol, p.version.as_str())).collect();
    assert_eq!(kept, [(1, "156.0.8078.100-1"), (2, "157.0.1.1-1")]);

    // A published list whose signature isn't the project's is refused.
    std::fs::write(published.join(list::SIGNATURE), "00\n").unwrap();
    assert!(ask(&draft, None, Some(&previous))(None).unwrap_err().contains("signature"));
}

/// Before publishing: the draft's list is signed, complete, its archives as it says; one that
/// would drop a package published since it was made is refused.
#[test]
fn a_draft_is_verified_before_it_is_published() {
    let scratch = Scratch::new("web-engine-verify");
    let (public, secret) = keyed();
    let version = "156.0.8078.100-2";
    let make = |dir: &Path, version: &str| {
        std::fs::create_dir_all(dir).unwrap();
        for platform in list::platforms() {
            let name = format!("web-engine-{platform}-{version}.tar.xz");
            let bytes = format!("{platform} {version}");
            std::fs::write(dir.join(&name), &bytes).unwrap();
            let described = Description {
                version: version.into(),
                protocol: 1,
                platform: platform.into(),
                name: name.clone(),
                size: bytes.len() as u64,
                sha256: pinned_downloads::digest::sha256_hex(bytes.as_bytes()),
            };
            let text = serde_json::to_string(&described).unwrap();
            std::fs::write(dir.join(format!("{name}.json")), text).unwrap();
        }
        manifest(&ManifestAsk {
            public: &public,
            dir,
            base_url: "https://example.test/web-engine",
            version: Some(version),
            previous: None,
            minimum: None,
            relist: false,
            secret: Some(&secret),
            signed: true,
        })
        .unwrap();
    };
    let draft = scratch.0.join("draft");
    make(&draft, version);
    assert!(verify(&public, &draft, version, None).is_ok());
    assert!(verify(&public, &draft, "156.0.8078.100-3", None).is_err(), "not this version's");

    let newer = scratch.0.join("newer");
    make(&newer, "156.0.8078.100-3");
    let published = newer.join(list::LIST);
    let refused = verify(&public, &draft, version, Some(&published)).unwrap_err();
    assert!(refused.contains("make the draft again"), "{refused}");
    assert!(verify(&public, &newer, "156.0.8078.100-3", Some(&draft.join(list::LIST))).is_ok());

    std::fs::write(draft.join(list::LIST), "{\"packages\":[]}\n").unwrap();
    assert!(verify(&public, &draft, version, None).unwrap_err().contains("signature"));
}

/// Re-listing changes only the published list's minimum: its packages kept as they are, no
/// description read, signed anew; a minimum some system has no package for is refused, and
/// so is a re-list without the published list or a minimum.
#[test]
fn a_relist_only_changes_the_minimum() {
    let scratch = Scratch::new("web-engine-relist");
    let (public, secret) = keyed();
    let published = scratch.0.join("published");
    std::fs::create_dir_all(&published).unwrap();
    for platform in list::platforms() {
        describe(&published, platform, "156.0.8078.100-2", 1);
    }
    describe(&published, "linux-x64", "156.0.8078.100-1", 0);
    let ask = |dir: &Path, previous: Option<&Path>, minimum: Option<&str>| {
        manifest(&ManifestAsk {
            public: &public,
            dir,
            base_url: "https://example.test/web-engine",
            version: None,
            previous,
            minimum,
            relist: true,
            secret: Some(&secret),
            signed: true,
        })
    };
    let first = manifest(&ManifestAsk {
        public: &public,
        dir: &published,
        base_url: "https://example.test/web-engine",
        version: None,
        previous: None,
        minimum: None,
        relist: false,
        secret: Some(&secret),
        signed: true,
    });
    first.unwrap();
    let before = list::read_signed(&published.join(list::LIST), &public).unwrap();

    let relisted = scratch.0.join("relisted");
    std::fs::create_dir_all(&relisted).unwrap();
    let previous = published.join(list::LIST);
    assert!(ask(&relisted, None, Some("156.0.8078.100-2")).is_err(), "no published list");
    assert!(ask(&relisted, Some(&previous), None).is_err(), "no minimum");
    let refused = ask(&relisted, Some(&previous), Some("157.0.0.1-1")).unwrap_err();
    assert!(refused.contains("meets the minimum"), "{refused}");
    ask(&relisted, Some(&previous), Some("156.0.8078.100-2")).unwrap();
    let after = list::read_signed(&relisted.join(list::LIST), &public).unwrap();
    assert_eq!(after.minimum.as_deref(), Some("156.0.8078.100-2"));
    assert_eq!(after.packages, before.packages, "the packages as published");
}

/// The bundle's languages but ours are removed before it's signed; anything else stays,
/// a language's name outside the languages' folders too.
#[test]
fn trimming_removes_only_other_languages() {
    let scratch = Scratch::new("web-engine-trim");
    let bundle = &scratch.0;
    let framework = "web-host.app/Contents/Frameworks/CEF.framework/Resources";
    for file in [
        "locales/ro.pak".to_owned(),
        "locales/de.pak".to_owned(),
        "de.pak".to_owned(),
        format!("{framework}/de.lproj/locale.pak"),
        format!("{framework}/en.lproj/locale.pak"),
        format!("{framework}/icudtl.dat"),
    ] {
        let path = bundle.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    assert!(trim(bundle).unwrap().starts_with("2 languages removed"));
    assert!(!bundle.join("locales/de.pak").exists());
    assert!(!bundle.join(framework).join("de.lproj").exists());
    for kept in ["locales/ro.pak", "de.pak"] {
        assert!(bundle.join(kept).exists(), "{kept}");
    }
    for kept in ["en.lproj/locale.pak", "icudtl.dat"] {
        assert!(bundle.join(framework).join(kept).exists(), "{kept}");
    }
}

/// What `--version` said is taken only from a run that ended well, as the engine's typed
/// description; a failed run is refused with its error output.
#[cfg(unix)]
#[test]
fn the_engine_is_read_from_a_run_that_ended_well() {
    use std::os::unix::process::ExitStatusExt as _;
    let program = Path::new("web-host");
    let run = |code: i32, stdout: &str, stderr: &str| std::process::Output {
        status: std::process::ExitStatus::from_raw(code << 8),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    };
    let said = r#"{"version":"156.0.8078.100-1","protocol":2}"#;
    let (text, engine) = engine_from(program, &run(0, &format!("{said}\n"), "")).unwrap();
    assert_eq!(text, said.as_bytes());
    assert_eq!(engine, EngineInfo { version: "156.0.8078.100-1".into(), protocol: 2 });

    let failed = engine_from(program, &run(127, "", "libcef.so: cannot open")).unwrap_err();
    assert!(failed.contains("libcef.so: cannot open"), "{failed}");
    assert!(engine_from(program, &run(0, r#"{"version":"1"}"#, "")).is_err(), "no protocol");
    let odd = r#"{"version":"156","protocol":2}"#;
    assert!(engine_from(program, &run(0, odd, "")).is_err(), "not an engine's version");
}
