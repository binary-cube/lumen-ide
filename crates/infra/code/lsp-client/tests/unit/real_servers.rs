//! The real bundled servers (only where `cargo xtask lsp-bundle` made `target/lsp/`; skipped
//! otherwise), each started from its language module's own description on a small project
//! in a temporary folder: vtsls (TypeScript), the extracted CSS / SCSS, HTML and JSON servers
//! (the JSON one with the bundled schemas only), clangd (C), lua-language-server (Lua),
//! ZLS (Zig), the shell, YAML, TOML and Dockerfile servers, and phpactor (PHP, on the bundled
//! PHP, over its pipes and over a local port).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use document::TextBuffer;
use kernel::DocumentId;

use crate::bundle::Bundle;
use crate::client::{Client, Config, Event, RestartPolicy, Timeouts};
use crate::launch::ProcessLauncher;
use crate::spec::{self, Places};
use crate::status::State;

/// The bundle made for development, if it's there (or the one `EDITOR_LSP_DIR` names: the
/// servers downloaded by `downloads`' real test, with their bundle manifest).
fn bundle() -> Option<Bundle> {
    if let Some(dir) = std::env::var_os("EDITOR_LSP_DIR") {
        return Bundle::open(Path::new(&dir));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../target/lsp");
    Bundle::open(&root.canonicalize().ok()?)
}

fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("real-vtsls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true, "target": "es2022", "module": "esnext"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/math.ts"),
        "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
    )
    .unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn vtsls_answers_from_the_bundle() {
    let Some(bundle) = bundle() else {
        eprintln!("no target/lsp: skipped (cargo xtask lsp-bundle)");
        return;
    };
    let text =
        include_str!("../../../../../modules/languages/javascript/data/language-servers.toml");
    let vtsls = spec::parse(text, "lang.javascript").unwrap().remove(0);
    let root = project();
    let places = Places {
        lsp: Some(bundle.root.clone()),
        node: bundle.node(),
        php: bundle.php(),
        server: bundle.server("vtsls").map(|(p, _)| p),
        root: root.clone(),
        cache: None,
        data: None,
        max_memory: 1024,
    };
    let launch = vtsls.launch(&places).unwrap();
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let heard = Arc::clone(&events);
    let config = Config {
        name: "vtsls".into(),
        root: root.clone(),
        project_root: root.clone(),
        initialization_options: launch.initialization_options.clone(),
        settings: launch.settings.clone(),
        contents: launch.contents.clone(),
        completion_kinds: launch.completion_kinds.clone(),
        on_initialized: launch.on_initialized.clone(),
        debounce: Duration::from_millis(25),
        timeouts: Timeouts {
            start: Duration::from_secs(60),
            request: Duration::from_secs(30),
            shutdown: Duration::from_secs(2),
        },
        restart: RestartPolicy::default(),
        idle: None,
        trace: Arc::new(AtomicBool::new(std::env::var_os("LSP_TRACE").is_some())),
        secret_options: Vec::new(),
        secrets: None,
        show_messages: false,
        wake: Arc::new(|| {}),
    };
    let started = Instant::now();
    let client = Client::start(
        config,
        Box::new(ProcessLauncher { launch, dir: root.clone() }),
        Arc::new(move |e| heard.lock().unwrap().push(e)),
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while !matches!(client.status().state(), State::Ready | State::Idle) {
        assert!(
            Instant::now() < until,
            "vtsls didn't start: {:?}",
            client.status().snapshot(Instant::now())
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    eprintln!("vtsls ready in {:?}", started.elapsed());
    let source = "import { add } from './math';\nconst total: number = add(1, 2);\ntotal.toFi\nconst wrong: string = total;\n";
    let doc = DocumentId::next();
    let path = root.join("src/main.ts");
    std::fs::write(&path, source).unwrap();
    client.open(doc, path, "typescript".into(), 1, TextBuffer::new(source).snapshot());
    let wait = Duration::from_secs(30);
    // Completion of a number's members.
    let at = source.find("toFi").unwrap() + 4;
    let completion = client.completion(doc, at).wait(wait).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label == "toFixed"),
        "{:?}",
        completion.items.iter().take(5).collect::<Vec<_>>()
    );
    // Hover, definition across files.
    let total = source.find("total").unwrap() + 1;
    let hover = client.hover(doc, total, 0..0).wait(wait).flatten().unwrap();
    assert!(hover.code.contains("const total: number"), "{hover:?}");
    // The type error, published.
    let until = Instant::now() + wait;
    loop {
        let found = events.lock().unwrap().iter().any(|e| matches!(e, Event::Diagnostics { diagnostics, .. } if diagnostics.iter().any(|d| d.rule.as_deref() == Some("2322"))));
        if found {
            break;
        }
        assert!(Instant::now() < until, "no diagnostics: {:?}", events.lock().unwrap());
        std::thread::sleep(Duration::from_millis(20));
    }
    // Once the project is loaded: a definition in another file.
    let add = source.find("add(1").unwrap() + 1;
    let definition = client.definition(doc, add).wait(wait).unwrap();
    assert!(definition.iter().any(|l| l.path.ends_with("src/math.ts")), "{definition:?}");
    let snapshot = client.status().snapshot(Instant::now());
    assert!(snapshot.pid.is_some());
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

/// A real server started from its module's description in `root`: the client and the
/// events it heard (`None`: no bundle).
fn start(toml: &str, name: &str, root: &Path) -> Option<(Client, Arc<Mutex<Vec<Event>>>)> {
    let spec = spec::parse(toml, "test").unwrap().into_iter().find(|s| s.name == name).unwrap();
    start_spec(spec, root)
}

/// A real server started from `spec` in `root` (see [`start`]).
fn start_spec(spec: spec::ServerSpec, root: &Path) -> Option<(Client, Arc<Mutex<Vec<Event>>>)> {
    start_installed(spec, root, None)
}

/// A real server started from `spec` in `root`, its folder `installed` when the user
/// installs it (not bundled), its own data folder beside the project.
fn start_installed(
    spec: spec::ServerSpec,
    root: &Path,
    installed: Option<PathBuf>,
) -> Option<(Client, Arc<Mutex<Vec<Event>>>)> {
    let Some(bundle) = bundle() else {
        eprintln!("no target/lsp: skipped (cargo xtask lsp-bundle)");
        return None;
    };
    let name = spec.name.clone();
    let name = name.as_str();
    let places = Places {
        lsp: Some(bundle.root.clone()),
        node: bundle.node(),
        php: bundle.php(),
        server: installed
            .or_else(|| spec.bundle.as_deref().and_then(|b| bundle.server(b)).map(|(p, _)| p)),
        root: root.to_path_buf(),
        cache: Some(spec::cache_dir(root, &spec.name)),
        data: Some(root.join(".app-data").join(&spec.name)),
        max_memory: 1024,
    };
    let launch = spec.launch(&places).unwrap();
    // Its folders, made as the module makes them before it starts.
    for dir in launch.cache.iter().chain(&launch.data) {
        std::fs::create_dir_all(dir).unwrap();
    }
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let heard = Arc::clone(&events);
    let config = Config {
        name: name.into(),
        root: root.to_path_buf(),
        project_root: root.to_path_buf(),
        initialization_options: launch.initialization_options.clone(),
        settings: launch.settings.clone(),
        contents: launch.contents.clone(),
        completion_kinds: launch.completion_kinds.clone(),
        on_initialized: launch.on_initialized.clone(),
        debounce: Duration::from_millis(25),
        timeouts: Timeouts {
            start: Duration::from_secs(60),
            request: Duration::from_secs(30),
            shutdown: Duration::from_secs(2),
        },
        restart: RestartPolicy::default(),
        idle: None,
        trace: Arc::new(AtomicBool::new(std::env::var_os("LSP_TRACE").is_some())),
        secret_options: Vec::new(),
        secrets: None,
        show_messages: false,
        wake: Arc::new(|| {}),
    };
    let started = Instant::now();
    let client = Client::start(
        config,
        Box::new(ProcessLauncher { launch, dir: root.to_path_buf() }),
        Arc::new(move |e| heard.lock().unwrap().push(e)),
    )
    .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while !matches!(client.status().state(), State::Ready | State::Idle) {
        assert!(
            Instant::now() < until,
            "{name} didn't start: {:?}",
            client.status().snapshot(Instant::now())
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    eprintln!("{name} ready in {:?}", started.elapsed());
    Some((client, events))
}

fn temp_project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("real-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// The diagnostics last published for a file ending in `file`, once one says `what`.
fn diagnostics_saying(
    events: &Mutex<Vec<Event>>,
    file: &str,
    what: impl Fn(&workbench_model::Diagnostic) -> bool,
) -> Vec<workbench_model::Diagnostic> {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        let last = events.lock().unwrap().iter().rev().find_map(|e| match e {
            Event::Diagnostics { path, diagnostics } if path.ends_with(file) => {
                Some(diagnostics.clone())
            }
            _ => None,
        });
        if let Some(found) = last.filter(|d| d.iter().any(&what)) {
            return found;
        }
        assert!(
            Instant::now() < until,
            "no such diagnostic in {file}: {:?}",
            events.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

const CSS: &str = include_str!("../../../../../modules/languages/css/data/language-servers.toml");
const HTML: &str = include_str!("../../../../../modules/languages/html/data/language-servers.toml");
const JSON: &str = include_str!("../../../../../modules/languages/json/data/language-servers.toml");
const WAIT: Duration = Duration::from_secs(30);

#[test]
fn the_css_server_answers_css_and_scss_from_the_bundle() {
    let root = temp_project("css");
    let Some((client, events)) = start(CSS, "vscode-css", &root) else { return };
    let source = "a {\n  colr: red;\n  col\n}\n";
    let doc = DocumentId::next();
    client.open(doc, root.join("a.css"), "css".into(), 1, TextBuffer::new(source).snapshot());
    let at = source.find("col\n").unwrap() + 3;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label == "color"),
        "{:?}",
        completion.items.len()
    );
    // A selector's specificity.
    let hover = client.hover(doc, 0, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some());
    let found = diagnostics_saying(&events, "a.css", |d| d.message.contains("colr"));
    assert!(found.iter().all(|d| d.source.contains("css")), "{found:?}");
    // A rename of a name longer than the word at the cursor (`--gap`, not `gap`): its places.
    let vars = ":root { --gap: 4px; }\n.a { margin: var(--gap); padding: var(--gap); }\n";
    let doc = DocumentId::next();
    client.open(doc, root.join("vars.css"), "css".into(), 1, TextBuffer::new(vars).snapshot());
    let places = client.rename_places(doc, vars.find("gap").unwrap() + 1).wait(WAIT).unwrap();
    assert_eq!(places.map(|p| p.len()), Ok(3));
    // SCSS in the same server: a variable suggested where it's used.
    // (Asked at the `$`, as the editor asks at a word's start and filters as it's typed.)
    let scss = "$gap: 4px;\n.b { margin: $ }\n";
    let doc = DocumentId::next();
    client.open(doc, root.join("b.scss"), "scss".into(), 1, TextBuffer::new(scss).snapshot());
    let at = scss.find("$ }").unwrap() + 1;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label == "$gap"),
        "{:?}",
        completion.items.len()
    );
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_html_server_answers_html_and_leaves_embedded_code_alone() {
    let root = temp_project("html");
    let Some((client, events)) = start(HTML, "vscode-html", &root) else { return };
    let source = "<!DOCTYPE html>\n<html>\n<body>\n<di\n<style>a { colr: red }</style>\n<script>let x: = 1;</script>\n</body>\n</html>\n";
    let doc = DocumentId::next();
    client.open(doc, root.join("index.html"), "html".into(), 1, TextBuffer::new(source).snapshot());
    let at = source.find("<di").unwrap() + 3;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(completion.items.iter().any(|i| i.item.label == "div"), "{:?}", completion.items.len());
    let hover = client.hover(doc, source.find("body").unwrap() + 1, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some_and(|h| h.doc.is_some() || !h.code.is_empty()));
    // Inside `<style>`: nothing (the built-in engine's).
    let at = source.find("colr").unwrap() + 2;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(completion.items.iter().all(|i| i.item.label != "color"));
    // Formatting answered.
    let options = crate::client::FormatOptions { tab_size: 2, insert_spaces: true };
    assert!(client.format(doc, None, options).wait(WAIT).unwrap().is_ok());
    // No diagnostic about the embedded code (its checks are off).
    std::thread::sleep(Duration::from_millis(500));
    let published =
        events.lock().unwrap().iter().any(
            |e| matches!(e, Event::Diagnostics { diagnostics, .. } if !diagnostics.is_empty()),
        );
    assert!(!published, "{:?}", events.lock().unwrap());
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_json_server_checks_with_the_bundled_schemas_only() {
    let root = temp_project("json");
    let Some((client, events)) = start(JSON, "vscode-json", &root) else { return };
    // package.json's schema refers to others (eslint's, prettier's…): all bundled, so no
    // "problems loading" warning; a wrong type found.
    let package = "{\n  \"name\": \"x\",\n  \"version\": 1,\n  \"scripts\": {}\n}\n";
    let doc = DocumentId::next();
    let path = root.join("package.json");
    client.open(doc, path, "json".into(), 1, TextBuffer::new(package).snapshot());
    let found = diagnostics_saying(&events, "package.json", |d| d.line == 2);
    assert!(
        found.iter().all(|d| !d.message.contains("loading") && !d.message.contains("download")),
        "{found:?}"
    );
    let at = package.find("\"scripts\"").unwrap() + 1;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some(), "the schema's description of `scripts`");
    // composer.json: Composer's schema.
    let composer = "{\n  \"require\": 5\n}\n";
    let doc = DocumentId::next();
    let path = root.join("composer.json");
    client.open(doc, path, "json".into(), 1, TextBuffer::new(composer).snapshot());
    diagnostics_saying(&events, "composer.json", |d| d.line == 1);
    // tsconfig.json is JSON with comments.
    let tsconfig = "{\n  // strict\n  \"compilerOptions\": {\"strict\": \"yes\"}\n}\n";
    let doc = DocumentId::next();
    let path = root.join("tsconfig.json");
    client.open(doc, path, "jsonc".into(), 1, TextBuffer::new(tsconfig).snapshot());
    let found = diagnostics_saying(&events, "tsconfig.json", |d| d.line == 2);
    assert!(found.iter().all(|d| d.line != 1), "comments are allowed: {found:?}");
    // A schema elsewhere is never downloaded: refused, said as the server says it.
    let other = "{\"$schema\": \"https://example.com/schema.json\", \"a\": 1}\n";
    let doc = DocumentId::next();
    let path = root.join("other.json");
    client.open(doc, path, "json".into(), 1, TextBuffer::new(other).snapshot());
    diagnostics_saying(&events, "other.json", |d| d.message.contains("downloads are off"));
    let (log, _) = client.status().log_lines();
    assert!(
        log.iter().any(
            |l| l.contains("never downloaded") && l.contains("https://example.com/schema.json")
        ),
        "{log:?}"
    );
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

const CPP: &str = include_str!("../../../../../modules/languages/cpp/data/language-servers.toml");
const LUA: &str = include_str!("../../../../../modules/languages/lua/data/language-servers.toml");
const ZIG: &str = include_str!("../../../../../modules/languages/zig/data/language-servers.toml");

/// `source` written to `root/name` and opened in `client` as `language`.
fn open_file(client: &Client, root: &Path, name: &str, language: &str, source: &str) -> DocumentId {
    let path = root.join(name);
    std::fs::write(&path, source).unwrap();
    let doc = DocumentId::next();
    client.open(doc, path, language.into(), 1, TextBuffer::new(source).snapshot());
    doc
}

#[test]
fn clangd_answers_c_from_the_bundle() {
    let root = temp_project("clangd");
    std::fs::write(root.join("compile_flags.txt"), "-std=c11\n-Wall\n").unwrap();
    std::fs::write(root.join("math.h"), "/** Adds two numbers. */\nint add(int a, int b);\n")
        .unwrap();
    std::fs::write(
        root.join("math.c"),
        "#include \"math.h\"\nint add(int a, int b) { return a + b; }\n",
    )
    .unwrap();
    let Some((client, events)) = start(CPP, "clangd", &root) else { return };
    let source = "#include \"math.h\"\n\nint main(void) {\n  int total = add(1, 2);\n  ad\n  return missing_name;\n}\n";
    let doc = open_file(&client, &root, "main.c", "c", source);
    // Once the file is read (before, clangd suggests only the file's words, as text).
    let found = diagnostics_saying(&events, "main.c", |d| d.message.contains("undeclared"));
    assert!(!found.is_empty());
    let at = source.find("ad\n").unwrap() + 2;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label.trim_start().starts_with("add")),
        "{:?}",
        completion.items.iter().take(5).collect::<Vec<_>>()
    );
    // The function's kind shown as a function, not a variable.
    let add = completion.items.iter().find(|i| i.item.label.trim_start().starts_with("add"));
    assert_eq!(add.map(|i| i.item.kind), Some(code_intelligence::CompletionKind::Function));
    let at = source.find("add(1").unwrap() + 1;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten().unwrap();
    assert!(format!("{hover:?}").contains("add"), "{hover:?}");
    let definition = client.definition(doc, at).wait(WAIT).unwrap();
    assert!(
        definition.iter().any(|l| l.path.ends_with("math.h") || l.path.ends_with("math.c")),
        "{definition:?}"
    );
    // Its process (with its PID) is what the Task Manager shows.
    assert!(client.status().snapshot(Instant::now()).pid.is_some());
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lua_language_server_answers_lua_and_writes_only_in_its_folder() {
    let root = temp_project("luals");
    let Some(bundle) = bundle() else { return };
    let Some((client, events)) = start(LUA, "lua-language-server", &root) else { return };
    let source = "local function greet(name)\n  return \"hi \" .. name\nend\n\ngre\nprint(greet(\"x\"), undefined_global_x)\n";
    let doc = open_file(&client, &root, "main.lua", "lua", source);
    // Once the workspace is loaded.
    let found =
        diagnostics_saying(&events, "main.lua", |d| d.message.contains("undefined_global_x"));
    assert!(!found.is_empty());
    let at = source.find("gre\n").unwrap() + 3;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label.starts_with("greet")),
        "{:?}",
        completion.items.iter().take(5).collect::<Vec<_>>()
    );
    let at = source.find("greet(\"x\")").unwrap() + 1;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some());
    let definition = client.definition(doc, at).wait(WAIT).unwrap();
    assert!(
        definition.iter().any(|l| l.path.ends_with("main.lua") && l.range.start < 30),
        "{definition:?}"
    );
    // Its log and standard library's definitions are in its folder, not in the bundle.
    let cache = spec::cache_dir(&root, "lua-language-server");
    assert!(cache.join("meta").is_dir(), "{}", cache.display());
    let (installed, _) = bundle.server("lua-language-server").unwrap();
    assert!(!installed.join("log").exists(), "nothing written in the bundle");
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn zls_answers_zig_without_zig_installed() {
    let root = temp_project("zls");
    std::fs::write(root.join("build.zig"), "").unwrap();
    let Some((client, events)) = start(ZIG, "zls", &root) else { return };
    let source = "fn add(a: i32, b: i32) i32 {\n    return a + b;\n}\n\npub fn main() void {\n    _ = add(1, 2);\n    _ = ad\n}\n\nconst broken = ;\n";
    let doc = open_file(&client, &root, "main.zig", "zig", source);
    let at = source.find("ad\n").unwrap() + 2;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label == "add"),
        "{:?}",
        completion.items.iter().take(5).collect::<Vec<_>>()
    );
    let at = source.find("add(1").unwrap() + 1;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some());
    let definition = client.definition(doc, at).wait(WAIT).unwrap();
    assert!(definition.iter().any(|l| l.path.ends_with("main.zig") && l.range.start < 10));
    // A syntax error, from its own parser.
    diagnostics_saying(&events, "main.zig", |d| d.message.contains("expected"));
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

