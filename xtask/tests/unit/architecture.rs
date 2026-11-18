use super::*;

fn real() -> Architecture {
    Architecture::read(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap()
}

#[test]
fn every_crate_folder_finds_its_layer_the_first_match_winning() {
    let a = real();
    let layer = |dir: &str| a.layer_of(dir).map(|l| l.name.as_str());
    assert_eq!(layer("crates/domain/code/document"), Some("domain"));
    assert_eq!(layer("crates/render/render-api"), Some("render-api"));
    assert_eq!(layer("crates/render/render-wgpu"), Some("render-driver"));
    assert_eq!(layer("crates/render/icons"), Some("text"));
    assert_eq!(layer("crates/modules/editing/emmet"), Some("module"));
    assert_eq!(layer("crates/infra/system/process"), Some("infra-shared"));
    assert_eq!(layer("crates/infra/database/sqlite"), Some("infra"));
    assert_eq!(layer("crates/elsewhere/x"), None);
}

#[test]
fn modules_may_never_use_modules() {
    let a = real();
    assert!(!a.layer("module").unwrap().may_use.as_ref().unwrap().contains(&"module".to_owned()));
    assert!(a.module_may_use("anything", "fs-local"));
    assert!(a.module_may_use("vcs-git", "git") && !a.module_may_use("relax", "git"));
}

#[test]
fn a_rule_naming_a_layer_that_doesnt_exist_is_refused() {
    let text = "[[layer]]\nname = \"a\"\npaths = [\"a\"]\nmay_use = [\"b\"]\n";
    assert!(Architecture::parse(text).unwrap_err().contains("`b`"));
}
