//! Reads the product's name from `brand.toml` (the repository root) into compile-time
//! environment variables, for `kernel::brand`.

fn main() {
    // Read when the script runs, not when it was built: the repository may have moved since.
    let dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default();
    let path = std::path::Path::new(&dir).join("../../brand.toml");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("brand.toml at the repository root: {e}"));
    for key in ["name", "id", "project_dir", "bundle_id", "repository", "publisher"] {
        let value = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| panic!("brand.toml: missing `{key}`"));
        println!("cargo:rustc-env=BRAND_{}={value}", key.to_ascii_uppercase());
    }
}
