//! What the module's tests share: a fake server described in a user file of a temporary
//! folder (never the user's), started in memory, and a project to open documents in. The
//! module's state is global (its settings, its catalog): the tests that touch it hold
//! [`Harness`]'s lock.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

use document::TextBuffer;
use document::events::{DocumentChanged, DocumentClosed, DocumentOpened};
use kernel::{DocumentId, EventBus, ProjectId, Subscription};
use lsp_client::fake::{self, Behaviour, Seen};
use workbench_model::{DiagnosticsFound, ProjectContext};

/// The fake servers started since the harness was made, and what they saw.
#[derive(Default)]
pub struct Fakes {
    pub seen: Arc<Mutex<Seen>>,
    pub launches: Arc<Mutex<usize>>,
    /// The projects a server was made for (by their root): a test asks about its own, never
    /// counting a start the test before it left behind and that came late.
    pub roots: Arc<Mutex<Vec<PathBuf>>>,
    pub behaviour: Behaviour,
}

static FAKES: Mutex<Option<Arc<Fakes>>> = Mutex::new(None);

/// The temporary folder the tests' user file and projects are in.
pub fn temp() -> PathBuf {
    std::env::temp_dir().join(format!("lsp-module-tests-{}", std::process::id()))
}

/// The package the fake registry serves (`installable-server` 1.0.0, as npm publishes
/// it: gzipped, everything under `package/`).
pub fn tarball() -> Vec<u8> {
    use std::io::Write;
    fn entry(out: &mut Vec<u8>, name: &str, body: &[u8]) {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..107].copy_from_slice(b"0000644");
        header[124..135].copy_from_slice(format!("{:011o}", body.len()).as_bytes());
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        out.extend_from_slice(&header);
        out.extend_from_slice(body);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    let mut tar = Vec::new();
    entry(
        &mut tar,
        "package/package.json",
        br#"{"name": "installable-server", "version": "1.0.0"}"#,
    );
    entry(&mut tar, "package/lib/server.js", b"// the server");
    tar.extend_from_slice(&[0u8; 1024]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

/// A registry in memory (never the network): what it was asked, what it answers.
#[derive(Default)]
pub struct Registry {
    pub asked: Mutex<Vec<String>>,
    pub body: Mutex<Vec<u8>>,
}

impl lsp_client::fetch::Fetch for Registry {
    fn open(
        &self,
        url: &str,
        _from: u64,
    ) -> Result<lsp_client::fetch::Body, lsp_client::fetch::FetchError> {
        self.asked.lock().unwrap().push(url.to_owned());
        let body = self.body.lock().unwrap().clone();
        Ok(lsp_client::fetch::Body {
            from: 0,
            total: Some(body.len() as u64),
            reader: Box::new(std::io::Cursor::new(body)),
        })
    }
}

/// The fake registry the module downloads from in these tests.
pub fn registry() -> Arc<Registry> {
    static REGISTRY: std::sync::OnceLock<Arc<Registry>> = std::sync::OnceLock::new();
    Arc::clone(REGISTRY.get_or_init(Arc::default))
}

/// Secrets in memory (never the keychain).
#[derive(Default)]
pub struct Secrets(pub Mutex<std::collections::HashMap<String, String>>);

impl kernel::SecretStore for Secrets {
    fn get(&self, id: &str) -> Option<kernel::Secret> {
        self.0.lock().unwrap().get(id).map(|s| kernel::Secret::new(s.as_str()))
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(id.into(), secret.into());
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

/// The secrets the module keeps in these tests.
pub fn secrets() -> Arc<Secrets> {
    static SECRETS: std::sync::OnceLock<Arc<Secrets>> = std::sync::OnceLock::new();
    Arc::clone(SECRETS.get_or_init(Arc::default))
}

/// The installs start from nothing, in a folder of their own (`name`'s).
pub fn fresh_installs(name: &str) -> PathBuf {
    let dir = temp().join(format!("installs-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    crate::install::use_install_dir(dir.clone());
    crate::install::use_fetch(registry());
    *registry().body.lock().unwrap() = tarball();
    registry().asked.lock().unwrap().clear();
    secrets().0.lock().unwrap().clear();
    crate::licence::use_secrets(secrets());
    dir
}

/// The user file describing the fake servers (`fake`, for TypeScript and TSX; `other`, for
/// TypeScript; `quiet`, for HTML, reporting no diagnostics of its own; `cached`, for Lua,
/// writing in its own folder; `opt-in`, for TOML, the built-in engine's by default;
/// `downloadable`, for YAML, run by Node from a downloaded `yaml-srv` — see
/// [`fresh_downloads`]; for PHP,
/// `php-first`, the built-in engine's by default — as phpactor on Windows — and
/// `installable`, not bundled: installed from the fake registry, given a licence key).
fn describe() {
    // npm's packages, as the JavaScript module says they are.
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["node_modules"],
        unlisted: &["node_modules"],
        generated: &[],
        ..Default::default()
    });
    let dir = temp();
    std::fs::create_dir_all(&dir).unwrap();
    let exe = std::env::current_exe().unwrap();
    let file = dir.join(crate::USER_FILE);
    let text = format!(
        "[[language-servers]]\nname = \"fake\"\nlanguages = [\"typescript\", \"tsx\"]\ncommand = {:?}\nfile-patterns = [\"**/*.ts\", \"**/tsconfig.json\"]\nlanguage-ids = {{ tsx = \"typescriptreact\" }}\n\n[[language-servers]]\nname = \"other\"\nlanguages = [\"typescript\"]\ncommand = {:?}\n\n[[language-servers]]\nname = \"quiet\"\nlanguages = [\"html\"]\ncommand = {:?}\nbuilt-in = [\"diagnostics\"]\n\n[[language-servers]]\nname = \"cached\"\nlanguages = [\"lua\"]\ncommand = {:?}\nargs = [\"--log=${{cache}}/log\"]\n\n[[language-servers]]\nname = \"opt-in\"\nlanguages = [\"toml\"]\ncommand = {:?}\ndefault-engine = \"built-in\"\n\n[[language-servers]]\nname = \"downloadable\"\nlanguages = [\"yaml\"]\nbundle = \"yaml-srv\"\ncommand = \"${{node}}\"\nargs = [\"${{server}}/node_modules/yaml-srv/server.js\", \"--stdio\"]\n",
        exe.to_string_lossy(),
        exe.to_string_lossy(),
        exe.to_string_lossy(),
        exe.to_string_lossy(),
        exe.to_string_lossy()
    );
    let integrity = format!(
        "sha512-{}",
        lsp_client::install::base64(
            ring::digest::digest(&ring::digest::SHA512, &tarball()).as_ref()
        )
    );
    let php = format!(
        "\n[[language-servers]]\nname = \"php-first\"\nlanguages = [\"php\"]\ncommand = {exe:?}\ndefault-engine = \"built-in\"\n\n[[language-servers]]\nname = \"installable\"\nlanguages = [\"php\"]\ncommand = {exe:?}\nargs = [\"${{server}}/lib/server.js\"]\nsecret-options = {{ licenceKey = \"installable-licence\" }}\nshow-messages = true\n[language-servers.install]\nnpm = \"installable-server\"\nversion = \"1.0.0\"\nintegrity = \"{integrity}\"\nentry = \"lib/server.js\"\nsize = 400\n",
        exe = exe.to_string_lossy()
    );
    // A language with two servers it can get (as PHP: one installed by the user first, one
    // downloaded): Markdown here.
    let php = php
        + &format!(
            "\n[[language-servers]]\nname = \"picky-install\"\nlanguages = [\"markdown\"]\ncommand = {exe:?}\nargs = [\"${{server}}/lib/server.js\"]\n[language-servers.install]\nnpm = \"installable-server\"\nversion = \"1.0.0\"\nintegrity = \"{integrity}\"\nentry = \"lib/server.js\"\nsize = 400\n\n[[language-servers]]\nname = \"picky-download\"\nlanguages = [\"markdown\"]\nbundle = \"yaml-srv\"\ncommand = \"${{node}}\"\nargs = [\"${{server}}/node_modules/yaml-srv/server.js\", \"--stdio\"]\n",
            exe = exe.to_string_lossy()
        );
    std::fs::write(&file, text + &php).unwrap();
    // Never the user's: installs in a temporary folder, downloads from the fake registry,
    // secrets in memory, no licence file of the user's.
    crate::install::use_install_dir(dir.join("installs"));
    crate::install::use_fetch(registry());
    crate::licence::use_secrets(secrets());
    crate::licence::use_licence_files(vec![dir.join("no-licence.txt")]);
    crate::use_user_file(file);
    crate::launch_with(Box::new(|_, _, root| {
        let fakes = FAKES.lock().unwrap().clone().unwrap_or_default();
        fakes.roots.lock().unwrap().push(root.to_path_buf());
        let behaviour = fakes.behaviour.clone();
        Box::new(fake::launcher(Arc::clone(&fakes.seen), Arc::clone(&fakes.launches), move |_| {
            behaviour.clone()
        }))
    }));
}

/// One test at a time among those touching what's the process's (the fakes, the licence
/// files): held for as long as the guard lives.
pub fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A test holding the module's state.
pub struct Harness {
    _lock: MutexGuard<'static, ()>,
    pub fakes: Arc<Fakes>,
}

impl Harness {
    pub fn new(behaviour: Behaviour) -> Self {
        static DESCRIBED: Once = Once::new();
        let lock = alone();
        DESCRIBED.call_once(describe);
        let fakes = Arc::new(Fakes { behaviour, ..Fakes::default() });
        *FAKES.lock().unwrap() = Some(Arc::clone(&fakes));
        crate::settings::set(crate::settings::Config {
            idle: None,
            ..crate::settings::Config::default()
        });
        // A test's own waits don't reach the next one.
        crate::projects::left_after(None);
        Self { _lock: lock, fakes }
    }

    pub fn launches(&self) -> usize {
        *self.fakes.launches.lock().unwrap()
    }

    /// The servers made for the project at `root`.
    pub fn made_for(&self, root: &std::path::Path) -> usize {
        self.fakes.roots.lock().unwrap().iter().filter(|r| r.as_path() == root).count()
    }

    pub fn seen<R>(&self, f: impl FnOnce(&Seen) -> R) -> R {
        f(&self.fakes.seen.lock().unwrap())
    }
}

/// An open project, its documents announced on its bus as the editor does.
pub struct Project {
    pub id: ProjectId,
    pub root: PathBuf,
    pub events: EventBus,
    pub subscriptions: Vec<Subscription>,
    pub diagnostics: Arc<Mutex<Vec<DiagnosticsFound>>>,
    _heard: Subscription,
}

impl Project {
    pub fn open(name: &str) -> Self {
        let root = temp().join(name);
        std::fs::create_dir_all(&root).unwrap();
        let events = EventBus::default();
        let id = ProjectId::new();
        let context = ProjectContext {
            marks: Default::default(),
            id,
            root: &root,
            storage: &root,
            events: &events,
            wake: Arc::new(|| {}),
            settings: None,
            language_of: Arc::new(|_: &Path| None),
            quality_tools: Vec::new(),
            text_formatters: Vec::new(),
            notifier: Arc::new(workbench_model::NoNotifier),
        };
        let subscriptions = crate::projects::ProjectServers::attach(&context).1;
        let diagnostics: Arc<Mutex<Vec<DiagnosticsFound>>> = Arc::default();
        let into = Arc::clone(&diagnostics);
        let heard =
            events.subscribe(move |e: &DiagnosticsFound| into.lock().unwrap().push(e.clone()));
        Self { id, root, events, subscriptions, diagnostics, _heard: heard }
    }

    pub fn servers(&self) -> Arc<crate::projects::ProjectServers> {
        crate::projects::project(self.id).unwrap()
    }

    pub fn open_doc(&self, name: &str, language: &str, text: &str) -> DocumentId {
        let document = DocumentId::next();
        self.events.publish(&DocumentOpened {
            document,
            path: self.root.join(name),
            language: language.into(),
            version: 1,
            text: TextBuffer::new(text).snapshot(),
        });
        document
    }

    pub fn change(&self, document: DocumentId, name: &str, version: u64, text: &str) {
        self.events.publish(&DocumentChanged {
            document,
            path: self.root.join(name),
            version,
            text: TextBuffer::new(text).snapshot(),
        });
    }

    pub fn close(&self, document: DocumentId, name: &str) {
        self.events.publish(&DocumentClosed { document, path: self.root.join(name) });
    }
}

/// Waits (up to 5 s) until `done`.
pub fn wait(what: &str, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A tarball (ustar) of `entries` (name, mode, body), gzipped.
pub fn tar_gz(entries: &[(&str, u32, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut tar = Vec::new();
    for (name, mode, body) in entries {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..107].copy_from_slice(format!("{mode:07o}").as_bytes());
        header[124..135].copy_from_slice(format!("{:011o}", body.len()).as_bytes());
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        tar.extend_from_slice(&header);
        tar.extend_from_slice(body);
        tar.resize(tar.len().div_ceil(512) * 512, 0);
    }
    tar.extend_from_slice(&[0u8; 1024]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

/// What the module downloads in these tests: Node (`node` 1.0.0, every platform) and
/// `yaml-srv` 2.0.0 (one npm package), served from memory, into a folder of `name`'s; what
/// was found and dismissed forgotten. The files, and the source.
pub fn fresh_downloads(name: &str) -> (PathBuf, Arc<lsp_client::fetch::Memory>) {
    use lsp_client::downloads::{Archive, Component, Manifest, Package, Role};
    let root = temp().join(format!("downloads-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    let memory = Arc::new(lsp_client::fetch::Memory::default());
    let node = tar_gz(&[
        ("node-v1/bin/node", 0o755, b"#!/bin/sh\n"),
        ("node-v1/node.exe", 0o755, b"MZ"),
        ("node-v1/LICENSE", 0o644, b"MIT"),
    ]);
    let node_url = "https://nodejs.org/dist/v1.0.0/node-v1.tar.gz";
    memory.serve(node_url, node.clone());
    let sha256 =
        lsp_client::install::hex(ring::digest::digest(&ring::digest::SHA256, &node).as_ref());
    let platforms =
        ["macos-arm64", "macos-x64", "linux-x64", "windows-x64", "windows-arm64", "linux-arm64"]
            .into_iter()
            .map(|p| {
                let bin = if p.starts_with("windows") { "node.exe" } else { "bin/node" };
                let archive = Archive {
                    url: node_url.into(),
                    sha256: sha256.clone(),
                    size: node.len() as u64,
                    strip: Some("node-v1".into()),
                    keep: vec![bin.into(), "LICENSE".into()],
                    program: None,
                    bin: Some(bin.into()),
                };
                (p.to_owned(), archive)
            })
            .collect();
    let package = tar_gz(&[
        ("package/package.json", 0o644, br#"{"name": "yaml-srv"}"#),
        ("package/server.js", 0o644, b"// the server"),
    ]);
    let package_url = "https://registry.npmjs.org/yaml-srv/-/yaml-srv-2.0.0.tgz";
    memory.serve(package_url, package.clone());
    let integrity = format!(
        "sha512-{}",
        lsp_client::install::base64(ring::digest::digest(&ring::digest::SHA512, &package).as_ref())
    );
    let manifest = Manifest {
        format: lsp_client::downloads::FORMAT,
        components: vec![
            Component {
                name: "node".into(),
                role: Role::Node,
                version: "1.0.0".into(),
                platforms,
                ..Component::default()
            },
            Component {
                name: "yaml-srv".into(),
                role: Role::Server,
                version: "2.0.0".into(),
                packages: vec![Package {
                    path: "node_modules/yaml-srv".into(),
                    url: package_url.into(),
                    integrity,
                    size: package.len() as u64,
                    ..Package::default()
                }],
                ..Component::default()
            },
        ],
    };
    crate::downloads::use_downloads(Some(root.clone()));
    crate::downloads::use_manifest(manifest);
    crate::install::use_fetch(Arc::clone(&memory) as Arc<dyn lsp_client::fetch::Fetch>);
    // The last test's source let go: a download it left stalled (it failed before letting
    // it go) ends, so no install of this one waits behind it.
    static LAST: Mutex<Option<Arc<lsp_client::fetch::Memory>>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(previous) = last.replace(Arc::clone(&memory)) {
        previous.release();
    }
    drop(last);
    crate::bar::forget_dismissals();
    (root, memory)
}

/// Settings kept in memory.
#[derive(Debug, Default)]
pub struct MemorySettings(Mutex<std::collections::HashMap<String, settings_model::Section>>);

impl settings_model::SettingsStore for MemorySettings {
    fn section(&self, name: &str) -> Option<settings_model::Section> {
        self.0.lock().unwrap().get(name).cloned()
    }

    fn update(
        &self,
        name: &str,
        change: &mut dyn FnMut(&mut settings_model::Section),
    ) -> std::io::Result<()> {
        change(self.0.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}