const BASH: &str = include_str!("../../../../../modules/languages/bash/data/language-servers.toml");
const YAML: &str = include_str!("../../../../../modules/languages/yaml/data/language-servers.toml");
const TOML: &str = include_str!("../../../../../modules/languages/toml/data/language-servers.toml");
const DOCKERFILE: &str =
    include_str!("../../../../../modules/languages/dockerfile/data/language-servers.toml");

/// Like [`start`], skipped too when this bundle was made before the server was in it.
fn start_bundled(toml: &str, name: &str, root: &Path) -> Option<(Client, Arc<Mutex<Vec<Event>>>)> {
    let spec = spec::parse(toml, "test").unwrap().into_iter().find(|s| s.name == name)?;
    let folder = spec.bundle.as_deref()?;
    if bundle()?.server(folder).is_none() {
        eprintln!("{folder} isn't in target/lsp: skipped (cargo xtask lsp-bundle)");
        return None;
    }
    start(toml, name, root)
}

#[test]
fn bash_language_server_answers_shell_scripts() {
    let root = temp_project("bash");
    let Some((client, events)) = start_bundled(BASH, "bash-language-server", &root) else {
        return;
    };
    let source = "#!/usr/bin/env bash\ngreet() {\n  echo \"hi $1\"\n}\n\ngre\ngreet world\n";
    let doc = open_file(&client, &root, "main.sh", "shellscript", source);
    // It's read (its diagnostics: ShellCheck's, if it's in PATH; none of its own).
    let until = Instant::now() + WAIT;
    while !events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, Event::Diagnostics { path, .. } if path.ends_with("main.sh")))
    {
        assert!(Instant::now() < until, "never read");
        std::thread::sleep(Duration::from_millis(20));
    }
    let at = source.find("gre\n").unwrap() + 3;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label == "greet"),
        "{:?}",
        completion.items.iter().take(5).collect::<Vec<_>>()
    );
    let at = source.find("greet world").unwrap() + 1;
    let definition = client.definition(doc, at).wait(WAIT).unwrap();
    assert!(
        definition.iter().any(|l| l.path.ends_with("main.sh") && l.range.start < 30),
        "{definition:?}"
    );
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some(), "the function's own line");
    assert!(client.status().snapshot(Instant::now()).pid.is_some());
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn yaml_language_server_checks_with_the_bundled_schemas_only() {
    let root = temp_project("yaml");
    let Some((client, events)) = start_bundled(YAML, "yaml-language-server", &root) else {
        return;
    };
    // Docker Compose: the Compose specification's schema, asked of the app.
    let compose = "services:\n  web:\n    image: nginx\n    ports: 80\n    restrat: always\n";
    let doc = open_file(&client, &root, "compose.yaml", "yaml", compose);
    let found = diagnostics_saying(&events, "compose.yaml", |d| d.line == 4 || d.line == 3);
    assert!(found.iter().all(|d| !d.message.contains("downloads are off")), "{found:?}");
    let at = compose.find("image").unwrap() + 1;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some(), "the schema's description of `image`");
    // A GitHub workflow: its keys completed from SchemaStore's schema.
    std::fs::create_dir_all(root.join(".github/workflows")).unwrap();
    let workflow = "on: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    \n";
    let doc = open_file(&client, &root, ".github/workflows/ci.yml", "yaml", workflow);
    let at = workflow.len() - 1;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label == "steps"),
        "{:?}",
        completion.items.iter().take(8).collect::<Vec<_>>()
    );
    // A schema elsewhere is never downloaded: refused, and said so in its log.
    let other = "# yaml-language-server: $schema=https://example.com/schema.json\na: 1\n";
    open_file(&client, &root, "other.yaml", "yaml", other);
    let until = Instant::now() + WAIT;
    loop {
        let (log, _) = client.status().log_lines();
        if log.iter().any(|l| l.contains("never downloaded") && l.contains("example.com")) {
            break;
        }
        assert!(Instant::now() < until, "{log:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn taplo_checks_cargo_and_pyproject_offline() {
    let root = temp_project("taplo");
    let Some((client, events)) = start_bundled(TOML, "taplo", &root) else { return };
    // Cargo.toml: a wrong type found; the lints' own schemas (inlined) accept what's right.
    let cargo = "[package]\nname = \"x\"\nversion = 1\nedition = \"2021\"\n\n[lints.clippy]\nall = \"warn\"\n\n[dependencies]\nserde = \"1\"\n";
    let doc = open_file(&client, &root, "Cargo.toml", "toml", cargo);
    let found = diagnostics_saying(&events, "Cargo.toml", |d| d.line == 2);
    assert!(found.iter().all(|d| d.line == 2), "{found:?}");
    let at = cargo.find("edition").unwrap() + 1;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some(), "the schema's description of `edition`");
    // pyproject.toml: a tool's section, from a schema it refers to.
    let pyproject =
        "[project]\nname = \"x\"\nversion = \"1\"\n\n[tool.ruff]\nline-length = \"long\"\n";
    open_file(&client, &root, "pyproject.toml", "toml", pyproject);
    diagnostics_saying(&events, "pyproject.toml", |d| d.line == 5);
    // A syntax error, without any schema.
    open_file(&client, &root, "plain.toml", "toml", "a = \n");
    diagnostics_saying(&events, "plain.toml", |d| d.line == 0);
    // Nothing fetched: the one request it makes before it's configured (SchemaStore's
    // catalog, at `initialize`) never leaves the machine — its proxy is a closed local port.
    let (log, _) = client.status().log_lines();
    for line in log.iter().filter(|l| l.contains("https://")) {
        assert!(line.contains("Connection refused") || line.contains("os error"), "{line}");
    }
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_dockerfile_server_answers_dockerfiles() {
    let root = temp_project("dockerfile");
    let Some((client, events)) = start_bundled(DOCKERFILE, "dockerfile-language-server", &root)
    else {
        return;
    };
    let source = "FROM node:22 AS build\nRUN npm ci\nMAINTAINER someone\nFROM nginx\nCOPY --from=build /app /usr/share\nWOR\n";
    let doc = open_file(&client, &root, "Dockerfile", "dockerfile", source);
    // MAINTAINER is deprecated.
    diagnostics_saying(&events, "Dockerfile", |d| d.line == 2);
    let at = source.find("WOR\n").unwrap() + 3;
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    assert!(
        completion.items.iter().any(|i| i.item.label.starts_with("WORKDIR")),
        "{:?}",
        completion.items.iter().take(5).collect::<Vec<_>>()
    );
    let at = source.find("RUN").unwrap() + 1;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some(), "the instruction's documentation");
    // The stage a `--from` names.
    let at = source.find("=build").unwrap() + 2;
    let definition = client.definition(doc, at).wait(WAIT).unwrap();
    assert!(definition.iter().any(|l| l.range.start < 21), "{definition:?}");
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

const PHP: &str = include_str!("../../../../../modules/languages/php/data/language-servers.toml");

/// A small Composer project: a model with a promoted, typed property and a service using
/// it.
fn php_project(name: &str) -> PathBuf {
    let root = temp_project(name);
    std::fs::create_dir_all(root.join("src/Model")).unwrap();
    std::fs::write(root.join("composer.json"), r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#)
        .unwrap();
    std::fs::write(
        root.join("src/Model/Address.php"),
        "<?php\n\nnamespace App\\Model;\n\nfinal class Address\n{\n    public function __construct(public string $city) {}\n\n    public function label(): string\n    {\n        return $this->city;\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/Model/User.php"),
        "<?php\n\nnamespace App\\Model;\n\nfinal class User\n{\n    public function __construct(public Address $address) {}\n\n    /** @deprecated use label() */\n    public function old(): void {}\n}\n",
    )
    .unwrap();
    root
}

/// phpactor, from the PHP module's description as it is on `os`, with a home of its own (it
/// reads `~/.config/phpactor`: never the real one in a test).
fn start_phpactor(os: &str, root: &Path) -> Option<(Client, Arc<Mutex<Vec<Event>>>)> {
    if bundle()?.php().is_none() || bundle()?.server("phpactor").is_none() {
        eprintln!("PHP / phpactor aren't in target/lsp: skipped (cargo xtask lsp-bundle)");
        return None;
    }
    let mut spec = spec::parse_for(PHP, "test", os)
        .unwrap()
        .into_iter()
        .find(|s| s.name == "phpactor")
        .unwrap();
    let home = root.join(".home");
    for key in ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME"] {
        spec.env.insert(key.into(), home.to_string_lossy().into_owned());
    }
    start_spec(spec, root)
}

