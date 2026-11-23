//! Development tools for the workspace.
//!
//! `cargo xtask check-deps` enforces the dependency rules from
//! docs/plan/11-conventii-calitate.md: each crate's layer is inferred from its path,
//! and internal and external dependencies must follow the table below; then the rules read
//! in the sources (`app_sources`: the app's; `conventions`: every crate's).

mod app_icon;
mod app_sources;
mod architecture;
mod module_coupling;
// Shared with the app's language-server downloads: one source for both.
#[allow(dead_code)]
#[path = "../../crates/infra/code/lsp-client/src/assemble.rs"]
mod assemble;
mod conventions;
mod dts;
mod emoji_data;
mod icon_packs;
mod lsp_bundle;
mod lsp_manifest;
mod lsp_update;
#[allow(dead_code)]
#[path = "../../crates/infra/files/pinned-downloads/src/paths.rs"]
mod paths;
mod php_stubs;
mod release;
mod sql_data;
mod ui_conventions;
mod web_data;
use std::path::Path;
use std::process::{Command, ExitCode};

use serde_json::Value;

use architecture::Architecture;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("check-deps") => match check_deps() {
            Ok(0) => {
                println!("check-deps: OK");
                ExitCode::SUCCESS
            }
            Ok(n) => {
                eprintln!("check-deps: {n} violations");
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("check-deps: {e}");
                ExitCode::FAILURE
            }
        },
        Some("php-stubs") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            let pull = std::env::args().any(|a| a == "--pull");
            let update = std::env::args().any(|a| a == "--update");
            // A new version comes with a new image: updating implies pulling.
            match php_stubs::generate(root, update, pull || update) {
                Ok(done) => {
                    println!("php-stubs: {done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("php-stubs: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("web-data") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            match web_data::generate(root) {
                Ok(done) => {
                    println!("web-data: {done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("web-data: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("emoji-data") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            match emoji_data::generate(root) {
                Ok(done) => {
                    println!("emoji-data: {done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("emoji-data: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("sql-data") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            match sql_data::generate(root) {
                Ok(done) => {
                    println!("sql-data: {done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("sql-data: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("icon-packs") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            let check = std::env::args().any(|a| a == "--check");
            match icon_packs::generate(root, check) {
                Ok(done) => {
                    println!("icon-packs: {done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("icon-packs: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("lsp-bundle") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            let args: Vec<String> = std::env::args().collect();
            let value = |flag: &str| {
                args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
            };
            let out = value("--out").map(std::path::PathBuf::from);
            match lsp_bundle::bundle(root, value("--platform").as_deref(), out.as_deref()) {
                Ok(done) => {
                    println!("lsp-bundle:\n{done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("lsp-bundle: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("lsp-manifest") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            let check = std::env::args().any(|a| a == "--check");
            match lsp_manifest::generate(root, check) {
                Ok(done) => {
                    println!("lsp-manifest: {done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("lsp-manifest: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("lsp-update") => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."));
            let check = std::env::args().any(|a| a == "--check");
            let major = std::env::args().any(|a| a == "--major");
            // `--root <dir>`: another copy of the workspace's packaging (a trial run).
            let args: Vec<String> = std::env::args().collect();
            let other = args.iter().position(|a| a == "--root").and_then(|i| args.get(i + 1));
            let root = other.map_or(root, Path::new);
            match lsp_update::update(root, check, major) {
                Ok(done) => {
                    println!("lsp-update:\n{done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("lsp-update: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("icon") => match app_icon::render(
            Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new(".")),
        ) {
            Ok(dir) => {
                println!("icon: {}", dir.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("icon: {e}");
                ExitCode::FAILURE
            }
        },
        // The version a release carries (the workspace's): the workflow checks its tag by it.
        Some("release-version") => match workspace_version(&workspace_root()) {
            Ok(version) => {
                println!("{version}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("release-version: {e}");
                ExitCode::FAILURE
            }
        },
        Some("release-key") => {
            let root = workspace_root();
            let args: Vec<String> = std::env::args().collect();
            let out = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1));
            let out =
                out.map_or_else(|| root.join("target/release-signing-key.secret"), Into::into);
            match release::make_key(&root, &out) {
                Ok(done) => {
                    println!("release-key:\n{done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("release-key: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("release-manifest") => {
            let root = workspace_root();
            let args: Vec<String> = std::env::args().collect();
            let dist = args.iter().position(|a| a == "--dist").and_then(|i| args.get(i + 1));
            let dist = dist.map_or_else(|| root.join("dist"), Into::into);
            let signed = args.iter().any(|a| a == "--signed");
            let secret = std::env::var("RELEASE_SIGNING_KEY").ok();
            let made = workspace_version(&root).and_then(|version| {
                let id = brand_id(&root)?;
                release::write_manifest(
                    &root,
                    &dist,
                    &id,
                    &version,
                    &today(),
                    secret.as_deref(),
                    signed,
                )
            });
            match made {
                Ok(done) => {
                    println!("release-manifest:\n{done}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("release-manifest: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!(
                "usage: cargo xtask check-deps | icon | icon-packs [--check] | lsp-bundle [--platform <p>|all] [--out <dir>] | lsp-manifest [--check] | lsp-update [--check] [--major] [--root <dir>] | php-stubs [--update] [--pull] | emoji-data | sql-data | web-data | release-version | release-key [--out <file>] | release-manifest [--dist <dir>] [--signed]"
            );
            ExitCode::FAILURE
        }
    }
}

struct Package {
    name: String,
    layer: String,
    /// (dependency name, is path dependency)
    deps: Vec<(String, bool)>,
}

fn check_deps() -> Result<usize, String> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("can't run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let meta: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("invalid output from cargo metadata: {e}"))?;

    let root = meta["workspace_root"].as_str().ok_or("workspace_root is missing")?;
    // The app's binary is named after `brand.toml`'s `id` (packages and scripts rely on it).
    let binary_mismatch = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|p| p["targets"].as_array().into_iter().flatten())
        .filter(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "bin")))
        .filter(|t| t["src_path"].as_str().is_some_and(|p| p.ends_with("crates/app/src/main.rs")))
        .any(|t| t["name"].as_str() != Some(kernel::brand::ID));
    if binary_mismatch {
        eprintln!("the app's binary isn't named `{}` (see brand.toml)", kernel::brand::ID);
    }
    let rules = Architecture::read(Path::new(root))?;
    let packages = parse_packages(&meta, Path::new(root), &rules)?;
    let brand_violations = usize::from(binary_mismatch);
    let mut violations = find_violations(&packages, &rules);
    violations.extend(app_module_references(&meta, Path::new(root)));
    violations.extend(module_coupling::violations(&module_coupling::read_sources(Path::new(root))));
    let locales = Path::new(root).join("crates/domain/core/i18n/locales/en.toml");
    let keys = conventions::locale_keys(&std::fs::read_to_string(&locales).unwrap_or_default());
    violations.extend(conventions::violations(Path::new(root), kernel::brand::ID, &keys));
    violations.extend(ui_conventions::violations(Path::new(root)));
    for v in &violations {
        eprintln!("  ✗ {v}");
    }
    Ok(violations.len() + brand_violations)
}

/// The rules over the app's sources (`app_sources`): the modules' names, commands and
/// threads, and the workbench's adapters.
fn app_module_references(meta: &Value, root: &Path) -> Vec<String> {
    // The modules' library names (`project_tree`).
    let modules: Vec<String> = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            p["manifest_path"]
                .as_str()
                .is_some_and(|m| m.replace('\\', "/").contains("/crates/modules/"))
        })
        .flat_map(|p| p["targets"].as_array().into_iter().flatten())
        .filter(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "lib")))
        .filter_map(|t| t["name"].as_str().map(|n| n.replace('-', "_")))
        .collect();
    let sources = app_sources::read_sources(&root.join("crates/app/src"));
    let names = app_sources::ModuleNames::read(&root.join("crates/modules"));
    app_sources::violations(&sources, &modules, &names)
}

fn parse_packages(meta: &Value, root: &Path, rules: &Architecture) -> Result<Vec<Package>, String> {
    let mut packages = Vec::new();
    for pkg in meta["packages"].as_array().ok_or("packages is missing")? {
        let name = pkg["name"].as_str().unwrap_or_default().to_owned();
        let manifest = Path::new(pkg["manifest_path"].as_str().unwrap_or_default());
        let rel_dir = manifest
            .parent()
            .and_then(|dir| dir.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .ok_or_else(|| format!("{name}: a path outside the workspace"))?;
        let layer = rules
            .layer_of(&rel_dir)
            .ok_or_else(|| format!("{name} ({rel_dir}): in no layer of architecture.toml"))?
            .name
            .clone();
        let deps = pkg["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            // Normal dependencies only; dev/build dependencies are unrestricted (e.g. proptest).
            .filter(|d| d["kind"].is_null())
            .map(|d| (d["name"].as_str().unwrap_or_default().to_owned(), d["path"].is_string()))
            .collect();
        packages.push(Package { name, layer, deps });
    }
    Ok(packages)
}

fn find_violations(packages: &[Package], rules: &Architecture) -> Vec<String> {
    let layer_of = |name: &str| packages.iter().find(|p| p.name == name).map(|p| p.layer.as_str());
    let mut violations = Vec::new();

    for pkg in packages {
        let layer = rules.layer(&pkg.layer);
        for (dep, is_path) in &pkg.deps {
            // The app's own screens change often: only the app builds on them (modules use
            // `ui-components`), so changing a screen recompiles only the app.
            if *dep == rules.app_screens && !rules.app_screens_users.contains(&pkg.layer) {
                violations.push(format!(
                    "{} ({}) may not depend on {dep} (the app's screens): use ui-components",
                    pkg.name, pkg.layer
                ));
                continue;
            }
            match (is_path, layer_of(dep)) {
                (true, Some(dep_layer)) => {
                    if layer
                        .and_then(|l| l.may_use.as_ref())
                        .is_some_and(|allowed| !allowed.iter().any(|a| a == dep_layer))
                    {
                        violations.push(format!(
                            "{} ({}) may not depend on {dep} ({dep_layer})",
                            pkg.name, pkg.layer
                        ));
                    } else if pkg.layer == "module"
                        && dep_layer == "infra"
                        && !rules.module_may_use(&pkg.name, dep)
                    {
                        violations.push(format!(
                            "{} (module) may not depend on {dep}: not in the modules' \
                             infrastructure in architecture.toml",
                            pkg.name
                        ));
                    }
                }
                _ => {
                    if layer
                        .and_then(|l| l.external.as_ref())
                        .is_some_and(|allowed| !allowed.contains(dep))
                    {
                        violations.push(format!(
                            "{} ({}): external dependency `{dep}` isn't allowed",
                            pkg.name, pkg.layer
                        ));
                    }
                }
            }
        }
    }
    violations
}

#[cfg(test)]
#[path = "../tests/unit/bin.rs"]
mod tests;

/// The workspace's folder (the one above `xtask`).
fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new(".")).to_path_buf()
}

/// `[workspace.package] version` of the workspace's `Cargo.toml`.
fn workspace_version(root: &Path) -> Result<String, String> {
    let text =
        std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let doc: toml::Table = text.parse().map_err(|e| format!("Cargo.toml: {e}"))?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "no [workspace.package] version in Cargo.toml".into())
}

/// The app's `id` in `brand.toml` (its files are named by it).
fn brand_id(root: &Path) -> Result<String, String> {
    let text =
        std::fs::read_to_string(root.join("brand.toml")).map_err(|e| format!("brand.toml: {e}"))?;
    let doc: toml::Table = text.parse().map_err(|e| format!("brand.toml: {e}"))?;
    doc.get("id")
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "no id in brand.toml".into())
}

/// Today's date (UTC), `YYYY-MM-DD`.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil_date(i64::try_from(secs / 86_400).unwrap_or(0))
}

/// The date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
