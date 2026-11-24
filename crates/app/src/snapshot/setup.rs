//! What is set before the screen is drawn: the fake language servers and downloads, the
//! secrets kept in memory, the workbench brought to the state the variables ask for.

use super::*;

/// Before anything opens: the language servers are never the real bundled ones (what they
/// say varies); `LSP` describes the fake one (this binary, `--fake-language-server`) for
/// TypeScript, and for CSS / SCSS as the CSS server is described (the built-in engine
/// keeping the deprecations), in a file of the temporary folder (never the user's).
pub fn before_open() {
    // The MCP server's token in memory (never the user's keychain).
    mcp_server::secret::use_secrets(std::sync::Arc::new(SnapshotSecrets::default()));
    // Measurements (`REAL_LSP=1`): the servers bundled beside the binary, as the app has them.
    if let Some(seconds) = parsed::<u64>("IDLE") {
        language_servers::idle_after(Duration::from_secs(seconds));
    }
    if flag("REAL_LSP") {
        return;
    }
    language_servers::use_bundle(None);
    php_engines();
    downloads();
    if var("LSP").is_none() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let file =
        std::env::temp_dir().join(format!("snapshot-language-servers-{}.toml", std::process::id()));
    let text = format!(
        "[[language-servers]]\nname = \"fake\"\nlanguages = [\"typescript\"]\ncommand = {exe:?}\nargs = [\"--fake-language-server\"]\n\n[[language-servers]]\nname = \"fake-css\"\nlanguages = [\"css\", \"scss\"]\ncommand = {exe:?}\nargs = [\"--fake-language-server\"]\nbuilt-in = [\"deprecated\", \"rename\"]\ncompletion-kinds = {{ property = \"attribute\" }}\n\n[[language-servers]]\nname = \"fake-cpp\"\nlanguages = [\"c\", \"cpp\"]\ncommand = {exe:?}\nargs = [\"--fake-language-server\"]\n",
        exe = exe.to_string_lossy()
    );
    if std::fs::write(&file, text).is_ok() {
        language_servers::use_user_file(file);
    }
}