/// What phpactor answers in the service (its own inference, not the built-in engine's).
fn phpactor_answers(client: &Client, events: &Mutex<Vec<Event>>, root: &Path) {
    let source = "<?php\n\nnamespace App;\n\nuse App\\Model\\User;\nuse App\\Model\\Missing;\n\nfinal class Service\n{\n    public function city(User $user): string\n    {\n        $address = $user->address;\n        $user->old();\n        return $address->\n    }\n}\n";
    let doc = open_file(client, root, "src/Service.php", "php", source);
    // Its index made (its progress said, then over): a project without `vendor/` has its
    // classes found through it. What it couldn't find before is found again at the next edit.
    let until = Instant::now() + WAIT;
    let progress = || client.status().snapshot(Instant::now()).progress;
    while progress().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    while progress().is_some() {
        assert!(Instant::now() < until, "still indexing: {:?}", progress());
        std::thread::sleep(Duration::from_millis(20));
    }
    client.change(doc, 2, TextBuffer::new(source).snapshot());
    // The promoted property's type: what no built-in completion knew (C02 of the comparison).
    let at = source.find("$address->\n").unwrap() + "$address->".len();
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    let labels: Vec<&str> = completion.items.iter().map(|i| i.item.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("label")),
        "{labels:?} {:?}",
        client.status().log_lines().0
    );
    assert!(labels.iter().any(|l| l.starts_with("city")), "{labels:?}");
    // A variable's type on hover (H03).
    let at = source.find("$address = ").unwrap() + 2;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some_and(|h| format!("{h:?}").contains("Address")), "the variable's type");
    // The declaration of a promoted property, in another file.
    let at = source.find("->address").unwrap() + 3;
    let definition = client.definition(doc, at).wait(WAIT).unwrap();
    assert!(definition.iter().any(|l| l.path.ends_with("User.php")), "{definition:?}");
    // Its diagnostics: the deprecated call — and none of what the built-in engine does (an
    // unused import: `Missing`) or that would be noise.
    let found = diagnostics_saying(events, "Service.php", |d| d.message.contains("deprecated"));
    assert!(found.iter().all(|d| !d.message.contains("\"User\" not found")), "{found:?}");
    assert!(found.iter().all(|d| !d.message.contains("imported but not used")), "{found:?}");
    assert!(found.iter().all(|d| !d.message.contains("docblock")), "{found:?}");
    assert!(found.iter().all(|d| !d.message.contains("Parse error")), "{found:?}");
}

