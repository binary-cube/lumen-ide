use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

const VTSLS: &str = r#"
[[language-servers]]
name = "vtsls"
languages = ["javascript", "typescript", "tsx"]
bundle = "vtsls"
command = "${node}"
args = ["${server}/bin/vtsls.js", "--stdio"]
env = { NODE_OPTIONS = "--max-old-space-size=${max-memory}" }
file-patterns = ["**/*.{js,ts,tsx}", "**/tsconfig.json"]
language-ids = { tsx = "typescriptreact" }
extension-language-ids = { jsx = "javascriptreact" }

[language-servers.settings.typescript.tsserver]
maxTsServerMemory = "${max-memory}"
log = "off"

[[language-servers]]
name = "typescript-language-server"
languages = ["typescript"]
command = "typescript-language-server"
args = ["--stdio"]
fallback = true
"#;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lsp-spec-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn servers_are_read_from_toml() {
    let servers = parse(VTSLS, "lang.javascript").unwrap();
    assert_eq!(servers.len(), 2);
    let vtsls = &servers[0];
    assert_eq!(vtsls.name, "vtsls");
    assert_eq!(vtsls.origin, "lang.javascript");
    assert_eq!(vtsls.languages, ["javascript", "typescript", "tsx"]);
    assert_eq!(
        vtsls.settings,
        json!({"typescript": {"tsserver": {"maxTsServerMemory": "${max-memory}", "log": "off"}}})
    );
    assert_eq!(vtsls.initialization_options, json!(null));
    assert!(!vtsls.fallback);
    assert!(servers[1].fallback);
}

#[test]
fn bad_descriptions_are_refused() {
    assert!(parse("[[language-servers]]\nname = \"x\"\nlanguages = [\"a\"]", "m").is_err());
    assert!(
        parse("[[language-servers]]\nname = \"\"\ncommand = \"x\"\nlanguages = [\"a\"]", "m")
            .is_err()
    );
    assert!(
        parse("[[language-servers]]\nname = \"x\"\ncommand = \"x\"\nlanguages = []", "m").is_err()
    );
    assert!(
        parse(
            "[[language-servers]]\nname = \"x\"\ncommand = \"x\"\nlanguages = [\"a\"]\nunknown = 1",
            "m"
        )
        .is_err()
    );
    assert_eq!(parse("", "m").unwrap(), Vec::new());
}

#[test]
fn language_ids_follow_the_extension_then_the_language() {
    let vtsls = &parse(VTSLS, "m").unwrap()[0];
    assert_eq!(vtsls.language_id("tsx", Path::new("a.tsx")), "typescriptreact");
    assert_eq!(vtsls.language_id("javascript", Path::new("a.jsx")), "javascriptreact");
    assert_eq!(vtsls.language_id("javascript", Path::new("a.mjs")), "javascript");
}

#[test]
fn watched_files_follow_the_patterns() {
    let vtsls = &parse(VTSLS, "m").unwrap()[0];
    assert!(vtsls.watches("src/app/main.ts"));
    assert!(vtsls.watches("main.tsx"));
    assert!(vtsls.watches("packages/a/tsconfig.json"));
    assert!(!vtsls.watches("src/main.rs"));
    assert!(!vtsls.watches("README.md"));
}

#[test]
fn globs() {
    assert!(glob("**/*.ts", "a.ts"));
    assert!(glob("**/*.ts", "a/b/c.ts"));
    assert!(!glob("*.ts", "a/b.ts"));
    assert!(glob("src/**", "src/a/b"));
    assert!(glob("a?c", "abc"));
    assert!(!glob("a?c", "a/c"));
    assert!(glob("**/{package,tsconfig}.json", "x/tsconfig.json"));
    assert!(!glob("**/{package,tsconfig}.json", "x/jsconfig.json"));
}

