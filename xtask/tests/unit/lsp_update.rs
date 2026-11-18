//! The version picking of `lsp-update`, against made-up listings (never the network).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;

use super::*;

/// A network of made-up answers: texts by URL; a file's SHA-256 is `sum-<its name>`, its
/// size 42. What was hashed is remembered.
#[derive(Default)]
struct Fake {
    texts: BTreeMap<String, String>,
    hashed: std::sync::Mutex<Vec<String>>,
}

impl Fake {
    fn with(mut self, url: &str, text: impl Into<String>) -> Self {
        self.texts.insert(url.into(), text.into());
        self
    }
}

impl Net for Fake {
    fn text(&self, url: &str) -> Result<String, String> {
        self.texts.get(url).cloned().ok_or_else(|| format!("{url}: not found"))
    }

    fn sha256(&self, url: &str) -> Result<String, String> {
        self.hashed.lock().unwrap().push(url.into());
        Ok(format!("sum-{}", url.rsplit('/').next().unwrap()))
    }

    fn size(&self, _url: &str) -> Result<u64, String> {
        Ok(42)
    }
}

fn versions(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn versions_are_numbers_without_pre_releases() {
    assert_eq!(parse("v26.13.0"), Some(vec![26, 13, 0]));
    assert_eq!(parse("2026.10.07.0"), Some(vec![2026, 10, 7, 0]));
    assert_eq!(parse("5.0.0-beta.1"), None);
    assert_eq!(parse("1.2.3+build"), None);
    assert_eq!(parse("latest"), None);
}

#[test]
fn a_major_update_is_the_first_number_or_below_one_the_second() {
    assert!(!is_major("23.1.0", "23.2.5"));
    assert!(is_major("23.1.0", "24.0.0"));
    assert!(!is_major("0.17.0", "0.17.3"));
    assert!(is_major("0.17.0", "0.18.0"));
    assert!(is_major("0.17.0", "1.0.0"));
    // A calendar version (phpactor's) never is.
    assert!(!is_major("2026.10.07.0", "2027.01.12.0"));
}

#[test]
fn the_newest_allowed_is_picked_and_a_major_one_held_back() {
    let offered = versions(&["5.9.2", "5.9.3", "5.9.4", "5.10.1", "6.0.2", "6.1.0-rc"]);
    assert_eq!(pick("5.9.3", &offered, false), (Some("5.10.1".into()), Some("6.0.2".into())));
    assert_eq!(pick("5.9.3", &offered, true), (Some("6.0.2".into()), None));
    assert_eq!(pick("6.0.2", &offered, false), (None, None));
    // Only a major one newer: nothing taken, it's said.
    assert_eq!(pick("5.10.1", &versions(&["6.0.2"]), false), (None, Some("6.0.2".into())));
}

#[test]
fn only_nodes_lts_releases_count() {
    let index = json!([
        {"version": "v27.1.0", "lts": false},
        {"version": "v26.14.0", "lts": "Krypton"},
        {"version": "v26.13.0", "lts": "Krypton"},
        {"version": "v24.20.0", "lts": "Jod"},
    ]);
    let lts = node_versions(&index);
    assert_eq!(lts, versions(&["26.14.0", "26.13.0", "24.20.0"]));
    assert_eq!(pick("26.13.0", &lts, false), (Some("26.14.0".into()), None));
    // A newer LTS line only with --major.
    let index = json!([{"version": "v28.0.1", "lts": "Lithium"}, {"version": "v26.13.0", "lts": "Krypton"}]);
    assert_eq!(pick("26.13.0", &node_versions(&index), false), (None, Some("28.0.1".into())));
}

#[test]
fn a_packages_versions_stop_at_its_latest_tag() {
    let document = json!({
        "dist-tags": {"latest": "2.1.0", "next": "3.0.0-beta.2"},
        "versions": {
            "2.0.0": {}, "2.1.0": {}, "2.2.0": {}, "2.0.5": {"deprecated": "broken"},
            "3.0.0-beta.2": {},
        }
    });
    let mut found = npm_versions(&document);
    found.sort();
    assert_eq!(found, versions(&["2.0.0", "2.1.0"]));
}

#[test]
fn githubs_drafts_and_pre_releases_are_left_out() {
    let list = json!([
        {"tag_name": "24.0.0-rc1", "prerelease": true, "assets": []},
        {"tag_name": "v23.2.0", "draft": true, "assets": []},
        {"tag_name": "23.1.4", "assets": [
            {"name": "clangd-mac-23.1.4.zip", "digest": "sha256:abc"},
            {"name": "clangd-linux-23.1.4.zip"},
        ]},
    ]);
    let releases = github_releases(&list);
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].version, "23.1.4");
    assert_eq!(releases[0].assets["clangd-mac-23.1.4.zip"].as_deref(), Some("abc"));
    assert_eq!(releases[0].assets["clangd-linux-23.1.4.zip"], None);
}