#[test]
fn phpactor_answers_php_on_the_bundled_php() {
    let root = php_project("phpactor");
    let Some((client, events)) = start_phpactor(std::env::consts::OS, &root) else { return };
    phpactor_answers(&client, &events, &root);
    // One process (no workers left, no outsourced diagnostics, no `php -l`), its caches in
    // its own folder of the project's data, nothing in the project's own files.
    assert!(client.status().snapshot(Instant::now()).pid.is_some());
    let cache = spec::cache_dir(&root, "phpactor");
    assert!(cache.join("phpactor/index").is_dir(), "its index in {}", cache.display());
    assert!(!root.join(".phpactor.json").exists());
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

/// Windows' description (phpactor over a local port, as an editor extension starts it
/// there) works the same — tried where PHP can select on pipes too.
#[test]
fn phpactor_answers_over_a_local_port() {
    let root = php_project("phpactor-tcp");
    let windows = spec::parse_for(PHP, "test", "windows")
        .unwrap()
        .into_iter()
        .find(|s| s.name == "phpactor")
        .unwrap();
    assert_eq!(windows.transport, spec::Transport::Tcp);
    assert!(windows.args.iter().any(|a| a.contains("${port}")));
    let Some((client, events)) = start_phpactor("windows", &root) else { return };
    phpactor_answers(&client, &events, &root);
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}

/// intelephense, installed where `EDITOR_TEST_INTELEPHENSE` says (its package's folder: never
/// installed by a test, never the user's), from the PHP module's description, with a home of
/// its own (it reads `~/intelephense/licence.txt`: never the real one in a test).
fn start_intelephense(root: &Path) -> Option<(Client, Arc<Mutex<Vec<Event>>>)> {
    let Some(package) = std::env::var_os("EDITOR_TEST_INTELEPHENSE").map(PathBuf::from) else {
        eprintln!("EDITOR_TEST_INTELEPHENSE isn't set: skipped");
        return None;
    };
    let mut spec =
        spec::parse(PHP, "test").unwrap().into_iter().find(|s| s.name == "intelephense")?;
    let install = spec.install.clone()?;
    assert!(
        crate::install::package_at(&package, &install).is_some(),
        "not intelephense: {package:?}"
    );
    let home = root.join(".home");
    std::fs::create_dir_all(&home).unwrap();
    for key in ["HOME", "USERPROFILE"] {
        spec.env.insert(key.into(), home.to_string_lossy().into_owned());
    }
    start_installed(spec, root, Some(package))
}

#[test]
fn intelephense_answers_php_once_installed() {
    let root = php_project("intelephense");
    let Some((client, events)) = start_intelephense(&root) else { return };
    let source = "<?php\n\nnamespace App;\n\nuse App\\Model\\User;\n\nfinal class Service\n{\n    public function city(User $user): string\n    {\n        $address = $user->address;\n        $user->old();\n        return $address->\n    }\n}\n";
    let doc = open_file(&client, &root, "src/Service.php", "php", source);
    // Its index made (its progress said, then over).
    let until = Instant::now() + WAIT;
    let progress = || client.status().snapshot(Instant::now()).progress;
    let said = Instant::now() + Duration::from_secs(5);
    while progress().is_none() && Instant::now() < said {
        std::thread::sleep(Duration::from_millis(10));
    }
    while progress().is_some() {
        assert!(Instant::now() < until, "still indexing: {:?}", progress());
        std::thread::sleep(Duration::from_millis(20));
    }
    let at = source.find("$address->\n").unwrap() + "$address->".len();
    let completion = client.completion(doc, at).wait(WAIT).unwrap();
    let labels: Vec<&str> = completion.items.iter().map(|i| i.item.label.as_str()).collect();
    assert!(labels.iter().any(|l| l.starts_with("label")), "{labels:?}");
    assert!(labels.iter().any(|l| l.starts_with("city")), "{labels:?}");
    let at = source.find("$address = ").unwrap() + 2;
    let hover = client.hover(doc, at, 0..0).wait(WAIT).flatten();
    assert!(hover.is_some_and(|h| format!("{h:?}").contains("Address")), "the variable's type");
    let at = source.find("->address").unwrap() + 3;
    let definition = client.definition(doc, at).wait(WAIT).unwrap();
    assert!(definition.iter().any(|l| l.path.ends_with("User.php")), "{definition:?}");
    // Whole again: its diagnostics — the deprecated call, nothing undefined.
    let whole = source.replace("$address->\n", "$address->city;\n");
    client.change(doc, 2, TextBuffer::new(&whole).snapshot());
    let found = diagnostics_saying(&events, "Service.php", |d| d.message.contains("deprecated"));
    assert!(found.iter().all(|d| !d.message.contains("Undefined")), "{found:?}");
    // Without a licence key: no rename (a premium feature) — the built-in engine's.
    assert!(!client.status().capabilities().supports(crate::status::Feature::Rename));
    assert!(client.status().capabilities().supports(crate::status::Feature::References));
    // Its index in its folder of the project's data, nothing in the project's own files.
    let cache = spec::cache_dir(&root, "intelephense");
    assert!(std::fs::read_dir(&cache).is_ok_and(|mut d| d.next().is_some()), "{cache:?}");
    drop(client);
    let _ = std::fs::remove_dir_all(root);
}