#[test]
fn placeholders_are_replaced() {
    let places = Places {
        lsp: Some("/app/lsp".into()),
        node: Some("/app/lsp/node/22/bin/node".into()),
        php: Some("/app/lsp/php/8/php".into()),
        server: Some("/app/lsp/servers/vtsls/1".into()),
        root: "/work/shop".into(),
        cache: Some("/work/shop/.data/lsp/vtsls".into()),
        data: None,
        max_memory: 3072,
    };
    assert_eq!(
        substitute("${server}/bin/x.js ${root} ${unknown}", &places),
        "/app/lsp/servers/vtsls/1/bin/x.js /work/shop ${unknown}"
    );
    // The bundled folder as a URI (a schema given by URL).
    assert_eq!(
        substitute("${server-uri}/schemas/cargo.json", &places),
        "file:///app/lsp/servers/vtsls/1/schemas/cargo.json"
    );
    let spaced = Places { server: Some("/Program Files/lsp/taplo".into()), ..places.clone() };
    assert_eq!(substitute("${server-uri}", &spaced), "file:///Program%20Files/lsp/taplo");
    let settings =
        json!({"a": "${max-memory}", "b": ["--max-old-space-size=${max-memory}"], "c": 1});
    assert_eq!(
        substitute_json(&settings, &places),
        json!({"a": 3072, "b": ["--max-old-space-size=3072"], "c": 1})
    );
}