#[test]
fn static_php_needs_every_platforms_file() {
    let minimal = "https://dl.static-php.dev/static-php-cli/minimal/".to_owned();
    let windows = "https://dl.static-php.dev/static-php-cli/windows/spc-min/".to_owned();
    let listing = r#"[{"name":"php-8.4.22-cli-macos-aarch64.tar.gz"},{"name":"php-8.4.23-cli-macos-aarch64.tar.gz"},{"name":"php-8.4.24-cli-macos-aarch64.tar.gz"},{"name":"php-8.4.23-cli-linux-x86_64.tar.gz"},{"name":"php-8.4.24-cli-linux-x86_64.tar.gz"},{"name":"README.txt"}]"#;
    let mut names = BTreeMap::new();
    names.insert(minimal.clone(), listing_names(listing));
    names.insert(
        windows.clone(),
        listing_names("<a href=\"php-8.4.23-cli-win.zip\">php-8.4.23-cli-win.zip</a>"),
    );
    let files = vec![
        (minimal.clone(), "php-8.4.22-cli-macos-aarch64.tar.gz".to_owned()),
        (minimal, "php-8.4.22-cli-linux-x86_64.tar.gz".to_owned()),
        (windows, "php-8.4.22-cli-win.zip".to_owned()),
    ];
    // 8.4.24 has no Windows build yet: 8.4.23 is the newest of every platform.
    let found = php_versions(&files, "8.4.22", &names);
    assert_eq!(pick_by("8.4.22", &found, false, is_major_line), (Some("8.4.23".into()), None));
    // A new minor line of PHP only with --major.
    let found = versions(&["8.4.23", "8.5.7"]);
    assert_eq!(
        pick_by("8.4.22", &found, false, is_major_line),
        (Some("8.4.23".into()), Some("8.5.7".into()))
    );
}

const PACKAGING: &str = r#"# Node, the LTS.
[node]
version = "26.13.0"
license = "MIT"

[node.platforms.macos-arm64]
url = "https://nodejs.org/dist/v26.13.0/node-v26.13.0-darwin-arm64.tar.xz"
sha256 = "old-node"

# The C / C++ server (23.1.0 said in a comment stays).
[[servers]]
name = "clangd"
version = "23.1.0"
strip = "clangd_23.1.0"

[servers.platforms.macos-arm64]
url = "https://github.com/clangd/clangd/releases/download/23.1.0/clangd-mac-23.1.0.zip"
sha256 = "old-mac"

[servers.platforms.linux-x64]
url = "https://github.com/clangd/clangd/releases/download/23.1.0/clangd-linux-23.1.0.zip"
sha256 = "old-linux"

[[servers]]
name = "vtsls"
version = "0.3.0"
packages = "packaging/lsp/vtsls"

[[servers]]
name = "phpactor"
version = "2026.10.07.0"

[[servers.files]]
url = "https://github.com/phpactor/phpactor/releases/download/2026.10.07.0/phpactor.phar"
sha256 = "old-phar"
path = "phpactor.phar"

[[servers.files]]
url = "https://raw.githubusercontent.com/phpactor/phpactor/2026.10.07.0/LICENSE"
sha256 = "old-licence"
path = "LICENSE"
"#;

#[test]
fn the_components_blocks_end_before_the_next_ones_comments() {
    let found = blocks(PACKAGING);
    let names: Vec<&str> = found.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["node", "clangd", "vtsls", "phpactor"]);
    let node = &PACKAGING[found[0].range.clone()];
    assert!(node.starts_with("[node]") && !node.contains("C / C++"), "{node}");
    let clangd = &PACKAGING[found[1].range.clone()];
    assert_eq!(pinned_urls(clangd).len(), 2);
    assert!(pinned_urls(clangd).iter().all(|(_, line)| line.is_some()));
    // The comments keep their versions; every other line changes.
    let moved = retarget(&PACKAGING[..found[2].range.start], "23.1.0", "23.2.0");
    assert!(moved.contains("(23.1.0 said in a comment stays)"));
    assert!(moved.contains("strip = \"clangd_23.2.0\""));
}

