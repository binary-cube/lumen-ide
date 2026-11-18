//! The files' icons: the modules' rules and the chosen file icon set.

use super::*;

/// A file named after its language with no extension to go by (a `Dockerfile`): a file
/// icon set finds its icon through its language, ours through the modules' rule.
#[test]
fn a_dockerfile_has_the_docker_icon_in_every_set() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    assert_eq!(support.languages.icon_for("Dockerfile"), ("docker", "dockerfile".to_owned()));
    assert_eq!(support.languages.icon_for("Dockerfile.dev").0, "docker");
    let mut icons = Icons::builtin();
    for icon in &support.icons {
        icons.register(icon.name, icon.svg);
    }
    for set in &support.file_icon_sets {
        icons.register_file_set(Arc::clone(set));
    }
    let palette = Palette::default();
    // Ours: the docker drawing.
    icons.set_file_set(None);
    let (id, _) = file_icon(&support, &icons, &palette, "Dockerfile");
    assert_eq!(icons.name(id), Some("docker"));
    // Every set that draws a docker icon: its own, not its plain file.
    for (i, set) in support.file_icon_sets.iter().enumerate() {
        let Some(key) = set.file("Dockerfile", "dockerfile") else { continue };
        let plain = set.file("plain", "");
        if Some(key) == plain {
            continue;
        }
        icons.set_file_set(Some(i as u8));
        let (id, _) = file_icon(&support, &icons, &palette, "Dockerfile");
        assert_eq!(
            Some(id),
            icons.file_icon_in(i as u8, "Dockerfile", "dockerfile"),
            "{}",
            set.id()
        );
    }
}
