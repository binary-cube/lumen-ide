//! `cargo xtask lsp-bundle [--platform <p>|all] [--out <dir>]`: the language servers the app
//! bundles (`packaging/lsp.toml`), put together for a platform — Node from nodejs.org (its
//! SHA-256 checked against the pinned one), the servers installed with `npm ci` from their
//! pinned lockfiles (every package's SHA-512 checked by npm), install scripts off, with
//! their data files (pinned on GitHub, SHA-256 checked; JSON schemas made self-contained
//! where a server would fetch the ones they refer to), and the native servers (clangd,
//! lua-language-server, zls, taplo) from their GitHub releases for the platform (SHA-256
//! checked; macOS's universal programs thinned to the platform's architecture), PHP for the
//! PHP servers (a static PHP CLI from static-php-cli's builds, SHA-256 checked) and the
//! servers that are a release's file (phpactor's PHAR, from GitHub's releases) — into
//!
//! ```text
//! <out>/manifest.toml
//! <out>/node/<version>/bin/node      (node.exe on Windows)
//! <out>/php/<version>/php            (php.exe on Windows)
//! <out>/servers/<server>/<version>/   (package.json, node_modules; a native server's files)
//! ```
//!
//! By default the host's platform, into `target/lsp/` (found there by the app while
//! developing). The downloads are kept in `target/lsp-cache/` (fetched once). `--platform
//! all` makes `<out>/<platform>/` for each; the dist builds (`scripts/build.sh`) ask for
//! theirs.

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(test)]
pub use crate::assemble::resolve_url;
pub use crate::assemble::{MachArch, inline_schema, safe_relative, thin};
pub use crate::paths::pruned;

/// The platforms the manifest knows.
pub const PLATFORMS: [&str; 4] = ["macos-arm64", "macos-x64", "linux-x64", "windows-x64"];

/// The platform this runs on.
pub fn host() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macos-arm64",
        ("macos", _) => "macos-x64",
        ("windows", _) => "windows-x64",
        _ => "linux-x64",
    }
}

/// Makes the bundle(s): `platform` (`None`: the host's; `all`), into `out` (default
/// `target/lsp`). Returns what was made, with the sizes.
pub fn bundle(root: &Path, platform: Option<&str>, out: Option<&Path>) -> Result<String, String> {
    let manifest_text = std::fs::read_to_string(root.join("packaging/lsp.toml"))
        .map_err(|e| format!("packaging/lsp.toml: {e}"))?;
    let manifest: toml::Table =
        toml::from_str(&manifest_text).map_err(|e| format!("packaging/lsp.toml: {e}"))?;
    let out = out.map_or_else(|| root.join("target/lsp"), Path::to_path_buf);
    let cache = root.join("target/lsp-cache");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let platforms: Vec<&str> = match platform {
        None => vec![host()],
        Some("all") => PLATFORMS.to_vec(),
        Some(p) if PLATFORMS.contains(&p) => vec![p],
        Some(p) => return Err(format!("unknown platform {p} (one of {})", PLATFORMS.join(", "))),
    };
    let several = platforms.len() > 1;
    let mut report = Vec::new();
    for p in platforms {
        let dir = if several { out.join(p) } else { out.clone() };
        make(root, &manifest, &cache, p, &dir)?;
        report.push(format!("{p}: {} ({})", dir.display(), megabytes(size(&dir))));
    }
    Ok(report.join("\n"))
}

fn table<'a>(value: &'a toml::Table, key: &str) -> Result<&'a toml::Table, String> {
    value
        .get(key)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("packaging/lsp.toml: no [{key}]"))
}

fn text<'a>(value: &'a toml::Table, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("packaging/lsp.toml: no `{key}`"))
}

