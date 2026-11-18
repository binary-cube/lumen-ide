use super::*;

fn mono(size: f32) -> Font {
    Font::new(FontFamily::Monospace, size)
}

#[test]
fn shapes_ascii_with_increasing_positions() {
    let mut text = TextSystem::new();
    let line = text.shape_line("abc", &mono(16.0));
    assert_eq!(line.glyphs.len(), 3);
    assert!(line.glyphs[0].x < line.glyphs[1].x && line.glyphs[1].x < line.glyphs[2].x);
    assert!(line.width > 0.0);
}

#[test]
fn maps_between_bytes_and_x() {
    let mut text = TextSystem::new();
    let line = text.shape_line("aăb", &mono(16.0));
    let advance = line.glyphs[0].advance;
    assert_eq!(line.x_at(0), line.glyphs[0].x);
    assert_eq!(line.x_at(3), line.glyphs[2].x); // `ă` is 2 bytes
    assert_eq!(line.x_at(99), line.width);
    assert_eq!(line.byte_at(advance * 0.4, 4), 0);
    assert_eq!(line.byte_at(advance * 0.6, 4), 1);
    assert_eq!(line.byte_at(1000.0, 4), 4);
}

#[test]
fn same_line_is_served_from_cache() {
    let mut text = TextSystem::new();
    let a = text.shape_line("<?php echo 1;", &mono(16.0));
    let b = text.shape_line("<?php echo 1;", &mono(16.0));
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn same_glyph_at_same_subpixel_offset_gets_same_key() {
    // The key also includes the subpixel offset, so compare glyphs located at x = 0.
    let mut text = TextSystem::new();
    let a = text.shape_line("a", &mono(16.0));
    let ab = text.shape_line("ab", &mono(16.0));
    assert_eq!(a.glyphs[0].key, ab.glyphs[0].key);
    assert_ne!(ab.glyphs[0].key, ab.glyphs[1].key);
}

#[test]
fn rasterizes_visible_glyphs_only() {
    let mut text = TextSystem::new();
    let line = text.shape_line("a b", &mono(16.0));
    let a = text.rasterize(line.glyphs[0].key).expect("glifa `a` are pixeli");
    assert!(a.width > 0 && a.height > 0);
    assert_eq!(a.data.len(), (a.width * a.height) as usize);
    let space = text.rasterize(line.glyphs[1].key);
    assert!(space.is_none_or(|g| g.width == 0 || g.height == 0));
}

#[test]
fn metrics_scale_with_size() {
    let mut text = TextSystem::new();
    let small = text.metrics(&mono(10.0));
    let large = text.metrics(&mono(20.0));
    assert!(small.ascent > 0.0 && small.descent > 0.0 && small.advance > 0.0);
    assert!((large.advance / small.advance - 2.0).abs() < 0.2);
}

#[test]
fn metrics_are_kept_per_whole_font() {
    let mut text = TextSystem::new();
    let regular = Font::new(FontFamily::SansSerif, 16.0);
    let bold = regular.clone().with_weight(Font::BOLD);
    let italic = regular.clone().with_italic(true);
    assert_ne!(FontKey::of(&regular), FontKey::of(&bold));
    assert_ne!(FontKey::of(&regular), FontKey::of(&italic));
    // The regular font's metrics are cached first: the bold one still gets its own.
    let cached_regular = text.metrics(&regular);
    let cached_bold = text.metrics(&bold);
    assert_eq!(cached_regular, text.measure(&regular));
    assert_eq!(cached_bold, text.measure(&bold));
    assert_eq!(text.metrics.len(), 2);
    text.metrics(&italic);
    assert_eq!(text.metrics.len(), 3);
}

#[test]
fn unknown_glyph_key_is_ignored() {
    let mut text = TextSystem::new();
    assert!(text.rasterize(GlyphKey(999_999)).is_none());
}

/// Zooming through many sizes keeps the lines of at most `LINE_CACHE_FONTS` fonts; the
/// one still in use stays cached.
#[test]
fn line_cache_keeps_a_bounded_number_of_fonts() {
    let mut text = TextSystem::new();
    let kept = text.shape_line("abc", &mono(14.0));
    for step in 0..(LINE_CACHE_FONTS * 2) {
        text.shape_line("abc", &mono(20.0 + step as f32 * 0.25));
        text.shape_line("abc", &mono(14.0));
    }
    assert!(text.lines.fonts.len() <= LINE_CACHE_FONTS);
    assert!(Arc::ptr_eq(&kept, &text.shape_line("abc", &mono(14.0))));
}

/// A size no one asked for in a long while (one a zoom went through) gives its lines back;
/// the one in use keeps them.
#[test]
fn line_cache_lets_go_of_fonts_unused_for_long() {
    let mut text = TextSystem::new();
    text.shape_line("abc", &mono(20.0));
    let kept = text.shape_line("abc", &mono(14.0));
    assert_eq!(text.lines.fonts.len(), 2);
    // Much later (lookups of the other font only), the next sweep.
    text.lines.used[1] = LINE_CACHE_STALE * 3;
    text.lines.tick = LINE_CACHE_STALE * 4 - 1;
    assert!(Arc::ptr_eq(&kept, &text.shape_line("abc", &mono(14.0))));
    assert_eq!(text.lines.fonts.len(), 1, "the 20 px lines are gone");
}

#[test]
fn warming_up_changes_nothing_shaped() {
    let sans = |weight| Font::new(FontFamily::SansSerif, 13.0).with_weight(weight);
    let fonts = [sans(500), sans(Font::SEMIBOLD), sans(Font::BOLD), mono(13.0)];
    let mut cold = TextSystem::new();
    let mut warm = TextSystem::new();
    warm.warm_up(&fonts);
    // Only the font system's caches: no glyph key handed out, no line kept.
    assert_eq!(warm.glyph_key_count(), 0);
    for font in &fonts {
        let (a, b) =
            (cold.shape_line("Project 42 ăș", font), warm.shape_line("Project 42 ăș", font));
        assert_eq!(a.width, b.width);
        let glyphs =
            |l: &ShapedLine| l.glyphs.iter().map(|g| (g.key, g.x, g.start)).collect::<Vec<_>>();
        assert_eq!(glyphs(&a), glyphs(&b));
        assert_eq!(cold.metrics(font), warm.metrics(font));
    }
}

/// Code mixing Latin and CJK in the code font: every glyph placed (none sized infinite by a
/// face the system calls monospaced whose space has no width).
#[test]
fn mixed_latin_and_cjk_lines_are_placed() {
    let mut text = TextSystem::new();
    let keys = |line: &ShapedLine| line.glyphs.iter().map(|g| g.key).collect::<Vec<_>>();
    let before = keys(&text.shape_line("abc", &mono(13.0)));
    let faces = text.fonts.db().len();
    let line = text.shape_line(&"a中 ".repeat(20), &mono(14.0));
    assert_eq!(line.glyphs.len(), 60);
    assert!(line.glyphs.iter().all(|g| g.x.is_finite() && g.advance.is_finite()));
    assert!(line.width.is_finite() && line.width > 0.0);
    // Only the broken face left (if the system has one); other text in the same faces.
    assert!(text.fonts.db().len() + MAX_BROKEN_FACES >= faces);
    assert_eq!(keys(&text.shape_line("abc", &mono(13.0))), before);
}
