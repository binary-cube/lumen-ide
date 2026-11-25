//! Settings → Appearance → Graphics card: which GPU draws where the computer has several —
//! the system's choice (the default: the user's setting for the app in the system, its
//! power policy on the battery), the integrated one (saves the battery) or the dedicated one
//! (stronger). The GPUs found are listed, the one drawing marked; a change applies at the
//! next start (the GPU is chosen once, before the first window). The web engine follows it:
//! told the window's GPU, it draws there.

use i18n::{t, tr};
use render_api::GpuPreference;
use workbench_model::{SettingsBlock, SettingsContribution, SettingsRow, SettingsScope};

use crate::settings::GPU;

/// The row's key (and the block's id).
const KEY: &str = "gpu";

/// The choices: each one's value in the row, what it is, its label's key — in the row's
/// order, the system's first (the default).
const CHOICES: [(i64, GpuPreference, &str); 3] = [
    (0, GpuPreference::System, "settings.gpu_system"),
    (1, GpuPreference::Integrated, "settings.gpu_integrated"),
    (2, GpuPreference::Dedicated, "settings.gpu_dedicated"),
];

/// The row's value of `preference`.
fn value_of(preference: GpuPreference) -> i64 {
    CHOICES.iter().find(|(_, p, _)| *p == preference).map_or(CHOICES[0].0, |(v, _, _)| *v)
}

/// The preference the row's `value` is (anything else: the system's).
fn preference_of(value: i64) -> GpuPreference {
    CHOICES.iter().find(|(v, _, _)| *v == value).map_or(GpuPreference::System, |(_, p, _)| *p)
}

/// "Graphics card".
#[derive(Debug)]
pub struct GpuSettings;

impl SettingsContribution for GpuSettings {
    fn id(&self) -> &'static str {
        KEY
    }

    fn page(&self) -> &'static str {
        "appearance"
    }

    fn order(&self) -> u16 {
        170
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let current = value_of(GpuPreference::parse(&scope.settings.get(&GPU)).unwrap_or_default());
        let gpus = render_wgpu::gpus();
        let mut notes: Vec<String> = gpus
            .iter()
            .map(|gpu| {
                let kind = t(if gpu.dedicated {
                    "settings.gpu_dedicated"
                } else {
                    "settings.gpu_integrated"
                });
                let line =
                    tr!("settings.gpu_found", name = gpu.name.as_str(), kind = kind.as_str());
                if gpu.used { tr!("settings.gpu_in_use", gpu = line.as_str()) } else { line }
            })
            .collect();
        notes.push(t(if gpus.len() > 1 { "settings.gpu_restart" } else { "settings.gpu_single" }));
        SettingsBlock {
            title: t("settings.gpu"),
            detail: t("settings.gpu_detail"),
            rows: vec![
                SettingsRow::Segmented {
                    key: KEY,
                    label: t("settings.gpu_choice"),
                    detail: t("settings.gpu_choice_detail"),
                    choices: CHOICES.iter().map(|(value, _, label)| (*value, t(label))).collect(),
                    current,
                    default: Some(value_of(GpuPreference::System)),
                },
                SettingsRow::Notes(notes),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key != KEY {
            return;
        }
        let chosen = preference_of(value);
        if let Err(e) = scope.settings.set(&GPU, chosen.as_str().to_owned()) {
            tracing::warn!(error = %e, "the graphics card's choice wasn't saved");
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/gpu_settings.rs"]
mod tests;