/// One platform's bundle into `dir` (made anew).
fn make(
    root: &Path,
    manifest: &toml::Table,
    cache: &Path,
    platform: &str,
    dir: &Path,
) -> Result<(), String> {
    let node = table(manifest, "node")?;
    let version = text(node, "version")?;
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Node: its program and its licence.
    let unpacked = node_unpacked(node, cache, platform)?;
    let windows = platform.starts_with("windows");
    let program = if windows { "node.exe" } else { "bin/node" };
    let node_dir = dir.join("node").join(version);
    copy_file(&unpacked.join(program), &node_dir.join(program))?;
    copy_file(&unpacked.join("LICENSE"), &node_dir.join("LICENSE"))?;
    // PHP, for the PHP servers: its program and its licence.
    let php = match manifest.get("php").and_then(toml::Value::as_table) {
        Some(php) => {
            let php_version = text(php, "version")?;
            let program = if windows { "php.exe" } else { "php" };
            let unpacked = php_unpacked(php, cache, platform)?;
            let php_dir = dir.join("php").join(php_version);
            copy_file(&unpacked.join(program), &php_dir.join(program))?;
            for file in data_files(php)? {
                copy_file(&downloaded(file.0, file.1, cache)?, &php_dir.join(file.2))?;
            }
            format!(
                "\n[php]\nversion = \"{php_version}\"\npath = \"php/{php_version}/{program}\"\nlicense = \"{}\"\n",
                text(php, "license").unwrap_or("")
            )
        }
        None => String::new(),
    };
    // The servers: installed once (JavaScript: the same for every platform), copied.
    let mut servers = String::new();
    for server in manifest.get("servers").and_then(toml::Value::as_array).into_iter().flatten() {
        let server = server.as_table().ok_or("packaging/lsp.toml: a bad [[servers]]")?;
        let (name, server_version) = (text(server, "name")?, text(server, "version")?);
        let target = dir.join("servers").join(name).join(server_version);
        let prune: Vec<&str> = server
            .get("prune")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_str)
            .collect();
        if server.contains_key("platforms") {
            // A native server: its release for the platform, from GitHub.
            let unpacked = native_unpacked(server, cache, platform)?;
            copy_tree(&unpacked, &target, "", &prune)?;
            // Debug information some releases keep (clangd's and ZLS's on Linux): left out
            // where a `strip` for the platform's programs is at hand.
            if let Some(files) = server
                .get("strip-debug")
                .and_then(toml::Value::as_table)
                .and_then(|t| t.get(platform))
                .and_then(toml::Value::as_array)
            {
                for file in files.iter().filter_map(toml::Value::as_str) {
                    strip_debug(&target.join(file), platform)?;
                }
            }
            // Universal macOS programs: only the platform's architecture kept.
            if let Some(arch) = macos_arch(platform) {
                for file in server
                    .get("universal")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                {
                    thin_file(&target.join(file), arch)?;
                }
            }
        } else if server.contains_key("packages") {
            let installed = installed(root, cache, node, server)?;
            copy_tree(&installed, &target, "", &prune)?;
        } else {
            // Only its files (a release's PHAR: phpactor's).
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        }
        // Its data files (the JSON server's schemas, a PHAR), each checked.
        for (url, sha256, path) in data_files(server)? {
            copy_file(&downloaded(url, sha256, cache)?, &target.join(path))?;
        }
        // Schemas made self-contained (`[[servers.inline]]`: what they refer to put inside
        // them), for a server that would fetch the others itself (taplo).
        let mut parts = Vec::new();
        for inline in server.get("inline").and_then(toml::Value::as_array).into_iter().flatten() {
            let inline = inline.as_table().ok_or("packaging/lsp.toml: a bad [[servers.inline]]")?;
            let (schema, out) = (text(inline, "schema")?, text(inline, "out")?);
            let urls: Vec<(String, String)> = table(inline, "urls")?
                .iter()
                .filter_map(|(prefix, dir)| Some((prefix.clone(), dir.as_str()?.to_owned())))
                .collect();
            if ![schema, out]
                .into_iter()
                .chain(urls.iter().map(|(_, d)| d.as_str()))
                .all(safe_relative)
            {
                return Err(format!("{name}: an inlined schema's paths leave its folder"));
            }
            let read = |path: &Path| -> Option<serde_json::Value> {
                serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
            };
            let root_schema =
                read(&target.join(schema)).ok_or_else(|| format!("{schema}: not JSON"))?;
            let load = |url: &str| {
                let (dir, rest) =
                    urls.iter().find_map(|(p, d)| Some((d, url.strip_prefix(p.as_str())?)))?;
                safe_relative(rest).then(|| read(&target.join(dir).join(rest)))?
            };
            let made = inline_schema(root_schema, &load).map_err(|e| format!("{schema}: {e}"))?;
            let bytes = serde_json::to_vec(&made).map_err(|e| e.to_string())?;
            std::fs::write(target.join(out), bytes).map_err(|e| e.to_string())?;
            parts.extend(urls.into_iter().map(|(_, dir)| dir));
        }
        // The parts are inside the schemas now.
        for dir in parts {
            let dir = target.join(dir);
            if dir.is_dir() {
                std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            }
        }
        servers.push_str(&format!(
            "\n[servers.{name}]\nversion = \"{server_version}\"\npath = \"servers/{name}/{server_version}\"\nlicense = \"{}\"\n",
            text(server, "license").unwrap_or("")
        ));
    }
    let text = format!(
        "# Made by `cargo xtask lsp-bundle` from packaging/lsp.toml.\nplatform = \"{platform}\"\n\n[node]\nversion = \"{version}\"\npath = \"node/{version}/{program}\"\nlicense = \"{}\"\n{php}{servers}",
        text(node, "license").unwrap_or("")
    );
    std::fs::write(dir.join("manifest.toml"), text).map_err(|e| e.to_string())?;
    // The bundled software's licences, said once for the folder.
    copy_file(&root.join("packaging/lsp/NOTICE"), &dir.join("NOTICE"))
}

