use super::*;

/// The sign as the platform gets it: its icon drawn at the platform's size (its alpha the
/// icon's), its tooltip, the mark on the app's icon (the same icon), its menu — the module's action, then bringing the app to
/// the front; an icon the app doesn't have: nothing shown (rather than a blank).
#[test]
fn the_sign_goes_to_the_platform_with_its_icon_and_menu() {
    let mut icons = Icons::builtin();
    icons.register(
        "cup",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" fill="#000"/></svg>"##,
    );
    let sign = workbench_model::SystemSign {
        icon: "cup",
        tooltip: "kept awake".into(),
        action: Some(("Turn off".into(), command_model::CommandId("keep_awake.toggle"))),
    };
    let shown = platform_sign(&icons, kernel::Rgba::new(1.0, 0.0, 0.0, 1.0), &sign).unwrap();
    assert_eq!((shown.icon.width, shown.icon.height), (u32::from(SIDE), u32::from(SIDE)));
    assert_eq!(shown.icon.rgba.len(), usize::from(SIDE) * usize::from(SIDE) * 4);
    let middle = (usize::from(SIDE) / 2 * usize::from(SIDE) + usize::from(SIDE) / 2) * 4;
    assert_eq!(shown.icon.rgba[middle + 3], 255, "drawn where the icon is");
    assert_eq!(shown.icon.rgba[3], 0, "clear around it");
    assert_eq!(shown.tooltip, "kept awake");
    // The mark on the app's icon: a disc in the accent, the icon in white at its middle,
    // clear at its corners.
    let side = usize::from(BADGE);
    assert_eq!((shown.badge.width, shown.badge.height), (u32::from(BADGE), u32::from(BADGE)));
    let at = |x: usize, y: usize| &shown.badge.rgba[(y * side + x) * 4..(y * side + x) * 4 + 4];
    assert_eq!(at(side / 2, side / 2), [255, 255, 255, 255], "the icon, white");
    assert_eq!(at(side / 2, 1)[..], [255, 0, 0, 255], "the disc, in the accent");
    assert_eq!(at(0, 0)[3], 0, "round: its corners clear");
    let ids: Vec<&str> = shown
        .menu
        .iter()
        .filter_map(|i| match i {
            platform_api::MenuItem::Command { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["keep_awake.toggle", platform_api::SHOW_APP]);
    let missing = workbench_model::SystemSign { icon: "nothing-like-it", ..sign };
    assert!(platform_sign(&icons, kernel::Rgba::BLACK, &missing).is_none());
}
