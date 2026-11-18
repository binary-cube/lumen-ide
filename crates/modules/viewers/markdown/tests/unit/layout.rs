use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::*;

/// A fresh folder of the test's own under the system temp directory.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("viewer-markdown-tests-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Writes an SVG image `width` × 1 at `path`, with the given modification time.
fn write_svg(path: &Path, width: u32, modified: SystemTime) {
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"1\">\
         <rect width=\"{width}\" height=\"1\" fill=\"red\"/></svg>"
    );
    std::fs::write(path, svg).expect("write image");
    let file = std::fs::File::options().write(true).open(path).expect("open image");
    file.set_modified(modified).expect("set modification time");
}

#[test]
fn a_layout_reads_only_sizes_and_keeps_every_one_it_uses() {
    let dir = temp_dir("sizes");
    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let count = 24;
    let mut source = String::new();
    for i in 0..count {
        write_svg(&dir.join(format!("{i}.svg")), 2, when);
        source.push_str(&format!("![{i}]({i}.svg)\n\n"));
    }
    let mut caches = Caches::inline();
    let mut text = TextSystem::new();
    let palette = Palette::default();
    for width in [600.0, 400.0] {
        let toggled = HashSet::new();
        let page =
            Page { source: &source, version: 1, width, scale: 1.0, dir: &dir, toggled: &toggled };
        let layout = Layout::build(page, &mut caches, &mut text, &palette);
        let shown = layout.items.iter().filter(|item| matches!(item, Item::Picture { .. })).count();
        assert_eq!(shown, count);
    }
    // Sizes only (the pixels are the paint's, decoded only when shown).
    assert_eq!(caches.images.sizes.len(), count);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_image_edited_on_disk_is_read_again() {
    let dir = temp_dir("reload");
    let path = dir.join("a.svg");
    let first = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_svg(&path, 2, first);
    let mut sizes = ImageSizes::inline();
    let mut pixels = ImageCache::inline();
    sizes.recheck_every = Duration::ZERO;
    let (probe, _, stamp) = sizes.size(&dir, "a.svg");
    assert_eq!((probe, stamp), (Probe::Size(2, 1), Some(first)));
    let a = pixels.image(&path, stamp, 256, 1.0).expect("decoded");
    // Unchanged: the same decoded image (the first poll only starts the clock).
    assert!(!sizes.poll());
    assert!(!sizes.poll());
    assert_eq!(pixels.image(&path, stamp, 256, 1.0).map(|i| i.id), Some(a.id));
    // Edited: read again once the stamps are looked at.
    let later = first + Duration::from_secs(60);
    write_svg(&path, 3, later);
    assert!(sizes.poll(), "the layout is built again");
    let (probe, _, stamp) = sizes.size(&dir, "a.svg");
    assert_eq!((probe, stamp), (Probe::Size(3, 1), Some(later)));
    assert_ne!(pixels.image(&path, stamp, 256, 1.0).map(|i| i.id), Some(a.id));
    // Missing: known as such.
    assert_eq!(sizes.size(&dir, "missing.svg").0, Probe::Missing);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Laying out never reads the disk: an edit is seen only once a worker looked at the
/// stamps (the interval hasn't passed: what was known stays).
#[test]
fn layout_never_reads_a_stamp() {
    let dir = temp_dir("no-stat");
    let path = dir.join("a.svg");
    let first = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_svg(&path, 2, first);
    let mut sizes = ImageSizes::inline();
    assert_eq!(sizes.size(&dir, "a.svg").0, Probe::Size(2, 1));
    write_svg(&path, 3, first + Duration::from_secs(60));
    assert!(!sizes.poll());
    assert_eq!(sizes.size(&dir, "a.svg"), (Probe::Size(2, 1), path, Some(first)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A layout's pictures carry the stamp their size came with; once another layout is in
/// place, the pixels of stamps it doesn't show go.
#[test]
fn pictures_carry_their_stamp_and_old_pixels_go() {
    let dir = temp_dir("stamped");
    let path = dir.join("a.svg");
    let first = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_svg(&path, 2, first);
    let mut caches = Caches::inline();
    let mut text = TextSystem::new();
    let palette = Palette::default();
    let toggled = HashSet::new();
    let source = "![a](a.svg)";
    let mut lay = |caches: &mut Caches| {
        let page =
            Page { source, version: 1, width: 600.0, scale: 1.0, dir: &dir, toggled: &toggled };
        Layout::build(page, caches, &mut text, &palette)
    };
    let stamp_of = |layout: &Layout| {
        layout.items.iter().find_map(|i| match i {
            Item::Picture { stamp, .. } => Some(*stamp),
            _ => None,
        })
    };
    let layout = lay(&mut caches);
    assert_eq!(stamp_of(&layout), Some(Some(first)));
    let mut pixels = ImageCache::inline();
    assert!(pixels.image(&path, Some(first), 256, 1.0).is_some());
    let later = first + Duration::from_secs(60);
    write_svg(&path, 2, later);
    caches.images.recheck_every = Duration::ZERO;
    caches.poll();
    assert!(caches.poll(), "changed on disk");
    let layout = lay(&mut caches);
    assert_eq!(stamp_of(&layout), Some(Some(later)));
    pixels.keep(&layout);
    assert!(pixels.decoded.is_empty(), "the old stamp's pixels went");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The stamp arrives with the size: a picture is decoded once, under it.
#[test]
fn a_picture_is_decoded_once_under_the_stamp_it_was_probed_with() {
    let dir = temp_dir("once");
    let path = dir.join("a.svg");
    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    write_svg(&path, 2, when);
    let mut sizes = ImageSizes::default();
    assert_eq!(sizes.size(&dir, "a.svg").0, Probe::Pending);
    let started = std::time::Instant::now();
    while !sizes.poll() {
        assert!(started.elapsed() < Duration::from_secs(10), "probed meanwhile");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (probe, _, stamp) = sizes.size(&dir, "a.svg");
    assert_eq!((probe, stamp), (Probe::Size(2, 1), Some(when)));
    let mut pixels = ImageCache::default();
    assert!(pixels.image(&path, stamp, 256, 1.0).is_none(), "asked for");
    while pixels.loading() {
        pixels.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(pixels.image(&path, stamp, 256, 1.0).is_some());
    let keys: Vec<_> = pixels.decoded.keys().cloned().collect();
    assert_eq!(keys, vec![(path.clone(), Some(when), 256)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn images_are_decoded_at_the_width_shown() {
    let dir = temp_dir("width");
    let path = dir.join("big.png");
    let mut pixmap = tiny_skia::Pixmap::new(3000, 2000).expect("pixmap");
    pixmap.fill(tiny_skia::Color::WHITE);
    pixmap.save_png(&path).expect("png");
    let mut sizes = ImageSizes::inline();
    let (probe, _, stamp) = sizes.size(&dir, "big.png");
    assert_eq!(probe, Probe::Size(3000, 2000));
    let image =
        ImageCache::inline().image(&path, stamp, decode_width(700.0), 2.0).expect("decoded");
    assert_eq!((image.width, image.height), (768, 512));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Painting never waits for an image: its size and its pixels are read on a worker.
#[test]
fn images_are_read_off_the_painting_thread() {
    let dir = temp_dir("async");
    write_svg(&dir.join("a.svg"), 2, SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000));
    let mut sizes = ImageSizes::default();
    assert_eq!(sizes.size(&dir, "a.svg").0, Probe::Pending);
    assert!(sizes.loading());
    let started = std::time::Instant::now();
    while !sizes.poll() {
        assert!(started.elapsed() < Duration::from_secs(10), "read meanwhile");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (probe, path, stamp) = sizes.size(&dir, "a.svg");
    assert_eq!(probe, Probe::Size(2, 1));
    let mut pixels = ImageCache::default();
    assert!(pixels.image(&path, stamp, 256, 1.0).is_none(), "not yet");
    while pixels.loading() {
        pixels.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(pixels.image(&path, stamp, 256, 1.0).is_some());
    // A device or a pipe is never read as an image.
    #[cfg(unix)]
    assert!(image_decode::read(Path::new("/dev/zero")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// How long a ~500 KB document takes to lay out, first and after an edit (a measure, not a
/// check: `cargo test -p viewer-markdown --release -- --ignored --nocapture layout_cost`).
#[test]
#[ignore = "a measure"]
fn layout_cost_of_a_big_document() {
    let mut source = String::new();
    let mut n = 0;
    while source.len() < 500_000 {
        n += 1;
        source.push_str(&format!(
            "## Section {n}\n\nSome **bold** text, some *italic*, a [link](https://example.com/{n}) \
             and `inline code` in a paragraph long enough to wrap over a few lines of the \
             preview, like the README of a project usually has, with words of many lengths.\n\n\
             - one item\n- another item with more words in it\n  - nested `code`\n\n\
             | Column | Another | Third |\n|---|---|---|\n| a {n} | b | c |\n| d | e | f |\n\n\
             > A quote, with a line or two of text in it.\n\n\
             ```rust\nfn main() {{\n    println!(\"{n}\");\n}}\n```\n\n"
        ));
    }
    let mut caches = Caches::default();
    let mut text = TextSystem::new();
    let palette = Palette::default();
    let toggled = HashSet::new();
    let dir = std::env::temp_dir();
    let mut times = Vec::new();
    for version in 1..=4 {
        let page = Page {
            source: &source,
            version,
            width: 1600.0,
            scale: 2.0,
            dir: &dir,
            toggled: &toggled,
        };
        let started = std::time::Instant::now();
        let layout = Layout::build(page, &mut caches, &mut text, &palette);
        times.push(started.elapsed());
        assert!(layout.height > 0.0);
    }
    println!("{} bytes, {n} sections: builds {times:?}", source.len());
}