/// Node for `platform`, downloaded (once), checked and unpacked in the cache: its folder.
fn node_unpacked(node: &toml::Table, cache: &Path, platform: &str) -> Result<PathBuf, String> {
    let platforms = table(node, "platforms")?;
    let entry = platforms
        .get(platform)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("no Node for {platform}"))?;
    let (url, sha256) = (text(entry, "url")?, text(entry, "sha256")?);
    if !url.starts_with("https://nodejs.org/dist/") {
        return Err(format!("{url}: Node comes only from nodejs.org"));
    }
    let file = url.rsplit('/').next().unwrap_or("node");
    let archive = cache.join(file);
    if !archive.is_file() || sha256_of(&archive)? != sha256 {
        println!("lsp-bundle: downloading {url}");
        let status = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&archive)
            .arg(url)
            .status()
            .map_err(|e| format!("curl: {e}"))?;
        if !status.success() {
            return Err(format!("{url}: the download failed"));
        }
    }
    let found = sha256_of(&archive)?;
    if found != sha256 {
        let _ = std::fs::remove_file(&archive);
        return Err(format!("{file}: SHA-256 {found}, expected {sha256}"));
    }
    let stem = file.trim_end_matches(".tar.xz").trim_end_matches(".zip");
    let unpacked = cache.join(stem);
    if !unpacked.is_dir() {
        let status = if file.ends_with(".zip") && which("unzip") {
            Command::new("unzip").args(["-q", "-o"]).arg(&archive).arg("-d").arg(cache).status()
        } else {
            Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(cache).status()
        };
        if !status.map_err(|e| e.to_string())?.success() {
            return Err(format!("{file}: it couldn't be unpacked"));
        }
    }
    Ok(unpacked)
}

/// A table's `[[files]]` (url, SHA-256, path inside its folder), the paths checked.
fn data_files(table: &toml::Table) -> Result<Vec<(&str, &str, &str)>, String> {
    let mut out = Vec::new();
    for file in table.get("files").and_then(toml::Value::as_array).into_iter().flatten() {
        let file = file.as_table().ok_or("packaging/lsp.toml: a bad [[files]]")?;
        let (url, sha256, path) = (text(file, "url")?, text(file, "sha256")?, text(file, "path")?);
        if !safe_relative(path) {
            return Err(format!("{path}: not a path inside its folder"));
        }
        out.push((url, sha256, path));
    }
    Ok(out)
}

/// Where the bundled PHP comes from: static-php-cli's builds (its project's distribution
/// server; its GitHub releases have only its `spc` tool), a version's file.
pub fn static_php(url: &str) -> bool {
    url.strip_prefix("https://dl.static-php.dev/static-php-cli/").is_some_and(|rest| {
        !rest.is_empty()
            && !rest.contains("..")
            && rest.rsplit('/').next().is_some_and(|f| f.starts_with("php-"))
    })
}

