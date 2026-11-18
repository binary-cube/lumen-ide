use super::*;

fn light() -> Palette {
    let mut p = Palette::dark();
    p.editor_background = Rgba::rgb(0xffffff);
    p.panel_background = Rgba::rgb(0xf3f3f3);
    p.window_background = Rgba::rgb(0xeeeeee);
    p.text = Rgba::rgb(0x202020);
    p.accent = Rgba::rgb(0x0066cc);
    p
}

#[test]
fn a_dark_theme_gives_draw_io_its_dark_look_in_our_colors() {
    let p = Palette::dark();
    let theme = Theme::from_palette(&p);
    assert!(theme.dark);
    assert_eq!(theme.look(), ("kennedy", "1"));
    assert_eq!(theme.page, p.editor_background.to_hex());
    let config = config(&theme);
    assert_eq!(config["darkColor"], json!(theme.page));
    assert_eq!(config["lightColor"], json!(theme.text));
    assert_eq!(config["defaultDarkPageBackgroundColor"], json!(theme.page));
    assert!(config.get("defaultPageBackgroundColor").is_none());
    assert_eq!(config["suppressNewWindows"], json!(true));
}

#[test]
fn a_light_theme_gives_its_page_and_grid() {
    let theme = Theme::from_palette(&light());
    assert!(!theme.dark);
    assert_eq!(theme.look(), ("kennedy", "0"));
    let config = config(&theme);
    assert_eq!(config["defaultPageBackgroundColor"], json!("#ffffff"));
    assert_eq!(config["defaultGridColor"], json!(theme.grid));
    assert!(config.get("darkColor").is_none());
}

#[test]
fn new_shapes_follow_the_look_and_the_theme_s_colors_are_offered_first() {
    let theme = Theme::from_palette(&light());
    let config = config(&theme);
    assert_eq!(config["defaultVertexStyle"]["fillColor"], "default");
    assert_eq!(config["defaultVertexStyle"]["strokeColor"], "default");
    assert_eq!(config["defaultEdgeStyle"]["strokeColor"], "default");
    assert_eq!(config["defaultEdgeStyle"]["edgeStyle"], "orthogonalEdgeStyle");
    let presets = config["presetColors"].as_array().unwrap();
    assert_eq!(presets[0], json!("0066CC"), "the accent first, hex without #");
    assert!(
        presets.iter().all(|c| c.as_str().is_some_and(|c| c.len() == 6 && !c.starts_with('#')))
    );
    assert!(presets.contains(&json!("EA6B66")), "draw.io's own after");
    assert!(presets.len() <= 36);
}

#[test]
fn the_css_sets_draw_io_s_interface_colors_from_the_palette() {
    let theme = Theme::from_palette(&light());
    let css = css(&theme);
    assert!(css.starts_with(":root{"));
    assert!(css.contains(&format!("--ge-panel-color:{};", theme.panel)));
    assert!(css.contains(&format!("--ge-dark-panel-color:{};", theme.panel)));
    assert!(css.contains(&format!("--accent-text-color:{};", theme.accent)));
    assert!(css.contains(&format!("--text-color:{};", theme.text)));
    assert!(css.contains(".geMenubarContainer"));
    // Only colors written: no quote or brace from a value.
    assert!(!css.contains('"'));
    assert_eq!(config(&theme)["css"], json!(css));
}

#[test]
fn a_translucent_color_is_laid_on_what_it_covers() {
    let mut p = light();
    p.hover_background = Rgba::rgb(0x000000).with_alpha(0.5);
    p.panel_background = Rgba::rgb(0xffffff);
    let theme = Theme::from_palette(&p);
    assert_eq!(theme.hover, "#808080");
}

#[test]
fn menus_that_cannot_work_here_are_hidden() {
    let config = config(&Theme::from_palette(&light()));
    let hidden = config["hideMenuItems"].as_array().unwrap();
    for name in ["importFrom", "print", "exportPdf", "embed", "exit", "keyboardShortcuts"] {
        assert!(hidden.contains(&json!(name)), "{name}");
    }
    for kept in ["save", "exportAs", "exportPng", "exportSvg", "undo", "editDiagram"] {
        assert!(!hidden.contains(&json!(kept)), "{kept}");
    }
}