/// The servers downloaded on demand, never in the user's data folder nor from the network:
/// nothing downloadable, or (`LSP_OFFER`) a folder of the temporary folder, files in memory.
fn downloads() {
    let Some(case) = var("LSP_OFFER") else {
        language_servers::use_downloads(None);
        return;
    };
    let dir = std::env::temp_dir().join(format!("snapshot-lsp-downloads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    language_servers::use_downloads(Some(dir.clone()));
    let memory = std::sync::Arc::new(lsp_client::fetch::Memory::default());
    language_servers::use_fetch(
        std::sync::Arc::clone(&memory) as std::sync::Arc<dyn lsp_client::fetch::Fetch>
    );
    let manifest = language_servers::downloads::manifest();
    let platform = language_servers::downloads::platform();
    match case.as_str() {
        "downloading" => {
            // Node's archive stalls at 40%: its progress shown, still going.
            let Some(node) = manifest.named("node") else { return };
            let Some(archive) = node.platforms.get(platform) else { return };
            let at = archive.size * 2 / 5;
            memory.serve(&archive.url, vec![0u8; at as usize + 1]);
            memory.stall_after(Some(at));
            language_servers::downloads::start(vec!["node".into(), "vtsls".into()], None);
            let until = Instant::now() + Duration::from_secs(10);
            while Instant::now() < until
                && !matches!(language_servers::downloads::status("node"), language_servers::downloads::Status::Downloading(p) if p.received >= at)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        "parts" => {
            // Downloaded already (their markers: what the list reads).
            for name in ["node", "php", "phpactor", "vtsls"] {
                let Some(c) = manifest.named(name) else { continue };
                let folder = lsp_client::store::path_in(&dir, &c.folder());
                let _ = std::fs::create_dir_all(&folder);
                let marker = lsp_client::store::Marker {
                    name: c.name.clone(),
                    version: c.version.clone(),
                    fingerprint: c.fingerprint(platform),
                    size: c.size(platform) * 3,
                    platform: platform.to_owned(),
                };
                let _ = lsp_client::store::write_marker(&folder, &marker);
                if let Some(bin) = c.bin(platform) {
                    let program = lsp_client::store::path_in(&folder, bin);
                    let _ = std::fs::create_dir_all(program.parent().unwrap_or(&folder));
                    let _ = std::fs::write(program, "");
                }
            }
            language_servers::use_downloads(Some(dir));
        }
        _ => {}
    }
}

/// The servers the user installs (intelephense), never the user's: installed in a folder
/// of the temporary folder or not, secrets in memory, a licence file there or not; with
/// `LSP_PHP` (`missing`, `installed`: a licence file found, `licensed`: a key kept) the
/// settings' language servers block shows PHP.
fn php_engines() {
    let dir = std::env::temp_dir().join("snapshot-lsp-installs");
    let _ = std::fs::remove_dir_all(&dir);
    let case = var("LSP_PHP");
    let secrets = std::sync::Arc::new(SnapshotSecrets::default());
    let licence = dir.join("intelephense").join("licence.txt");
    if matches!(case.as_deref(), Some("installed" | "licensed")) {
        let package = dir.join("installed").join("intelephense").join("1.18.5");
        let _ = std::fs::create_dir_all(package.join("lib"));
        let _ = std::fs::write(
            package.join("package.json"),
            r#"{"name": "intelephense", "version": "1.18.5"}"#,
        );
        let _ = std::fs::write(package.join("lib").join("intelephense.js"), "");
        let _ = std::fs::create_dir_all(dir.join("intelephense"));
        let _ = std::fs::write(&licence, "SNAPSHOT-KEY\n");
    }
    if case.as_deref() == Some("licensed") {
        let _ = kernel::SecretStore::set(&*secrets, "intelephense-licence", "SNAPSHOT-KEY");
    }
    language_servers::use_install_dir(dir.join("installed"));
    language_servers::use_secrets(secrets);
    language_servers::use_licence_files(vec![licence]);
    if case.is_some() {
        language_servers::wait_known(Duration::from_secs(5));
    }
}

/// Secrets in memory (the snapshots never touch the keychain).
#[derive(Default)]
struct SnapshotSecrets(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl kernel::SecretStore for SnapshotSecrets {
    fn get(&self, id: &str) -> Option<kernel::Secret> {
        self.0.lock().ok()?.get(id).map(|s| kernel::Secret::new(s.as_str()))
    }

    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert(id.into(), secret.into());
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(id);
        Ok(())
    }
}

/// The app-level state of the snapshot (theme, theme studio, settings), before rendering.
pub fn prepare(
    options: &Options,
    workbench: &mut Workbench,
    themes: &mut Themes,
) -> anyhow::Result<()> {
    if let Some(theme) = &options.theme
        && !themes.select(theme)
    {
        anyhow::bail!("theme `{theme}` does not exist");
    }
    if let Some(family) = &options.themed {
        let dark = themes.resolved().is_some_and(|t| t.appearance == theming::Appearance::Dark);
        let variant = format!("{family}-{}", if dark { "dark" } else { "light" });
        if !themes.select(&variant) {
            anyhow::bail!("theme `{variant}` does not exist");
        }
    }
    if let Some(preset) = &options.my_decorations {
        use theme_decorations::settings::{Mine, Overlay, set_overlay};
        use theming::decorations::{FieldValue, LayerKind, LayerSpec};
        // A preset's id, or one effect of a kind (`matrix`), its style (`synthwave:city`),
        // where it shows (`@panels`).
        let (name, zone) = preset.split_once('@').unwrap_or((preset.as_str(), ""));
        let (name, style) = name.split_once(':').unwrap_or((name, ""));
        // With a zone it's always an effect (`synthwave@panels`); without, a preset first.
        let mine = if zone.is_empty() && style.is_empty() { Mine::parse(name) } else { None };
        let overlay = match (mine, LayerKind::parse(name)) {
            (Some(mine), _) => Overlay { mine, ..Overlay::default() },
            (None, Some(kind)) => {
                let mut layer = LayerSpec::new(kind);
                if !zone.is_empty()
                    && layer.set("zone", &FieldValue::Text(zone.to_owned())).is_err()
                {
                    anyhow::bail!("no zone `{zone}` for `{name}`");
                }
                if !style.is_empty()
                    && layer.set("style", &FieldValue::Text(style.to_owned())).is_err()
                {
                    anyhow::bail!("no style `{style}` for `{name}`");
                }
                Overlay { mine: Mine::Custom, custom: vec![layer], ..Overlay::default() }
            }
            (None, None) => anyhow::bail!("no decorations preset or effect `{preset}`"),
        };
        set_overlay(overlay);
    }
    // The theme's decorations as a still frame (the same every run).
    workbench.set_decorations_window(false, true, true);
    // Word wrap on (`WRAP=1`): every view told so at its first frame, as the setting would.
    if flag("WRAP") {
        crate::text_options::set(ui_workbench::TextOptions { word_wrap: true });
    }
    // The user's backdrop over the theme's (`BACKDROP=glass:6:20`: the style, the blur in
    // pixels, the tint in percent; each optional).
    if let Some(backdrop) = var("BACKDROP") {
        crate::backdrop::set_own(crate::backdrop::parse_own(&backdrop));
    }
    workbench.set_palette(themes.palette());
    if let Some(theme) = themes.resolved() {
        crate::theme_options::set_active(&theme);
        workbench.set_theme(theme);
    }
    if options.studio {
        let mut input = themes.studio_input(themes.current_id());
        // The names as their ids (what the snapshots always showed).
        for theme in &mut input.themes {
            theme.name = theme.id.clone();
        }
        workbench.set_theme_source(std::rc::Rc::new(move |_| input.clone()));
        workbench.run_command(workbench_model::THEME_STUDIO);
    }
    if let Some(steps) = options.settings {
        workbench.open_settings(crate::app::settings_model(themes));
        if let Some(query) = &options.settings_query {
            wait_for_semantic_model(Duration::from_secs(10));
            workbench.take_settings_embedder();
            workbench.settings_query(query);
        }
        workbench.settings_step(steps);
        if let Some(page) = &options.settings_page {
            workbench.settings_show_page(page);
        }
        if let Some(language) = &options.settings_language {
            workbench.select_settings_language(language);
        }
        if options.settings_advanced {
            workbench.settings_open_advanced();
        }
        // The modal's appear animation finishes before the capture.
        std::thread::sleep(Duration::from_millis(220));
    }
    Ok(())
}

/// Waits (up to `timeout`) for the semantic model, if it is installed.
pub(super) fn wait_for_semantic_model(timeout: Duration) {
    semantic::load(None);
    let deadline = Instant::now() + timeout;
    while semantic::is_installed() && semantic::model().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}