/// PHP for `platform` (`[php.platforms.<platform>]`: static-php-cli's build — a static PHP
/// CLI, `php` in a `.tar.gz`, `php.exe` in a `.zip` — and its SHA-256), downloaded once,
/// checked and unpacked in the cache: its folder.
fn php_unpacked(php: &toml::Table, cache: &Path, platform: &str) -> Result<PathBuf, String> {
    let entry = table(php, "platforms")?
        .get(platform)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("no PHP for {platform}"))?;
    let (url, sha256) = (text(entry, "url")?, text(entry, "sha256")?);
    if !static_php(url) {
        return Err(format!("{url}: PHP comes only from static-php-cli's builds"));
    }
    let file = url.rsplit('/').next().unwrap_or("php");
    let folder = cache.join("php").join(sha256);
    let archive = folder.join(file);
    if !archive.is_file() || sha256_of(&archive)? != sha256 {
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        println!("lsp-bundle: downloading {url}");
        let status = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&archive)
            .arg(url)
            .status()
            .map_err(|e| format!("curl: {e}"))?;
        if !status.success() {
            return Err(format!("{url}: the download failed"));
        }
    }
    let found = sha256_of(&archive)?;
    if found != sha256 {
        let _ = std::fs::remove_file(&archive);
        return Err(format!("{file}: SHA-256 {found}, expected {sha256}"));
    }
    let unpacked = folder.join("unpacked");
    if !unpacked.join(".installed").is_file() {
        if unpacked.exists() {
            std::fs::remove_dir_all(&unpacked).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
        let status = if file.ends_with(".zip") && which("unzip") {
            Command::new("unzip").args(["-q", "-o"]).arg(&archive).arg("-d").arg(&unpacked).status()
        } else {
            Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&unpacked).status()
        };
        if !status.map_err(|e| e.to_string())?.success() {
            return Err(format!("{file}: it couldn't be unpacked"));
        }
        std::fs::write(unpacked.join(".installed"), sha256).map_err(|e| e.to_string())?;
    }
    Ok(unpacked)
}

/// A native server's release for `platform` (`[servers.platforms.<platform>]`: its URL on
/// GitHub's releases and its SHA-256), downloaded once, checked and unpacked in the cache:
/// its folder (the archive's top folder `strip`, if it has one).
fn native_unpacked(server: &toml::Table, cache: &Path, platform: &str) -> Result<PathBuf, String> {
    let (name, version) = (text(server, "name")?, text(server, "version")?);
    let entry = table(server, "platforms")?
        .get(platform)
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("no {name} for {platform}"))?;
    let (url, sha256) = (text(entry, "url")?, text(entry, "sha256")?);
    if !github_release(url) {
        return Err(format!("{url}: native servers come only from GitHub's releases"));
    }
    let file = url.rsplit('/').next().unwrap_or("archive");
    let archive = cache.join("native").join(sha256).join(file);
    if !archive.is_file() || sha256_of(&archive)? != sha256 {
        std::fs::create_dir_all(archive.parent().unwrap_or(cache)).map_err(|e| e.to_string())?;
        println!("lsp-bundle: downloading {url}");
        let status = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&archive)
            .arg(url)
            .status()
            .map_err(|e| format!("curl: {e}"))?;
        if !status.success() {
            return Err(format!("{url}: the download failed"));
        }
    }
    let found = sha256_of(&archive)?;
    if found != sha256 {
        let _ = std::fs::remove_file(&archive);
        return Err(format!("{file}: SHA-256 {found}, expected {sha256}"));
    }
    let unpacked = cache.join("native").join(sha256).join("unpacked");
    let done = unpacked.join(".installed");
    if !done.is_file() {
        if unpacked.exists() {
            std::fs::remove_dir_all(&unpacked).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
        if single_gzip(file) {
            // One gzipped program (taplo's releases): named as the entry says.
            let program = text(entry, "program")?;
            if !safe_relative(program) {
                return Err(format!("{name}: `program = {program}` isn't a name"));
            }
            gunzip(&archive, &unpacked.join(program))?;
        } else {
            let status = if file.ends_with(".zip") && which("unzip") {
                Command::new("unzip")
                    .args(["-q", "-o"])
                    .arg(&archive)
                    .arg("-d")
                    .arg(&unpacked)
                    .status()
            } else {
                Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&unpacked).status()
            };
            if !status.map_err(|e| e.to_string())?.success() {
                return Err(format!("{file}: it couldn't be unpacked"));
            }
        }
        std::fs::write(&done, version).map_err(|e| e.to_string())?;
    }
    match server.get("strip").and_then(toml::Value::as_str) {
        Some(top) if safe_relative(top) => Ok(unpacked.join(top)),
        Some(top) => Err(format!("{name}: `strip = {top}` isn't a folder inside the archive")),
        None => Ok(unpacked),
    }
}

