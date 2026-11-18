//! What a language server's files go through on their way in, the same whether `cargo xtask
//! lsp-bundle` makes the app's `lsp/` (the file is that tool's too: only `std` and
//! `serde_json` here) or the app downloads them ([`crate::downloads`]): paths checked to stay
//! inside their folder, a universal macOS program thinned to one architecture, JSON schemas
//! made self-contained. What a server leaves out (`prune`) is `pinned_downloads::paths`'.

/// A JSON schema made self-contained: every `$ref` to another schema (by URL — relative ones
/// resolved against the schema's `$id`) that `load` has is put inside it, under
/// `definitions/x-bundled-<name>`, and pointed to there; the inlined schemas' own `#…`
/// references follow them. A reference `load` doesn't have is an error (the server would
/// fetch it).
pub fn inline_schema(
    mut root: serde_json::Value,
    load: &dyn Fn(&str) -> Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let id = root.get("$id").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    let mut inliner = Inliner {
        load,
        root: id.clone(),
        keys: Default::default(),
        pending: Vec::new(),
        missing: Default::default(),
    };
    inliner.rewrite(&mut root, &id, "");
    let mut done = Vec::new();
    while let Some((url, key, mut schema)) = inliner.pending.pop() {
        let base = schema.get("$id").and_then(serde_json::Value::as_str).unwrap_or(&url).to_owned();
        if let Some(object) = schema.as_object_mut() {
            object.remove("$id");
            object.remove("$schema");
        }
        inliner.rewrite(&mut schema, &base, &format!("/definitions/{key}"));
        done.push((key, schema));
    }
    if !inliner.missing.is_empty() {
        let missing: Vec<String> = inliner.missing.into_iter().collect();
        return Err(format!("not bundled: {}", missing.join(", ")));
    }
    let object = root.as_object_mut().ok_or("not an object")?;
    let definitions = object
        .entry("definitions")
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or("`definitions` isn't an object")?;
    for (key, schema) in done {
        definitions.insert(key, schema);
    }
    Ok(root)
}

struct Inliner<'a> {
    load: &'a dyn Fn(&str) -> Option<serde_json::Value>,
    /// The root schema's own URL (its `$id`).
    root: String,
    /// The schemas inlined: URL → key under `definitions`.
    keys: std::collections::BTreeMap<String, String>,
    /// Inlined, not rewritten yet: (URL, key, schema).
    pending: Vec<(String, String, serde_json::Value)>,
    missing: std::collections::BTreeSet<String>,
}

impl Inliner<'_> {
    /// `value`'s references rewritten: `base` is the URL its relative ones resolve against,
    /// `at` where its schema is in the made one (`""`: the root).
    fn rewrite(&mut self, value: &mut serde_json::Value, base: &str, at: &str) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(serde_json::Value::as_str) {
                    let reference = self.reference(reference, base, at);
                    map.insert("$ref".into(), serde_json::Value::String(reference));
                }
                for (key, child) in map.iter_mut() {
                    // Data, not schemas.
                    if !matches!(key.as_str(), "$ref" | "enum" | "const" | "default" | "examples") {
                        self.rewrite(child, base, at);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.rewrite(item, base, at);
                }
            }
            _ => {}
        }
    }

    fn reference(&mut self, reference: &str, base: &str, at: &str) -> String {
        let (url, fragment) = reference.split_once('#').unwrap_or((reference, ""));
        let fragment = fragment.to_owned();
        if url.is_empty() {
            return format!("#{at}{fragment}");
        }
        let absolute = resolve_url(base, url);
        if absolute == self.root {
            return format!("#{fragment}");
        }
        let key = match self.keys.get(&absolute) {
            Some(key) => key.clone(),
            None => {
                let Some(schema) = (self.load)(&absolute) else {
                    self.missing.insert(absolute);
                    return reference.to_owned();
                };
                let stem =
                    absolute.rsplit('/').next().unwrap_or("schema").trim_end_matches(".json");
                let stem: String = stem
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
                    .collect();
                let mut key = format!("x-bundled-{stem}");
                while self.keys.values().any(|k| *k == key) {
                    key.push('-');
                }
                self.keys.insert(absolute.clone(), key.clone());
                self.pending.push((absolute, key.clone(), schema));
                key
            }
        };
        format!("#/definitions/{key}{fragment}")
    }
}

/// `reference` resolved against `base` (both URLs; `reference` relative or absolute).
pub fn resolve_url(base: &str, reference: &str) -> String {
    if reference.contains("://") || base.is_empty() {
        return reference.to_owned();
    }
    let dir = base.rfind('/').map_or(base, |i| &base[..=i]);
    format!("{dir}{}", reference.trim_start_matches("./"))
}

/// An architecture in a universal (fat) Mach-O program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachArch {
    Arm64,
    X86_64,
}

impl MachArch {
    /// Its `cputype` in a Mach-O header.
    fn cpu_type(self) -> u32 {
        const ABI64: u32 = 0x0100_0000;
        match self {
            Self::Arm64 => ABI64 | 12,
            Self::X86_64 => ABI64 | 7,
        }
    }
}

/// `bytes`' slice for `arch` if they're a universal Mach-O program (`None`: they aren't — a
/// thin one is left as it is); an error if it hasn't that architecture.
pub fn thin(bytes: &[u8], arch: MachArch) -> Result<Option<&[u8]>, String> {
    let word = |at: usize| -> Option<u32> {
        bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let wide = |at: usize| -> Option<u64> {
        bytes.get(at..at + 8).map(|b| u64::from_be_bytes(b.try_into().unwrap_or([0; 8])))
    };
    // FAT_MAGIC (32-bit offsets) or FAT_MAGIC_64.
    let entry_size = match word(0) {
        Some(0xcafe_babe) => 20,
        Some(0xcafe_babf) => 32,
        _ => return Ok(None),
    };
    let count = word(4).ok_or("a truncated universal header")? as usize;
    // Java's class files share the magic: a universal program has few architectures.
    if count == 0 || count > 16 {
        return Ok(None);
    }
    for i in 0..count {
        let at = 8 + i * entry_size;
        let cpu = word(at).ok_or("a truncated universal header")?;
        if cpu != arch.cpu_type() {
            continue;
        }
        let (offset, size) = if entry_size == 20 {
            (word(at + 8).map(u64::from), word(at + 12).map(u64::from))
        } else {
            (wide(at + 8), wide(at + 16))
        };
        let (offset, size) = (
            offset.ok_or("a truncated universal header")? as usize,
            size.ok_or("a truncated universal header")? as usize,
        );
        return bytes
            .get(offset..offset.saturating_add(size))
            .map(Some)
            .ok_or_else(|| "a universal program shorter than its header says".to_owned());
    }
    Err(format!("no {arch:?} in the universal program"))
}

/// Whether `path` stays inside the folder it's relative to.
pub fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// The architecture this program runs on, as a universal macOS program names it (`None`:
/// neither of the two).
pub fn host_arch() -> Option<MachArch> {
    match std::env::consts::ARCH {
        "aarch64" => Some(MachArch::Arm64),
        "x86_64" => Some(MachArch::X86_64),
        _ => None,
    }
}
