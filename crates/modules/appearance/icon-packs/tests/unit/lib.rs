use super::*;

/// Every drawing of every pack draws (resvg reads it, it covers something), in place of
/// ours; the pack's index is read only once drawn from, and let go when released.
#[test]
fn every_pack_draws_its_icons() {
    for module in modules() {
        let pack = Arc::new((module.pack)());
        assert!(!pack.is_read(), "{}: nothing read before it's drawn", pack.id());
        let mut icons = icons::Icons::builtin();
        let look = icons.register_pack(pack.clone());
        let names: Vec<&str> = DataPack::read(pack.data).keys().copied().collect();
        assert!(names.len() >= 60, "{}: {} drawings", pack.id(), names.len());
        for name in names {
            // Ours too (the set's or a module's): one registered for the test if not.
            let id = icons.id(name).unwrap_or_else(|| icons.register(name, "<svg/>"));
            let glyph = icons.rasterize_key(icons.key_in(id, 32, look));
            let covered = glyph.map_or(0, |g| g.data.iter().filter(|&&a| a > 100).count());
            assert!(covered > 8, "{}: {name} draws nothing ({covered})", pack.id());
        }
        assert!(pack.is_read());
        pack.release();
        assert!(!pack.is_read(), "{}: released", pack.id());
    }
}

/// Every pack draws the icons seen everywhere (a file, a folder, search, settings, close),
/// so choosing one changes the whole interface, not a few corners of it.
#[test]
fn every_pack_has_the_common_icons() {
    for module in modules() {
        let pack = (module.pack)();
        for name in ["file", "folder", "search", "settings", "x", "plus", "chevron-right"] {
            assert!(pack.svg(name).is_some(), "{}: no {name}", pack.id());
        }
    }
}

/// The file icon set: a file by its exact name, its longest extension, its language; a
/// folder by its name, open or closed; else its defaults. Every drawing draws, in color.
#[test]
fn the_file_icon_set_finds_files_and_folders() {
    use icons::FileIconSet;
    let set = Arc::new(material_files());
    assert!(!set.is_read());
    assert_eq!(set.file("package.json", "json"), Some("nodejs"), "its exact name");
    assert_eq!(
        set.file("composer.json", "json"),
        Some("json"),
        "no rule of its own: its extension"
    );
    assert_eq!(set.file("view.blade.php", "blade"), set.file("x.blade.php", ""));
    assert_ne!(set.file("view.blade.php", ""), set.file("index.php", ""), "the longest extension");
    assert_eq!(set.file("Index.PHP", ""), Some("php"), "any case");
    assert_eq!(set.file("nothing.unknown", ""), Some("file"), "its default");
    assert_eq!(set.folder("src", false), Some("folder-src"));
    assert_eq!(set.folder("src", true), Some("folder-src-open"));
    assert_eq!(set.folder("whatever", false), Some("folder"));
    assert!(set.is_read());
    let mut icons = icons::Icons::builtin();
    let place = icons.register_file_set(set.clone());
    icons.set_file_set(Some(place));
    let (id, tint) = icons.file("index.php", "file-code", "php");
    assert_eq!(tint, icons::Tint::Own);
    let glyph = icons.rasterize_key(icons.key(id, 24)).expect("drawn");
    assert_eq!(glyph.format, render_api::GlyphFormat::Color);
    for key in DataPack::read(set.svgs).keys() {
        assert!(set.svg(key).is_some(), "{key}");
    }
    icons.set_file_set(None);
    assert!(!set.is_read(), "released when another is chosen");
    assert_eq!(icons.file("index.php", "file-code", "php").1, icons::Tint::Language("php".into()));
}

/// Every file icon set: an icon for the common files (by name, extension or language) and
/// folders, open and closed, its defaults; each extension's drawing draws, in color;
/// released when another is chosen.
#[test]
fn every_file_icon_set_covers_the_common_files() {
    use icons::FileIconSet;
    for module in file_set_modules() {
        let set = Arc::new((module.set)());
        let id = set.id();
        assert!(!set.is_read(), "{id}: nothing read before it's chosen");
        for (name, language) in
            [("index.php", "php"), ("app.ts", "typescript"), ("composer.json", "json")]
        {
            let icon = set.file(name, language);
            assert!(icon.is_some_and(|i| set.svg(i).is_some()), "{id}: {name}");
        }
        assert_ne!(set.file("index.php", "php"), set.file("x.unknown", ""), "{id}: php's own");
        for open in [false, true] {
            assert!(set.folder("src", open).is_some(), "{id}: src");
            assert!(set.folder("whatever", open).is_some(), "{id}: a folder");
        }
        let mut icons = icons::Icons::builtin();
        let place = icons.register_file_set(set.clone());
        icons.set_file_set(Some(place));
        // Each extension's drawing draws, in color (resvg reads it, it covers something).
        let extensions = set.rules.lines().filter_map(|l| l.strip_prefix("e\t"));
        for extension in extensions.filter_map(|l| l.split('\t').next()) {
            let (icon, _) = icons.file(&format!("x.{extension}"), "file", "");
            let glyph = icons.rasterize_key(icons.key(icon, 24));
            let drawn = glyph.is_some_and(|g| g.format == render_api::GlyphFormat::Color);
            assert!(drawn, "{id}: .{extension} draws nothing");
        }
        let (icon, tint) = icons.file("index.php", "file-code", "php");
        assert_eq!(tint, icons::Tint::Own, "{id}");
        let glyph = icons.rasterize_key(icons.key(icon, 24)).expect("drawn");
        assert_eq!(glyph.format, render_api::GlyphFormat::Color, "{id}");
        icons.set_file_set(None);
        assert!(!set.is_read(), "{id}: released when another is chosen");
    }
}

/// Memory: with every pack registered, only the chosen one reads its data; choosing
/// another lets the first go; ours chosen, none holds anything.
#[test]
fn only_the_chosen_pack_holds_anything() {
    let packs: Vec<Arc<DataPack>> = modules().iter().map(|m| Arc::new((m.pack)())).collect();
    let mut icons = icons::Icons::builtin();
    let looks: Vec<icons::IconLook> =
        packs.iter().map(|p| icons.register_pack(p.clone() as Arc<dyn IconPack>)).collect();
    let read = || packs.iter().filter(|p| p.is_read()).count();
    let draw = |icons: &icons::Icons| {
        for name in ["search", "folder", "trash", "plus"] {
            let id = icons.id(name).unwrap();
            icons.rasterize_key(icons.key(id, 24));
        }
    };
    draw(&icons);
    assert_eq!(read(), 0, "ours drawn: no pack read");
    icons.set_look(looks[2]);
    draw(&icons);
    assert!(packs[2].is_read());
    assert_eq!(read(), 1, "only the chosen one");
    icons.set_look(looks[5]);
    draw(&icons);
    assert!(!packs[2].is_read() && packs[5].is_read(), "the first let go");
    icons.set_look(icons::IconLook::default());
    assert_eq!(read(), 0, "ours again: none holds anything");
}
