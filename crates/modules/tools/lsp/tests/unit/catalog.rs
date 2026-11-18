use code_intelligence::servers::ServerData;

use super::*;

const MODULE: &str = r#"
[[language-servers]]
name = "vtsls"
languages = ["javascript", "typescript"]
bundle = "vtsls"
command = "${node}"
args = ["${server}/vtsls.js", "--stdio"]

[[language-servers]]
name = "tsls"
languages = ["typescript"]
command = "no-such-ts-server-anywhere"
fallback = true
"#;

fn data() -> Vec<ServerData> {
    vec![
        ServerData { module: "lang.javascript", toml: MODULE },
        ServerData { module: "lang.bad", toml: "[[language-servers]]\nname = 1" },
    ]
}

#[test]
fn the_modules_descriptions_are_read_and_bad_ones_said() {
    let catalog = Catalog::load(&data(), None);
    assert_eq!(catalog.servers.len(), 2);
    assert_eq!(catalog.errors.len(), 1);
    assert!(catalog.errors[0].starts_with("lang.bad"));
    assert_eq!(catalog.languages(), ["javascript", "typescript"]);
    assert_eq!(catalog.for_language("typescript").count(), 2);
    assert_eq!(catalog.for_language("css").count(), 0);
}

#[test]
fn the_users_file_adds_servers_and_replaces_one_of_the_same_name() {
    let dir = std::env::temp_dir().join(format!("lsp-catalog-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let user = dir.join(USER_FILE);
    std::fs::write(&user, "[[language-servers]]\nname = \"tsls\"\nlanguages = [\"typescript\"]\ncommand = \"mine\"\n\n[[language-servers]]\nname = \"clangd\"\nlanguages = [\"c\"]\ncommand = \"clangd\"\n").unwrap();
    let catalog = Catalog::load(&data(), Some(&user));
    assert_eq!(catalog.named("tsls").unwrap().command, "mine");
    assert_eq!(catalog.named("tsls").unwrap().origin, "user");
    assert_eq!(catalog.languages(), ["javascript", "typescript", "c"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_server_is_chosen_where_it_can_start_else_why_not() {
    // The module's state held (another test's downloads folder would have servers in it):
    // nothing downloaded.
    let _h = crate::harness::Harness::new(lsp_client::fake::Behaviour::default());
    crate::downloads::use_downloads(None);
    let catalog = Catalog::load(&data(), None);
    let places =
        |spec: &lsp_client::spec::ServerSpec| places(None, spec, std::path::Path::new("/p"), 1024);
    // No bundle: vtsls can't, its fallback isn't found either; the first reason is said.
    let chosen = catalog.choose("typescript", None, &[], places);
    assert_eq!(chosen.err(), Some(Some(Missing::Bundle)));
    // Turned off: the next one's reason.
    let chosen = catalog.choose("typescript", None, &["vtsls".into()], places);
    assert_eq!(chosen.err(), Some(Some(Missing::Program("no-such-ts-server-anywhere".into()))));
    assert_eq!(catalog.choose("css", None, &[], places).err(), Some(None));
}

#[test]
fn the_server_preferred_comes_first() {
    let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    let text = format!(
        "[[language-servers]]\nname = \"a\"\nlanguages = [\"x\"]\ncommand = {exe:?}\n\n[[language-servers]]\nname = \"b\"\nlanguages = [\"x\"]\ncommand = {exe:?}\n"
    );
    let data = [ServerData { module: "m", toml: Box::leak(text.into_boxed_str()) }];
    let catalog = Catalog::load(&data, None);
    let places =
        |spec: &lsp_client::spec::ServerSpec| places(None, spec, std::path::Path::new("/p"), 1024);
    assert_eq!(catalog.choose("x", None, &[], places).unwrap().0.name, "a");
    assert_eq!(catalog.choose("x", Some("b"), &[], places).unwrap().0.name, "b");
    assert_eq!(catalog.choose("x", Some("gone"), &[], places).unwrap().0.name, "a");
}

#[test]
fn a_server_added_is_written_to_the_users_file() {
    let _harness = crate::harness::Harness::new(Default::default());
    let before = std::fs::read_to_string(user_file().unwrap()).unwrap();
    let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    let path =
        add_user_server("mine \"quoted\"", &exe, &["--stdio".into()], &["lua".into()]).unwrap();
    let catalog = catalog();
    let mine = catalog.named("mine \"quoted\"").unwrap();
    assert_eq!(mine.languages, ["lua"]);
    assert_eq!(mine.args, ["--stdio"]);
    assert!(
        add_user_server("mine \"quoted\"", &exe, &[], &["lua".into()])
            .unwrap_err()
            .contains("exists")
    );
    assert!(add_user_server("x", "", &[], &["lua".into()]).is_err());
    std::fs::write(&path, before).unwrap();
    reload();
}
