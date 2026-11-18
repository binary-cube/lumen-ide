use std::sync::Arc;

use lsp_client::fake::Behaviour;
use settings_model::SettingsService;
use workbench_model::SettingsScope;

use super::*;
use crate::harness::{Harness, MemorySettings, fresh_downloads};

fn rows(settings: &SettingsService) -> Vec<SettingsRow> {
    DownloadedParts::default()
        .block(SettingsScope {
            settings,
            project: None,
            project_data: None,
            wake: None,
            language: None,
        })
        .rows
}

#[test]
fn each_component_is_listed_with_what_it_takes_and_its_buttons() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("parts");
    let settings = SettingsService::new(Arc::new(MemorySettings::default()));
    let before = rows(&settings);
    let buttons: Vec<&str> = before
        .iter()
        .flat_map(|r| match r {
            SettingsRow::Actions { buttons, .. } => buttons.iter().map(|(id, _)| *id).collect(),
            SettingsRow::Status { button: Some((id, _)), .. } => vec![*id],
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(buttons, ["download:node", "download:yaml-srv", "download-all"]);
    downloads::install("node", None).unwrap();
    let after = rows(&settings);
    let node = after.iter().find_map(|r| match r {
        SettingsRow::Status { text, button: Some((id, label)) } if *id == "remove:node" => {
            Some((text.clone(), label.clone()))
        }
        _ => None,
    });
    let (text, label) = node.expect("node downloaded, with Remove");
    assert!(text.contains("Node.js 1.0.0"), "{text}");
    assert_eq!(label, i18n::t("settings.lsp_part_remove"));
    // Removing asks first.
    let block = DownloadedParts::default();
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let (question, _) = block.press(scope, "remove:node").expect("asked");
    assert!(question.danger);
    // Download all: what's missing.
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    assert!(block.press(scope, "download-all").is_none());
    crate::harness::wait("all downloaded", || downloads::found("yaml-srv").is_some());
}

#[test]
fn without_downloads_the_block_says_so() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("parts-off");
    downloads::use_downloads(None);
    let settings = SettingsService::new(Arc::new(MemorySettings::default()));
    assert_eq!(rows(&settings), [SettingsRow::Notes(vec![i18n::t("settings.lsp_parts_off")])]);
}
