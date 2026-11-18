//! Settings bounded context: layered configuration (default, user, project).
//!
//! The user's settings (declared once, read and written through [`SettingsService`]) and the
//! formatting settings from `.editorconfig`.

pub mod editorconfig;
pub mod project_frameworks;
pub mod project_levels;
pub mod service;
pub mod store;

pub use editorconfig::{
    Charset, EditorConfig, EditorConfigFile, EditorConfigSource, EndOfLine, IndentStyle, resolve,
};
pub use project_frameworks::ProjectFrameworksStore;
pub use project_levels::{ProjectLevelsStore, SavedLevels};
pub use service::{Setting, SettingChanged, SettingType, SettingsService};
pub use store::{Section, SettingValue, SettingsStore};