/// Whether a release's file is one gzipped program (`taplo-darwin-aarch64.gz`), not a
/// gzipped archive (`.tar.gz`, `.tgz`).
pub fn single_gzip(file: &str) -> bool {
    file.ends_with(".gz") && !file.ends_with(".tar.gz")
}

/// `archive` (gzip) decompressed into `to`, executable.
pub fn gunzip(archive: &Path, to: &Path) -> Result<(), String> {
    let input = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut output = std::fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    std::io::copy(&mut flate2::read::GzDecoder::new(input), &mut output)
        .map_err(|e| format!("{}: {e}", archive.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Whether `url` is a release's file on GitHub (`https://github.com/<owner>/<repo>/releases/download/…`).
pub fn github_release(url: &str) -> bool {
    url.strip_prefix("https://github.com/").is_some_and(|rest| {
        let parts: Vec<&str> = rest.splitn(5, '/').collect();
        parts.len() == 5
            && parts[2] == "releases"
            && parts[3] == "download"
            && !parts[0].is_empty()
            && !parts[1].is_empty()
            && !rest.contains("..")
    })
}

/// The Mach-O architecture of a macOS platform (`None`: not macOS).
pub fn macos_arch(platform: &str) -> Option<MachArch> {
    match platform {
        "macos-arm64" => Some(MachArch::Arm64),
        "macos-x64" => Some(MachArch::X86_64),
        _ => None,
    }
}

/// The universal program at `path` thinned to `arch`, in place (a thin one is left as it is).
fn thin_file(path: &Path, arch: MachArch) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(slice) = thin(&bytes, arch).map_err(|e| format!("{}: {e}", path.display()))? else {
        return Ok(());
    };
    let permissions = std::fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    std::fs::write(path, slice).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, permissions).map_err(|e| e.to_string())
}

/// The `strip` programs that read `platform`'s programs, in the order tried: the host's own
/// where it builds for itself, else a cross one (binutils' MinGW `strip` reads ELF too).
pub fn strip_tools(platform: &str) -> &'static [&'static str] {
    match (platform, host()) {
        ("linux-x64", "linux-x64") => &["strip", "llvm-strip"],
        ("linux-x64", _) => &["x86_64-linux-gnu-strip", "llvm-strip", "x86_64-w64-mingw32-strip"],
        ("windows-x64", "windows-x64") => &["strip", "llvm-strip"],
        ("windows-x64", _) => &["x86_64-w64-mingw32-strip", "llvm-strip"],
        _ => &[],
    }
}

