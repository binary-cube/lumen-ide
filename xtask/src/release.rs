//! What a release carries besides its files (plan 84b): `SHA256SUMS`, and `latest.json` — the
//! version and, for each platform, its file's name, size and SHA-256 — signed with the
//! project's Ed25519 key (`latest.json.sig`). The app installs nothing the signature or a sum
//! doesn't vouch for.
//!
//! `cargo xtask release-key` makes the key once: the public half in `packaging/release-key.pub`
//! (committed, built into the app), the secret one in a file of its own, for the repository's
//! secrets and nowhere else. `cargo xtask release-manifest` writes the manifest from `dist/`,
//! signed when the secret is given (`RELEASE_SIGNING_KEY`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Where the public key lives, relative to the workspace.
pub const PUBLIC_KEY: &str = "packaging/release-key.pub";

/// A release's file: its platform (`macos-universal`, `windows-x64`, `linux-x86_64`), name,
/// size and SHA-256 (lowercase hex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub platform: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// A release's installer (`<id>-<version>-<platform>-setup.exe`): listed in `SHA256SUMS`,
/// never in `latest.json` (the app updates itself from the archive).
pub fn is_installer(name: &str, id: &str, version: &str) -> bool {
    name.strip_prefix(&format!("{id}-{version}-")).is_some_and(|rest| rest.ends_with("-setup.exe"))
}

/// The platform in a release file's name `<id>-<version>-<platform>.<zip|tar.gz>`; `None` for
/// any other file.
pub fn platform_of(name: &str, id: &str, version: &str) -> Option<String> {
    let rest = name.strip_prefix(&format!("{id}-{version}-"))?;
    let platform = rest.strip_suffix(".zip").or_else(|| rest.strip_suffix(".tar.gz"))?;
    let valid = !platform.is_empty()
        && platform.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| platform.to_owned())
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Bytes from hex (whitespace around it ignored); `None` if it isn't exactly `N` bytes of hex.
pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The manifest's text (pretty JSON, a newline at the end: signed as written).
pub fn manifest(version: &str, date: &str, assets: &[Asset]) -> String {
    let files: Vec<Value> = assets
        .iter()
        .map(
            |a| json!({"platform": a.platform, "name": a.name, "size": a.size, "sha256": a.sha256}),
        )
        .collect();
    let doc = json!({"version": version, "date": date, "files": files});
    let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
    text.push('\n');
    text
}

/// `SHA256SUMS`, as `sha256sum -c` reads it.
pub fn sums(assets: &[Asset]) -> String {
    assets.iter().map(|a| format!("{}  {}\n", a.sha256, a.name)).collect()
}

/// `text` signed with the secret key `secret` (hex of its 32-byte seed): the signature's hex.
pub fn sign(text: &str, secret: &str) -> Result<String, String> {
    let seed = unhex::<32>(secret).ok_or("the signing key isn't 32 bytes of hex")?;
    let key = SigningKey::from_bytes(&seed);
    Ok(hex(&key.sign(text.as_bytes()).to_bytes()))
}

/// Whether `signature` (hex) is `public`'s (hex) on `text`.
pub fn verify(text: &str, signature: &str, public: &str) -> bool {
    let (Some(public), Some(signature)) = (unhex::<32>(public), unhex::<64>(signature)) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&public) else { return false };
    key.verify(text.as_bytes(), &Signature::from_bytes(&signature)).is_ok()
}

/// A new key: the secret seed's hex and the public key's hex.
pub fn new_key() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("no randomness from the system: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((hex(&seed), hex(key.verifying_key().as_bytes())))
}

/// `release-key`: a key made once, its public half written to [`PUBLIC_KEY`], its secret to
/// `secret_out` (only the owner can read it). Refused when a public key is there already:
/// a new one would make every release signed before it unverifiable for the app.
pub fn make_key(root: &Path, secret_out: &Path) -> Result<String, String> {
    let public_path = root.join(PUBLIC_KEY);
    if public_path.exists() {
        return Err(format!("{PUBLIC_KEY} exists: the project has its key already"));
    }
    if secret_out.exists() {
        return Err(format!("{} exists: not overwritten", secret_out.display()));
    }
    let (secret, public) = new_key()?;
    write_secret(secret_out, &secret).map_err(|e| format!("{}: {e}", secret_out.display()))?;
    std::fs::write(&public_path, format!("{public}\n"))
        .map_err(|e| format!("{}: {e}", public_path.display()))?;
    Ok(format!(
        "public key: {public}\n  written to {PUBLIC_KEY} (commit it)\nsecret key: written to {}\n  \
         put its content in the repository's secret RELEASE_SIGNING_KEY, then delete the file",
        secret_out.display()
    ))
}

#[cfg(unix)]
fn write_secret(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file =
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")
}

#[cfg(not(unix))]
fn write_secret(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, format!("{text}\n"))
}

/// `release-manifest`: `SHA256SUMS` and `latest.json` for the files of `version` in `dist`,
/// and `latest.json.sig` when `secret` is given (checked against [`PUBLIC_KEY`] before it's
/// written). `signed`: a signature is required (the release workflow).
pub fn write_manifest(
    root: &Path,
    dist: &Path,
    id: &str,
    version: &str,
    date: &str,
    secret: Option<&str>,
    signed: bool,
) -> Result<String, String> {
    let mut assets = Vec::new();
    let mut installers = Vec::new();
    let entries = std::fs::read_dir(dist).map_err(|e| format!("{}: {e}", dist.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let platform = platform_of(&name, id, version);
        let installer = is_installer(&name, id, version);
        if platform.is_none() && !installer {
            continue;
        }
        let path: PathBuf = entry.path();
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let asset = Asset {
            platform: platform.unwrap_or_default(),
            name,
            size: bytes.len() as u64,
            sha256: hex(&Sha256::digest(&bytes)),
        };
        if installer { installers.push(asset) } else { assets.push(asset) }
    }
    if assets.is_empty() {
        return Err(format!("no {id}-{version}-*.zip / .tar.gz in {}", dist.display()));
    }
    assets.sort_by(|a, b| a.platform.cmp(&b.platform));
    let text = manifest(version, date, &assets);
    let write = |name: &str, text: &str| {
        std::fs::write(dist.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    installers.sort_by(|a, b| a.name.cmp(&b.name));
    let listed: Vec<Asset> = assets.iter().chain(&installers).cloned().collect();
    write("SHA256SUMS", &sums(&listed))?;
    write("latest.json", &text)?;
    let mut done: Vec<String> =
        assets.iter().map(|a| format!("{}  {} ({} bytes)", a.platform, a.name, a.size)).collect();
    done.extend(installers.iter().map(|a| format!("installer  {} ({} bytes)", a.name, a.size)));
    match secret.map(str::trim).filter(|s| !s.is_empty()) {
        Some(secret) => {
            let signature = sign(&text, secret)?;
            let public = std::fs::read_to_string(root.join(PUBLIC_KEY))
                .map_err(|e| format!("{PUBLIC_KEY}: {e}"))?;
            if !verify(&text, &signature, &public) {
                return Err(format!("the signing key isn't the one of {PUBLIC_KEY}"));
            }
            write("latest.json.sig", &format!("{signature}\n"))?;
            done.push("latest.json signed".into());
        }
        None if signed => return Err("RELEASE_SIGNING_KEY isn't set: nothing signed".into()),
        None => done.push("latest.json NOT signed (no RELEASE_SIGNING_KEY)".into()),
    }
    Ok(done.join("\n"))
}

#[cfg(test)]
#[path = "../tests/unit/release.rs"]
mod tests;
