use super::*;

#[test]
fn styles_rewrite_the_drawing() {
    let svg = r#"<svg stroke-width="2" stroke-linecap="round"><rect x="1" rx="2"/><path d="m9 6 6 6"/><path d="M3 3h4z"/></svg>"#;
    assert!(IconStyle::Light.restyle(svg).contains(r#"stroke-width="1.5""#));
    assert!(IconStyle::Bold.restyle(svg).contains(r#"stroke-width="2.5""#));
    let duo = IconStyle::Duotone.restyle(svg);
    assert_eq!(duo.matches("fill-opacity").count(), 2, "the rect and the closed path: {duo}");
    let sharp = IconStyle::Sharp.restyle(svg);
    assert!(sharp.contains("square") && sharp.contains(r#"rx="0.6""#), "{sharp}");
    // Every bundled icon renders in every style.
    let icons = Icons::builtin();
    for style in IconStyle::ALL {
        for (name, _) in BUILTIN {
            let id = icons.id(name).unwrap();
            let key = icons.key_styled(id, 24, style);
            assert!(icons.rasterize_key(key).is_some(), "{name} in {style:?}");
        }
    }
    // Switching styles drops the others' shapes.
    icons.set_style(IconStyle::Duotone);
    assert_eq!(icons.style(), IconStyle::Duotone);
    assert!(icons.icons.iter().all(|i| i.trees.lock().unwrap().len() <= 1));
}

#[test]
fn builtin_icons_render_and_have_shape() {
    let icons = Icons::builtin();
    for (name, _) in BUILTIN {
        let id = icons.id(name).unwrap();
        let glyph = icons.rasterize_key(icons.key(id, 32)).unwrap_or_else(|| panic!("{name}"));
        assert_eq!((glyph.width, glyph.height), (32, 32));
        let covered = glyph.data.iter().filter(|&&a| a > 128).count();
        assert!(covered > 20, "{name}: an empty icon ({covered} pixels)");
    }
}

#[test]
fn keys_do_not_collide_with_text() {
    let icons = Icons::builtin();
    let key = icons.key(icons.id("x").unwrap(), 18);
    assert!(is_icon(key));
    assert!(!is_icon(GlyphKey(12345)));
    assert!(icons.rasterize_key(GlyphKey(12345)).is_none());
}

#[test]
fn modules_can_add_and_replace_icons() {
    let mut icons = Icons::builtin();
    let n = icons.register("twig", r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="24" height="24"/></svg>"#);
    assert_eq!(icons.id("twig"), Some(n));
    let again = icons.register("twig", r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="6"/></svg>"#);
    assert_eq!(n, again);
}

/// A pack drawing only `search` (a filled square), counting its releases.
#[derive(Debug, Default)]
struct Squares(std::sync::atomic::AtomicUsize);

impl IconPack for Squares {
    fn id(&self) -> &'static str {
        "squares"
    }
    fn name(&self) -> &'static str {
        "Squares"
    }
    fn credit(&self) -> &'static str {
        "Tests · MIT"
    }
    fn svg(&self, name: &str) -> Option<std::borrow::Cow<'static, str>> {
        (name == "search").then_some(std::borrow::Cow::Borrowed(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="24" height="24"/></svg>"#,
        ))
    }
    fn release(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A pack draws what it has, ours the rest; its look never shares a glyph with a style;
/// it's saved and found again by its id; not drawn any more, it lets its data go.
#[test]
fn a_pack_draws_what_it_has_and_ours_the_rest() {
    let mut icons = Icons::builtin();
    let pack = Arc::new(Squares::default());
    let look = icons.register_pack(pack.clone());
    assert_eq!(look, IconLook::Pack(0));
    assert_eq!(icons.looks().len(), IconStyle::ALL.len() + 1);
    assert_eq!(icons.look_id(look), "squares");
    assert_eq!(icons.look_from_id("squares"), Some(look));
    assert_eq!(icons.look_from_id("bold"), Some(IconLook::Style(IconStyle::Bold)));
    assert_eq!(icons.look_from_id("nothing"), None);
    let coverage = |icons: &Icons, name: &str, look: IconLook| {
        let key = icons.key_in(icons.id(name).unwrap(), 24, look);
        icons.rasterize_key(key).unwrap().data.iter().filter(|&&a| a > 128).count()
    };
    let ours = IconLook::Style(IconStyle::Regular);
    assert_eq!(coverage(&icons, "search", look), 24 * 24, "the pack's square");
    assert!(coverage(&icons, "search", ours) < 24 * 24 / 2, "ours: a magnifier");
    assert_eq!(
        coverage(&icons, "folder", look),
        coverage(&icons, "folder", ours),
        "what the pack lacks: ours"
    );
    let id = icons.id("search").unwrap();
    assert_ne!(icons.key_in(id, 24, look), icons.key_in(id, 24, ours), "never the same glyph");
    icons.set_look(look);
    assert_eq!(icons.look(), look);
    assert_eq!(icons.style(), IconStyle::Regular, "a pack counts as the regular style");
    let released = pack.0.load(std::sync::atomic::Ordering::Relaxed);
    icons.set_look(ours);
    assert!(pack.0.load(std::sync::atomic::Ordering::Relaxed) > released, "let go");
}

/// The drawings let go (the window idle) are read again when an icon is rasterized next:
/// the same pixels.
#[test]
fn icons_drawn_again_after_their_drawings_go_are_the_same() {
    let icons = Icons::builtin();
    let id = icons.id("folder").unwrap();
    let before = icons.rasterize_key(icons.key(id, 24)).unwrap().data;
    assert!(icons.icons.iter().any(|i| !i.trees.lock().unwrap().is_empty()));
    icons.release_drawings();
    assert!(icons.icons.iter().all(|i| i.trees.lock().unwrap().is_empty()), "all let go");
    assert_eq!(icons.rasterize_key(icons.key(id, 24)).unwrap().data, before);
}
