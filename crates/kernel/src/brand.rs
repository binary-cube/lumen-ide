//! The product's name (from `brand.toml` at the repository root, read at build time):
//! everything that shows or stores the app's name goes through here, so renaming the app
//! is a one-line change.

/// Shown to people: windows, menus, About.
pub const NAME: &str = env!("BRAND_NAME");
/// Lowercase: the binary, the config directory, file prefixes.
pub const ID: &str = env!("BRAND_ID");
/// The directory a project's data is kept in (caches, index, session, settings).
pub const PROJECT_DIR: &str = env!("BRAND_PROJECT_DIR");
/// The macOS bundle identifier.
pub const BUNDLE_ID: &str = env!("BRAND_BUNDLE_ID");

/// Where the app's code and releases are (`owner/name` on GitHub): its updates come from
/// there.
pub const REPOSITORY: &str = env!("BRAND_REPOSITORY");

/// The app's version (the workspace's: every crate shares it).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The app's icon (256 × 256 PNG, round art on a transparent background, made by `cargo
/// xtask icon`): the windows' and the Dock's (outside the bundle), the calm scene's mark.
pub const ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon-256.png");

/// `text` with `{app}` replaced by the name (e.g. a built-in theme's name).
pub fn named(text: &str) -> String {
    text.replace("{app}", NAME)
}