fn clangd_releases() -> String {
    json!([{"tag_name": "23.2.0", "assets": [
        {"name": "clangd-mac-23.2.0.zip", "digest": "sha256:new-mac"},
        {"name": "clangd-linux-23.2.0.zip"},
    ]}, {"tag_name": "24.0.0", "assets": [
        {"name": "clangd-mac-24.0.0.zip"}, {"name": "clangd-linux-24.0.0.zip"},
    ]}])
    .to_string()
}

#[test]
fn a_native_servers_new_release_gets_its_urls_and_checksums() {
    let net = Fake::default()
        .with("https://api.github.com/repos/clangd/clangd/releases?per_page=30", clangd_releases());
    let b = &blocks(PACKAGING)[1];
    let (text, found, _) =
        update_block(Path::new("."), "clangd", &PACKAGING[b.range.clone()], &net, false, false)
            .unwrap();
    let text = text.unwrap();
    assert_eq!(found.update.as_deref(), Some("23.2.0"));
    assert_eq!(found.held.as_deref(), Some("24.0.0"));
    assert!(
        text.contains("download/23.2.0/clangd-mac-23.2.0.zip\"\nsha256 = \"new-mac\""),
        "{text}"
    );
    // Without GitHub's digest the file is downloaded and hashed.
    assert!(text.contains("clangd-linux-23.2.0.zip\"\nsha256 = \"sum-clangd-linux-23.2.0.zip\""));
    assert_eq!(net.hashed.lock().unwrap().len(), 1);
}

#[test]
fn a_release_missing_a_pinned_file_isnt_taken() {
    let list = json!([{"tag_name": "23.2.0", "assets": [{"name": "clangd-mac-23.2.0.zip"}]}]);
    let net = Fake::default()
        .with("https://api.github.com/repos/clangd/clangd/releases?per_page=30", list.to_string());
    let b = &blocks(PACKAGING)[1];
    let (text, found, _) =
        update_block(Path::new("."), "clangd", &PACKAGING[b.range.clone()], &net, false, false)
            .unwrap();
    assert!(text.is_none() && found.update.is_none());
}

#[test]
fn node_takes_its_checksums_from_the_release() {
    let index =
        json!([{"version": "v26.14.0", "lts": "Krypton"}, {"version": "v27.0.0", "lts": false}]);
    let net = Fake::default().with("https://nodejs.org/dist/index.json", index.to_string()).with(
        "https://nodejs.org/dist/v26.14.0/SHASUMS256.txt",
        "aaa  node-v26.14.0-darwin-arm64.tar.xz\nbbb  node-v26.14.0-linux-x64.tar.xz\n",
    );
    let b = &blocks(PACKAGING)[0];
    let (text, found, _) =
        update_block(Path::new("."), "node", &PACKAGING[b.range.clone()], &net, false, false)
            .unwrap();
    let text = text.unwrap();
    assert_eq!(found.update.as_deref(), Some("26.14.0"));
    assert!(text.contains("version = \"26.14.0\""));
    assert!(text.contains("node-v26.14.0-darwin-arm64.tar.xz\"\nsha256 = \"aaa\""), "{text}");
    assert!(net.hashed.lock().unwrap().is_empty());
}

#[test]
fn phpactors_calendar_releases_are_taken_with_their_licence_rehashed() {
    let list = json!([{"tag_name": "2026.11.02.0", "assets": [{"name": "phpactor.phar", "digest": "sha256:new-phar"}]}]);
    let net = Fake::default().with(
        "https://api.github.com/repos/phpactor/phpactor/releases?per_page=30",
        list.to_string(),
    );
    let b = &blocks(PACKAGING)[3];
    let (text, _, _) =
        update_block(Path::new("."), "phpactor", &PACKAGING[b.range.clone()], &net, false, false)
            .unwrap();
    let text = text.unwrap();
    assert!(text.contains("download/2026.11.02.0/phpactor.phar\"\nsha256 = \"new-phar\""));
    assert!(text.contains("phpactor/2026.11.02.0/LICENSE\"\nsha256 = \"sum-LICENSE\""), "{text}");
}

/// A temporary project root with the packaging, a Node server's packages and a language's
/// description of a server the user installs.
fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lsp-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("packaging/lsp/vtsls")).unwrap();
    std::fs::create_dir_all(dir.join("crates/modules/languages/php/data")).unwrap();
    std::fs::write(dir.join("packaging/lsp.toml"), PACKAGING).unwrap();
    std::fs::write(
        dir.join("packaging/lsp/vtsls/package.json"),
        "{\n  \"dependencies\": {\n    \"@vtsls/language-server\": \"0.3.0\",\n    \"typescript\": \"5.9.3\"\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("crates/modules/languages/php/data/language-servers.toml"),
        "[[language-servers]]\nname = \"intelephense\"\n\n[language-servers.install]\nnpm = \"intelephense\"\nversion = \"1.18.5\"\nintegrity = \"sha512-old\"\nsize = 1\n\n[language-servers.settings]\n",
    )
    .unwrap();
    dir
}