/// `path`'s debug information left out (`strip --strip-debug`), with the first tool of
/// [`strip_tools`] found; none: kept as it is (said).
fn strip_debug(path: &Path, platform: &str) -> Result<(), String> {
    let Some(tool) = strip_tools(platform).iter().find(|t| which_version(t)) else {
        println!(
            "lsp-bundle: no strip for {platform}: {} keeps its debug information",
            path.display()
        );
        return Ok(());
    };
    let stripped = path.with_extension("stripped");
    let status = Command::new(tool)
        .arg("--strip-debug")
        .arg(path)
        .arg("-o")
        .arg(&stripped)
        .status()
        .map_err(|e| format!("{tool}: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&stripped);
        return Err(format!("{}: {tool} failed", path.display()));
    }
    let permissions = std::fs::metadata(path).map_err(|e| e.to_string())?.permissions();
    std::fs::rename(&stripped, path).map_err(|e| e.to_string())?;
    std::fs::set_permissions(path, permissions).map_err(|e| e.to_string())
}

/// Whether `program --version` runs.
fn which_version(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// A server installed with `npm ci` from its lockfile (once per version, in the cache), with
/// the host's Node and the npm it comes with.
fn installed(
    root: &Path,
    cache: &Path,
    node: &toml::Table,
    server: &toml::Table,
) -> Result<PathBuf, String> {
    let (name, version) = (text(server, "name")?, text(server, "version")?);
    let packages = root.join(text(server, "packages")?);
    let dir = cache.join("servers").join(name).join(version);
    let lock = std::fs::read(packages.join("package-lock.json"))
        .map_err(|e| format!("{name}'s lockfile: {e}"))?;
    let done = dir.join(".installed");
    // Installed from this very lockfile already.
    if std::fs::read(&done).is_ok_and(|seen| seen == lock) {
        return Ok(dir);
    }
    let lock_text = String::from_utf8_lossy(&lock);
    if lock_text
        .lines()
        .filter(|l| l.contains("\"resolved\""))
        .any(|l| !l.contains("https://registry.npmjs.org/"))
    {
        return Err(format!("{name}: a package from somewhere other than registry.npmjs.org"));
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for file in ["package.json", "package-lock.json"] {
        copy_file(&packages.join(file), &dir.join(file))?;
    }
    let host_node = node_unpacked(node, cache, host())?;
    let windows = cfg!(windows);
    let node_program = host_node.join(if windows { "node.exe" } else { "bin/node" });
    let npm = host_node.join(if windows {
        "node_modules/npm/bin/npm-cli.js"
    } else {
        "lib/node_modules/npm/bin/npm-cli.js"
    });
    println!("lsp-bundle: installing {name} {version} (npm ci)");
    let status = Command::new(&node_program)
        .arg(&npm)
        .args([
            "ci",
            "--ignore-scripts",
            "--omit=dev",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
        ])
        .current_dir(&dir)
        .env("npm_config_cache", cache.join("npm"))
        .env("npm_config_update_notifier", "false")
        .status()
        .map_err(|e| format!("npm: {e}"))?;
    if !status.success() {
        return Err(format!("{name}: npm ci failed"));
    }
    std::fs::write(&done, &lock).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn which(program: &str) -> bool {
    Command::new(program).arg("-v").output().is_ok()
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(pinned_downloads::digest::sha256_hex(&bytes))
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::copy(from, to).map_err(|e| format!("{}: {e}", from.display()))?;
    Ok(())
}

/// `from`'s files under `to` (the install's marker left out, and what `prune` names:
/// paths relative to the top, `/`-separated, `*` any one name); `relative` is `from`'s path
/// from the top. A folder left empty isn't made.
fn copy_tree(from: &Path, to: &Path, relative: &str, prune: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == ".installed" || name == ".package-lock.json" {
            continue;
        }
        let path = match relative {
            "" => name.to_string_lossy().into_owned(),
            dir => format!("{dir}/{}", name.to_string_lossy()),
        };
        if prune.iter().any(|p| pruned(p, &path)) {
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = to.join(&name);
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, &path, prune)?;
        } else if kind.is_file() {
            copy_file(&entry.path(), &target)?;
        }
        // Links (npm's `.bin`) are left out: the app runs the servers' entry files.
    }
    if std::fs::read_dir(to).map_err(|e| e.to_string())?.next().is_none() && !relative.is_empty() {
        std::fs::remove_dir(to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A data file downloaded once (into the cache, by its checksum) from a pinned commit or
/// tag on GitHub (or a release's file there: a PHAR), checked against its SHA-256: its path.
fn downloaded(url: &str, sha256: &str, cache: &Path) -> Result<PathBuf, String> {
    if !url.starts_with("https://raw.githubusercontent.com/") && !github_release(url) {
        return Err(format!(
            "{url}: data files come only from raw.githubusercontent.com or GitHub's releases"
        ));
    }
    let file = cache.join("files").join(sha256);
    if file.is_file() && sha256_of(&file)? == sha256 {
        return Ok(file);
    }
    std::fs::create_dir_all(cache.join("files")).map_err(|e| e.to_string())?;
    println!("lsp-bundle: downloading {url}");
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&file)
        .arg(url)
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    if !status.success() {
        return Err(format!("{url}: the download failed"));
    }
    let found = sha256_of(&file)?;
    if found != sha256 {
        let _ = std::fs::remove_file(&file);
        return Err(format!("{url}: SHA-256 {found}, expected {sha256}"));
    }
    Ok(file)
}

/// A folder's size (bytes).
pub fn size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

#[cfg(test)]
#[path = "../tests/unit/lsp_bundle.rs"]
mod tests;