#[test]
fn a_server_needing_the_bundle_says_what_is_missing() {
    let vtsls = &parse(VTSLS, "m").unwrap()[0];
    let none = Places { root: "/w".into(), max_memory: 1024, ..Places::default() };
    assert_eq!(vtsls.launch(&none), Err(Missing::Bundle));
    let dir = temp_dir("missing");
    let mut places = Places { lsp: Some(dir.clone()), node: Some(dir.join("node")), ..none };
    assert_eq!(vtsls.launch(&places), Err(Missing::Node));
    std::fs::write(dir.join("node"), "").unwrap();
    places.server = Some(dir.join("servers/vtsls"));
    assert_eq!(vtsls.launch(&places), Err(Missing::Server("vtsls".into())));
    std::fs::create_dir_all(dir.join("servers/vtsls")).unwrap();
    let launch = vtsls.launch(&places).unwrap();
    assert_eq!(launch.program, dir.join("node").into_os_string());
    assert_eq!(launch.args[1], "--stdio");
    assert_eq!(launch.env, vec![("NODE_OPTIONS".into(), "--max-old-space-size=1024".into())]);
    assert_eq!(launch.settings["typescript"]["tsserver"]["maxTsServerMemory"], json!(1024));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_program_is_found_absolute_in_the_bundle_or_missing() {
    let dir = temp_dir("program");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::write(dir.join("bin/own-server"), "").unwrap();
    let mut spec = parse(
        "[[language-servers]]\nname = \"own\"\nlanguages = [\"a\"]\ncommand = \"bin/own-server\"",
        "user",
    )
    .unwrap()
    .remove(0);
    let places =
        Places { lsp: Some(dir.clone()), root: dir.clone(), max_memory: 1, ..Places::default() };
    assert_eq!(spec.launch(&places).unwrap().program, dir.join("bin/own-server").into_os_string());
    spec.command = dir.join("bin/own-server").to_string_lossy().into_owned();
    assert!(spec.launch(&Places::default()).is_ok());
    spec.command = "no-such-language-server-anywhere".into();
    assert_eq!(
        spec.launch(&places),
        Err(Missing::Program("no-such-language-server-anywhere".into()))
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_root_is_the_nearest_folder_with_a_marker_within_the_project() {
    let dir = temp_dir("root");
    std::fs::create_dir_all(dir.join("packages/a/src")).unwrap();
    std::fs::write(dir.join("packages/a/tsconfig.json"), "{}").unwrap();
    let markers = vec!["tsconfig.json".to_owned()];
    let file = dir.join("packages/a/src/main.ts");
    assert_eq!(root_for(&file, &markers, &dir), dir.join("packages/a"));
    // No marker: the project's root.
    assert_eq!(root_for(&dir.join("x/y.ts"), &markers, &dir), dir);
    assert_eq!(root_for(&file, &[], &dir), dir);
    // A marker above the project doesn't count.
    let inner = dir.join("packages/a/src");
    assert_eq!(root_for(&inner.join("b.ts"), &markers, &inner), inner);
    let _ = std::fs::remove_dir_all(dir);
}

const JSON: &str = r#"
[[language-servers]]
name = "json"
languages = ["json"]
bundle = "web"
command = "${node}"
built-in = ["diagnostics"]
extension-language-ids = { jsonc = "jsonc" }
file-language-ids = { "tsconfig*.json" = "jsonc", ".babelrc" = "jsonc" }
contents = { "https://json.schemastore.org/" = "${server}/schemas" }
"#;

#[test]
fn file_names_choose_the_language_id_before_the_extension() {
    let json = &parse(JSON, "m").unwrap()[0];
    assert_eq!(json.language_id("json", Path::new("/p/tsconfig.json")), "jsonc");
    assert_eq!(json.language_id("json", Path::new("/p/tsconfig.base.json")), "jsonc");
    assert_eq!(json.language_id("json", Path::new("/p/.babelrc")), "jsonc");
    assert_eq!(json.language_id("json", Path::new("/p/a.jsonc")), "jsonc");
    assert_eq!(json.language_id("json", Path::new("/p/package.json")), "json");
    // Only the name is matched, not its folders.
    assert_eq!(json.language_id("json", Path::new("/tsconfig.d/package.json")), "json");
}

#[test]
fn what_the_built_in_engine_keeps_is_named() {
    assert_eq!(parse(JSON, "m").unwrap()[0].built_in, ["diagnostics"]);
    assert!(parse(VTSLS, "m").unwrap().iter().all(|s| s.built_in.is_empty()));
    let unknown = "[[language-servers]]\nname = \"x\"\ncommand = \"x\"\nlanguages = [\"a\"]\nbuilt-in = [\"hover\"]";
    assert!(parse(unknown, "m").unwrap_err().contains("hover"));
}

#[test]
fn contents_are_files_in_their_folders_only() {
    let dir = temp_dir("contents");
    std::fs::create_dir_all(dir.join("web")).unwrap();
    std::fs::write(dir.join("node"), "").unwrap();
    let json = &parse(JSON, "m").unwrap()[0];
    let places = Places {
        lsp: Some(dir.clone()),
        node: Some(dir.join("node")),
        server: Some(dir.join("web")),
        root: dir.clone(),
        cache: None,
        data: None,
        max_memory: 1,
        ..Places::default()
    };
    let launch = json.launch(&places).unwrap();
    let schemas = dir.join("web").join("schemas");
    assert_eq!(
        launch.contents,
        vec![("https://json.schemastore.org/".to_owned(), schemas.clone())]
    );
    let file = |url: &str| content_file(&launch.contents, url);
    assert_eq!(
        file("https://json.schemastore.org/package.json"),
        Some(schemas.join("package.json"))
    );
    assert_eq!(
        file("https://json.schemastore.org/a/b.json?x=1#/definitions/c"),
        Some(schemas.join("a").join("b.json"))
    );
    for refused in [
        "https://json.schemastore.org/",
        "https://json.schemastore.org/../secret.json",
        "https://json.schemastore.org/a//b.json",
        "https://json.schemastore.org/a\\..\\b.json",
        "https://json.schemastore.org/C:/x.json",
        "https://example.com/package.json",
        "http://json.schemastore.org/package.json",
    ] {
        assert_eq!(file(refused), None, "{refused}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_language_modules_descriptions_are_read() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules/languages");
    let read = |module: &str| {
        let path = modules.join(module).join("data/language-servers.toml");
        parse(&std::fs::read_to_string(&path).unwrap(), module).unwrap()
    };
    let names = |module: &str| read(module).into_iter().map(|s| s.name).collect::<Vec<_>>();
    assert_eq!(names("javascript"), ["vtsls"]);
    assert_eq!(names("css"), ["vscode-css"]);
    assert_eq!(names("html"), ["vscode-html"]);
    assert_eq!(names("json"), ["vscode-json"]);
    assert_eq!(read("css")[0].languages, ["css", "scss"]);
    // HTML's server has no diagnostics of its own; its embedded modes are off.
    let html = &read("html")[0];
    assert_eq!(html.built_in, ["diagnostics"]);
    assert_eq!(read("css")[0].built_in, ["deprecated", "rename"]);
    assert_eq!(
        html.initialization_options["embeddedLanguages"],
        json!({"css": false, "javascript": false})
    );
    // JSON: schemas only from the bundle, never downloaded.
    let json = &read("json")[0];
    assert_eq!(json.initialization_options["handledSchemaProtocols"], json!(["file"]));
    let schemas = json.settings["json"]["schemas"].as_array().unwrap();
    assert!(schemas.len() >= 8);
    for schema in schemas {
        let url = schema["url"].as_str().unwrap();
        assert!(json.contents.keys().any(|prefix| url.starts_with(prefix.as_str())), "{url}");
    }
    assert_eq!(json.language_id("json", Path::new("/p/tsconfig.json")), "jsonc");
    // Every server of the bundle shares its folder.
    for module in ["css", "html", "json"] {
        assert_eq!(read(module)[0].bundle.as_deref(), Some("vscode-langservers-extracted"));
    }
    // 43d: Bash, YAML, TOML, Dockerfile.
    assert_eq!(names("bash"), ["bash-language-server"]);
    assert_eq!(names("yaml"), ["yaml-language-server"]);
    assert_eq!(names("toml"), ["taplo"]);
    assert_eq!(names("dockerfile"), ["dockerfile-language-server"]);
    let bash = &read("bash")[0];
    assert_eq!(bash.language_id("bash", Path::new("/p/build.sh")), "shellscript");
    assert_eq!(bash.settings["bashIde"]["explainshellEndpoint"], json!(""));
    // No syntax errors of its own: the built-in engine's stay.
    assert_eq!(bash.built_in, ["diagnostics"]);
    // YAML: asked for its schemas only through the app, which has them all.
    let yaml = &read("yaml")[0];
    assert_eq!(yaml.on_initialized, ["yaml/registerContentRequest"]);
    assert_eq!(yaml.settings["yaml"]["schemaStore"]["enable"], json!(false));
    let schemas = yaml.settings["yaml"]["schemas"].as_object().unwrap();
    assert!(schemas.len() >= 5);
    for url in schemas.keys() {
        assert!(yaml.contents.keys().any(|prefix| url.starts_with(prefix.as_str())), "{url}");
    }
    // TOML: no catalog, the schemas bundled, nothing fetched.
    let taplo = &read("toml")[0];
    let schema = &taplo.settings["evenBetterToml"]["schema"];
    assert_eq!(schema["catalogs"], json!([]));
    for url in schema["associations"].as_object().unwrap().values() {
        assert!(url.as_str().unwrap().starts_with("${server-uri}/schemas/"), "{url}");
    }
    assert_eq!(taplo.env["HTTPS_PROXY"], "http://127.0.0.1:9");
}

#[test]
fn notifications_to_send_once_initialized_are_read() {
    let text = "[[language-servers]]\nname = \"y\"\nlanguages = [\"yaml\"]\ncommand = \"y\"\non-initialized = [\"yaml/registerContentRequest\"]\n";
    let spec = &parse(text, "m").unwrap()[0];
    assert_eq!(spec.on_initialized, ["yaml/registerContentRequest"]);
    let places = Places { root: "/w".into(), ..Places::default() };
    let dir = temp_dir("on-initialized");
    let program = dir.join("y");
    std::fs::write(&program, "").unwrap();
    let spec = ServerSpec { command: program.to_string_lossy().into_owned(), ..spec.clone() };
    assert_eq!(spec.launch(&places).unwrap().on_initialized, ["yaml/registerContentRequest"]);
    let _ = std::fs::remove_dir_all(dir);
}

const NATIVE: &str = r#"
[[language-servers]]
name = "clangd"
languages = ["c", "cpp"]
bundle = "clangd"
command = "${server}/bin/clangd"
args = ["--log=error"]
env = { XDG_CACHE_HOME = "${cache}" }
"#;

#[test]
fn a_servers_own_folder_is_in_the_projects_data_folder() {
    let root = Path::new("/work/shop");
    let dir = cache_dir(root, "lua-language-server");
    assert_eq!(dir, root.join(kernel::brand::PROJECT_DIR).join("lsp/lua-language-server"));
    // A user's server named anyhow stays one folder inside it.
    for name in ["../x", "a/b", "..", "c:\\d"] {
        let dir = cache_dir(root, name);
        assert_eq!(dir.parent().unwrap(), root.join(kernel::brand::PROJECT_DIR).join("lsp"));
        assert!(!dir.file_name().unwrap().to_string_lossy().contains(['/', '\\', ':']));
    }
}

#[test]
fn the_cache_placeholder_is_its_folder_made_only_when_named() {
    let dir = temp_dir("native");
    std::fs::create_dir_all(dir.join("servers/clangd/bin")).unwrap();
    let program = if cfg!(windows) { "clangd.exe" } else { "clangd" };
    std::fs::write(dir.join("servers/clangd/bin").join(program), "").unwrap();
    let cache = dir.join("project/.data/lsp/clangd");
    let places = Places {
        lsp: Some(dir.clone()),
        server: Some(dir.join("servers/clangd")),
        root: dir.join("project"),
        cache: Some(cache.clone()),
        data: None,
        max_memory: 1024,
        ..Places::default()
    };
    let clangd = &parse(NATIVE, "lang.cpp").unwrap()[0];
    let launch = clangd.launch(&places).unwrap();
    // `${server}/bin/clangd` is `clangd.exe` on Windows (compared as paths: `/` and `\\`
    // are both separators there).
    let expected = dir.join("servers/clangd/bin").join(program);
    assert_eq!(Path::new(&launch.program), expected.as_path());
    assert_eq!(launch.env, vec![("XDG_CACHE_HOME".into(), cache.clone().into_os_string())]);
    assert_eq!(launch.cache, Some(cache));
    // A server that doesn't name it has none to make.
    let vtsls = &parse(VTSLS, "m").unwrap()[1];
    let none = Places { cache: Some(dir.join("unused")), ..places };
    assert_eq!(
        vtsls.launch(&none).map(|l| l.cache),
        Err(Missing::Program("typescript-language-server".into()))
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_program_without_its_extension_is_found_with_exe_only_on_windows() {
    let dir = temp_dir("exe");
    std::fs::write(dir.join("tool.exe"), "").unwrap();
    let text = "[[language-servers]]\nname = \"t\"\nlanguages = [\"c\"]\ncommand = \"tool\"\n";
    let spec = &parse(text, "m").unwrap()[0];
    let places = Places { lsp: Some(dir.clone()), root: dir.clone(), ..Places::default() };
    let found = spec.launch(&places).map(|l| l.program);
    if cfg!(windows) {
        assert_eq!(found, Ok(dir.join("tool.exe").into_os_string()));
    } else {
        assert!(found.is_err(), "{found:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn completion_kinds_are_checked_when_read() {
    let with = |kinds: &str| {
        parse(
            &format!(
                "[[language-servers]]\nname = \"s\"\nlanguages = [\"css\"]\ncommand = \"s\"\ncompletion-kinds = {{ {kinds} }}\n"
            ),
            "m",
        )
    };
    let spec = &with("property = \"attribute\"").unwrap()[0];
    let places = Places { root: "/w".into(), ..Places::default() };
    let kinds = spec.launch(&places).map(|l| l.completion_kinds);
    if let Ok(kinds) = kinds {
        assert_eq!(kinds.of(10), code_intelligence::CompletionKind::Attribute);
    }
    assert!(with("propertee = \"attribute\"").unwrap_err().contains("propertee"));
    assert!(with("property = \"atribute\"").unwrap_err().contains("atribute"));
}

const PER_OS: &str = r#"
[[language-servers]]
name = "phpish"
languages = ["php"]
bundle = "phpish"
command = "${php}"
args = ["${server}/s.phar", "serve"]

[language-servers.os.windows]
default-engine = "built-in"
transport = "tcp"
args = ["${server}/s.phar", "serve", "--address=127.0.0.1:${port}"]
"#;

#[test]
fn what_differs_on_an_os_is_applied_when_read() {
    let unix = &parse_for(PER_OS, "m", "macos").unwrap()[0];
    assert_eq!(unix.default_engine(), code_intelligence::servers::Engine::Server);
    assert_eq!(unix.transport, Transport::Stdio);
    assert_eq!(unix.args, ["${server}/s.phar", "serve"]);
    let windows = &parse_for(PER_OS, "m", "windows").unwrap()[0];
    assert_eq!(windows.default_engine(), code_intelligence::servers::Engine::BuiltIn);
    assert_eq!(windows.transport, Transport::Tcp);
    assert_eq!(windows.args[2], "--address=127.0.0.1:${port}");
    // An unknown OS or engine is said.
    let bad_os = PER_OS.replace("os.windows", "os.amiga");
    assert!(parse_for(&bad_os, "m", "linux").unwrap_err().contains("amiga"));
    let bad_engine = PER_OS.replace("\"built-in\"", "\"lsp\"");
    assert!(parse_for(&bad_engine, "m", "linux").unwrap_err().contains("lsp"));
}

#[test]
fn the_php_placeholder_is_the_bundled_php() {
    let dir = temp_dir("php");
    std::fs::create_dir_all(dir.join("servers/phpish")).unwrap();
    std::fs::write(dir.join("php"), "").unwrap();
    let spec = &parse_for(PER_OS, "m", "windows").unwrap()[0];
    let places = Places {
        lsp: Some(dir.clone()),
        server: Some(dir.join("servers/phpish")),
        root: dir.clone(),
        ..Places::default()
    };
    // Without it, it can't start.
    assert_eq!(spec.launch(&places), Err(Missing::Php));
    let with = Places { php: Some(dir.join("php")), ..places };
    let launch = spec.launch(&with).unwrap();
    assert_eq!(launch.program, dir.join("php").into_os_string());
    assert_eq!(launch.transport, Transport::Tcp);
    // The port is chosen at each start: still named.
    assert!(launch.args[2].to_string_lossy().ends_with(":${port}"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_php_modules_description_is_read() {
    let text = include_str!("../../../../../modules/languages/php/data/language-servers.toml");
    for os in OSES {
        let phpactor = parse_for(text, "lang.php", os)
            .unwrap()
            .into_iter()
            .find(|s| s.name == "phpactor")
            .unwrap();
        assert_eq!(phpactor.languages, ["php"]);
        let windows = os == "windows";
        // The built-in engine by default only on Windows, phpactor over a port there.
        assert_eq!(
            phpactor.default_engine() == code_intelligence::servers::Engine::BuiltIn,
            windows
        );
        assert_eq!(phpactor.transport == Transport::Tcp, windows);
        assert_eq!(phpactor.args.iter().any(|a| a.contains("${port}")), windows);
        // What the built-in engine keeps: the union of suggestions, its rename, its
        // diagnostics.
        let kept = code_intelligence::servers::Kept::from_names(
            phpactor.built_in.iter().map(String::as_str),
        );
        assert!(kept.completion && kept.rename && kept.diagnostics && !kept.deprecated);
        // No php.ini of the machine's; its own watchers off (the client's events).
        assert_eq!(phpactor.args[0], "-n");
        let options = &phpactor.initialization_options;
        assert_eq!(options["indexer.enabled_watchers"], json!(["lsp"]));
        assert_eq!(options["language_server.diagnostic_outsource"], json!(false));
        assert!(phpactor.watches("src/Model/User.php"));
    }
}

#[test]
fn intelephense_is_described_as_installed_by_the_user() {
    let text = include_str!("../../../../../modules/languages/php/data/language-servers.toml");
    for os in OSES {
        let servers = parse_for(text, "lang.php", os).unwrap();
        let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
        // intelephense first (the default once installed, plan 43j), phpactor after it.
        assert_eq!(names, ["intelephense", "phpactor"]);
        let intelephense = &servers[0];
        // Never bundled: installed from npm's registry, pinned, its SHA-512 pinned.
        assert_eq!(intelephense.bundle, None);
        let install = intelephense.install.as_ref().unwrap();
        assert_eq!(install.npm, "intelephense");
        assert!(install.integrity.starts_with("sha512-") && install.integrity.len() > 80);
        assert_eq!(install.entry, "lib/intelephense.js");
        assert_eq!(install.program.as_deref(), Some("intelephense"));
        assert!(!install.version.is_empty() && install.size.is_some());
        // Its default where it's offered: the server on every OS (phpactor's is the
        // built-in engine's on Windows: intelephense, once installed, answers there).
        assert_eq!(intelephense.default_engine(), code_intelligence::servers::Engine::Server);
        assert_eq!(intelephense.transport, Transport::Stdio);
        // Its licence key from the secrets; its notifications shown.
        assert_eq!(
            intelephense.secret_options.get("licenceKey").map(String::as_str),
            Some("intelephense-licence")
        );
        assert!(intelephense.show_messages);
        // The built-in engine keeps its suggestions and diagnostics; Rename / Find Usages
        // only through its capabilities (a licence's).
        let kept = code_intelligence::servers::Kept::from_names(
            intelephense.built_in.iter().map(String::as_str),
        );
        assert!(kept.completion && kept.diagnostics && !kept.rename);
        assert_eq!(intelephense.settings["intelephense"]["telemetry"]["enabled"], json!(false));
        let excluded =
            intelephense.settings["intelephense"]["files"]["exclude"].as_array().unwrap();
        assert!(excluded.iter().any(|e| e == "**/vendor/**/{Tests,tests}/**"));
        assert!(excluded.iter().any(|e| e == "**/storage/framework/views/**"));
    }
}

#[test]
fn an_installed_server_starts_from_its_folder_or_says_it_isnt_installed() {
    let text = include_str!("../../../../../modules/languages/php/data/language-servers.toml");
    let intelephense =
        parse(text, "lang.php").unwrap().into_iter().find(|s| s.name == "intelephense").unwrap();
    let dir = std::env::temp_dir().join(format!("spec-installed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lsp/node")).unwrap();
    let node = dir.join("lsp/node/node");
    std::fs::write(&node, "").unwrap();
    let places = Places {
        lsp: Some(dir.join("lsp")),
        node: Some(node.clone()),
        root: dir.join("project"),
        cache: Some(dir.join("project/.data/lsp/intelephense")),
        data: Some(dir.join("data/intelephense")),
        max_memory: 2048,
        ..Places::default()
    };
    assert_eq!(intelephense.launch(&places), Err(Missing::NotInstalled("intelephense".into())));
    let package = dir.join("installed/intelephense/1.18.5");
    std::fs::create_dir_all(package.join("lib")).unwrap();
    let with = Places { server: Some(package.clone()), ..places };
    let launch = intelephense.launch(&with).unwrap();
    assert_eq!(launch.program, node.into_os_string());
    let args: Vec<String> = launch.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args[0], "--max-old-space-size=2048");
    assert_eq!(args[1], format!("{}/lib/intelephense.js", package.to_string_lossy()));
    assert_eq!(args[2], "--stdio");
    // Its index per project, its licence's activation for every project.
    let options = &launch.initialization_options;
    assert_eq!(
        options["storagePath"],
        json!(dir.join("project/.data/lsp/intelephense").to_string_lossy())
    );
    assert_eq!(
        options["globalStoragePath"],
        json!(dir.join("data/intelephense").to_string_lossy())
    );
    assert_eq!(launch.data, Some(dir.join("data/intelephense")));
    assert!(launch.cache.is_some());
    assert_eq!(
        launch.secret_options,
        [("licenceKey".to_owned(), "intelephense-licence".to_owned())]
    );
    // Without the app's Node, nothing (it's always run by ours).
    let no_node = Places { node: None, ..with };
    assert_eq!(intelephense.launch(&no_node), Err(Missing::Node));
}

#[test]
fn a_server_is_told_what_was_excluded_beside_what_it_lists() {
    let toml = r#"
        [[language-servers]]
        name = "s"
        languages = ["php"]
        command = "/bin/sh"
        excluded-paths = { setting = "s.files.exclude", option = "paths", pattern = "**/{path}/**" }
        [language-servers.settings.s.files]
        exclude = ["**/.git/**"]
    "#;
    // A program there on every system: this test's own.
    let exe = toml::Value::String(std::env::current_exe().unwrap().display().to_string());
    let toml = toml.replace("\"/bin/sh\"", &exe.to_string());
    let server = parse(&toml, "test").unwrap().remove(0);
    let launch = server.launch(&Places::default()).unwrap();
    let told = launch.excluding(&server, &["var/old".to_owned()]);
    assert_eq!(
        told.settings["s"]["files"]["exclude"],
        serde_json::json!(["**/.git/**", "**/var/old/**"])
    );
    assert_eq!(launch.excluding(&server, &[]).settings, launch.settings);
}