fn registry() -> Fake {
    let intelephense = json!({"dist-tags": {"latest": "1.19.0"}, "versions": {
        "1.18.5": {}, "1.19.0": {"dist": {"integrity": "sha512-new", "tarball": "https://registry.npmjs.org/intelephense/-/intelephense-1.19.0.tgz"}},
    }});
    Fake::default()
        .with("https://nodejs.org/dist/index.json", "[]")
        .with("https://api.github.com/repos/clangd/clangd/releases?per_page=30", clangd_releases())
        .with("https://api.github.com/repos/phpactor/phpactor/releases?per_page=30", "[]")
        .with(
            "https://registry.npmjs.org/@vtsls/language-server",
            json!({"dist-tags": {"latest": "0.4.1"}, "versions": {"0.3.0": {}, "0.3.2": {}, "0.4.1": {}}}).to_string(),
        )
        .with(
            "https://registry.npmjs.org/typescript",
            json!({"dist-tags": {"latest": "6.0.2"}, "versions": {"5.9.3": {}, "5.9.4": {}, "6.0.2": {}}}).to_string(),
        )
        .with("https://registry.npmjs.org/intelephense", intelephense.to_string())
}

#[test]
fn an_installable_servers_pin_follows_the_registry() {
    let text = std::fs::read_to_string(
        root("installs").join("crates/modules/languages/php/data/language-servers.toml"),
    )
    .unwrap();
    let (new, found) = update_installs(&text, &registry(), false, false).unwrap();
    let new = new.unwrap();
    assert_eq!(found[0].update.as_deref(), Some("1.19.0"));
    assert!(new.contains("version = \"1.19.0\"\nintegrity = \"sha512-new\"\nsize = 42\n"), "{new}");
    assert!(new.ends_with("[language-servers.settings]\n"));
}

#[test]
fn a_node_servers_packages_each_follow_the_semver_policy() {
    let dir = root("packages");
    let b = &blocks(PACKAGING)[2];
    let (text, found, packages) =
        update_block(&dir, "vtsls", &PACKAGING[b.range.clone()], &registry(), false, false)
            .unwrap();
    // vtsls 0.3 → 0.4 is a major update below 1.0: 0.3.2; TypeScript 5.9.4, not 6.
    assert_eq!(found.update.as_deref(), Some("0.3.2"));
    assert_eq!(found.held.as_deref(), Some("0.4.1"));
    assert!(text.unwrap().contains("version = \"0.3.2\""));
    assert_eq!(packages["typescript"], "5.9.4");
    let manifest = std::fs::read_to_string(dir.join("packaging/lsp/vtsls/package.json")).unwrap();
    let pinned = pin_packages(&manifest, &packages);
    assert!(pinned.contains("\"@vtsls/language-server\": \"0.3.2\""));
    assert!(pinned.contains("\"typescript\": \"5.9.4\""));
}

#[test]
fn the_check_says_what_would_change_and_writes_nothing() {
    let dir = root("check");
    let net = registry();
    let report = update_with(&dir, &net, true, false).unwrap();
    assert!(
        report.contains("clangd: 23.1.0 → 23.2.0, 24.0.0 is a major update: --major"),
        "{report}"
    );
    assert!(report.contains("node: 26.13.0 (up to date)"), "{report}");
    assert!(report.contains("intelephense: 1.18.5 → 1.19.0 (installed by the user)"), "{report}");
    assert!(report.contains("updates available"), "{report}");
    // Nothing downloaded to hash.
    assert!(net.hashed.lock().unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(dir.join("packaging/lsp.toml")).unwrap(), PACKAGING);
    let description = std::fs::read_to_string(
        dir.join("crates/modules/languages/php/data/language-servers.toml"),
    )
    .unwrap();
    assert!(description.contains("1.18.5"));
}

#[test]
fn a_component_that_cant_be_looked_up_is_said_and_the_others_go_on() {
    let dir = root("failing");
    let mut net = registry();
    net.texts.remove("https://nodejs.org/dist/index.json");
    let report = update_with(&dir, &net, true, false).unwrap();
    assert!(report.contains("not updated: node:"), "{report}");
    assert!(report.contains("clangd: 23.1.0 → 23.2.0"), "{report}");
}
