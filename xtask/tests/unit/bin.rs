use super::*;

fn rules() -> Architecture {
    Architecture::read(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap()
}

fn pkg(name: &str, layer: &str, deps: &[(&str, bool)]) -> Package {
    Package {
        name: name.into(),
        layer: layer.into(),
        deps: deps.iter().map(|(d, p)| ((*d).into(), *p)).collect(),
    }
}

#[test]
fn domain_cannot_depend_on_infra_or_render() {
    let packages = vec![
        pkg("document", "domain", &[("fs-local", true), ("wgpu", false)]),
        pkg("fs-local", "infra", &[]),
    ];
    assert_eq!(find_violations(&packages, &rules()).len(), 2);
}

#[test]
fn allowed_dependencies_pass() {
    let packages = vec![
        pkg("kernel", "kernel", &[]),
        pkg("document", "domain", &[("kernel", true), ("ropey", false)]),
        pkg("render-wgpu", "render-driver", &[("wgpu", false)]),
    ];
    assert!(find_violations(&packages, &rules()).is_empty());
}

#[test]
fn only_the_app_builds_on_its_screens() {
    let packages = vec![
        pkg("ui-workbench", "ui", &[]),
        pkg("project-tree", "module", &[("ui-workbench", true)]),
        pkg("ui-components", "ui", &[("ui-workbench", true)]),
        pkg("app", "app", &[("ui-workbench", true)]),
    ];
    let found = find_violations(&packages, &rules());
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|v| !v.starts_with("app ")));
}

#[test]
fn a_module_uses_only_the_infrastructure_it_is_allowed() {
    let packages = vec![
        pkg("pty", "infra", &[]),
        pkg("fs-local", "infra", &[]),
        pkg("process", "infra-shared", &[]),
        pkg("db-postgres", "infra", &[]),
        pkg("terminal-panel", "module", &[("pty", true), ("fs-local", true)]),
        pkg("quality", "module", &[("process", true), ("pty", true)]),
        pkg("structure", "module", &[("db-postgres", true)]),
        pkg("app", "app", &[("db-postgres", true), ("pty", true)]),
    ];
    let found = find_violations(&packages, &rules());
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|v| v.starts_with("quality ") && v.contains("pty")));
    assert!(found.iter().any(|v| v.starts_with("structure ") && v.contains("db-postgres")));
}

/// The release's date: days since 1970 as a calendar date, leap years included.
#[test]
fn days_are_calendar_dates() {
    assert_eq!(civil_date(0), "1970-01-01");
    assert_eq!(civil_date(11_016), "2000-02-29");
    assert_eq!(civil_date(20_775), "2026-11-18");
}
