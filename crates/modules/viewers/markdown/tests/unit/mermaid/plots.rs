use super::super::scene::Prim;
use super::super::tests::{scene_of, texts};

/// Every text of `scene` is inside it.
fn inside(scene: &super::Scene) {
    assert!(scene.width > 0.0 && scene.height > 0.0);
    for p in &scene.prims {
        if let Prim::Text(t) = p {
            let ok = (-1.0..=scene.width + 1.0).contains(&t.at.x)
                && (-1.0..=scene.height + 1.0).contains(&t.at.y);
            assert!(ok, "{:?} at {:?} outside {}×{}", t.text, t.at, scene.width, scene.height);
        }
    }
}

#[test]
fn journey_sections_tasks_and_actors() {
    let s = scene_of(
        "journey\n    title My working day\n    section Go to work\n      Make tea: 5: Me\n      Go upstairs: 3: Me\n      Do work: 1: Me, Cat\n    section Go home\n      Go downstairs: 5: Me\n      Sit down: 5: Me",
    );
    let t = texts(&s);
    for want in ["My working day", "Go to work", "Go home", "Make tea", "Sit down", "Me", "Cat"] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}

#[test]
fn sankey_nodes_in_columns_with_their_values() {
    let s = scene_of(
        "sankey-beta\nAgricultural 'waste',Bio-conversion,124.729\nBio-conversion,Liquid,0.597\nBio-conversion,Losses,26.862\nBio-conversion,Solid,280.322\nBio-conversion,Gas,81.144",
    );
    let t = texts(&s);
    assert!(t.contains(&"Bio-conversion 388.93"), "{t:?}");
    assert!(t.contains(&"Solid 280.32"), "{t:?}");
    inside(&s);
}

#[test]
fn xychart_bars_lines_and_axes() {
    let src = "xychart-beta\n    title \"Sales Revenue\"\n    x-axis [jan, feb, mar, apr, may, jun, jul, aug, sep, oct, nov, dec]\n    y-axis \"Revenue (in $)\" 4000 --> 11000\n    bar [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]\n    line [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]";
    let s = scene_of(src);
    let t = texts(&s);
    for want in ["Sales Revenue", "jan", "dec", "Revenue (in $)", "4000", "10000"] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
    let bars = s.prims.iter().filter(|p| matches!(p, Prim::Path { .. })).count();
    assert!(bars >= 12, "{bars}");
    let h = scene_of(&src.replacen("xychart-beta", "xychart-beta horizontal", 1));
    assert!(texts(&h).contains(&"jan"));
    inside(&h);
}

#[test]
fn quadrant_names_axes_and_points() {
    let s = scene_of(
        "quadrantChart\n    title Reach and engagement of campaigns\n    x-axis Low Reach --> High Reach\n    y-axis Low Engagement --> High Engagement\n    quadrant-1 We should expand\n    quadrant-2 Need to promote\n    quadrant-3 Re-evaluate\n    quadrant-4 May be improved\n    Campaign A: [0.3, 0.6]\n    Campaign B: [0.45, 0.23]",
    );
    let t = texts(&s);
    for want in ["We should expand", "Re-evaluate", "Low Reach", "High Engagement", "Campaign A"] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}

#[test]
fn radar_axes_curves_and_legend() {
    let s = scene_of(
        "radar-beta\n  title Grades\n  axis m[\"Math\"], s[\"Science\"], e[\"English\"]\n  axis h[\"History\"], g[\"Geography\"], a[\"Art\"]\n  curve a[\"Alice\"]{85, 90, 80, 70, 75, 90}\n  curve b[\"Bob\"]{70, 75, 85, 80, 90, 85}\n  max 100\n  min 0",
    );
    let t = texts(&s);
    for want in ["Grades", "Math", "Art", "Alice", "Bob"] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}

#[test]
fn packet_fields_on_rows_of_bits() {
    let s = scene_of(
        "packet-beta\ntitle UDP Packet\n0-15: \"Source Port\"\n16-31: \"Destination Port\"\n32-47: \"Length\"\n48-63: \"Checksum\"\n64-95: \"Data (variable length)\"",
    );
    let t = texts(&s);
    for want in ["UDP Packet", "Source Port", "Checksum", "0", "15", "31", "64", "95"] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}

#[test]
fn treemap_sections_and_leaves() {
    let s = scene_of(
        "treemap-beta\n\"Section 1\"\n    \"Leaf 1.1\": 12\n    \"Section 1.2\"\n      \"Leaf 1.2.1\": 12\n\"Section 2\"\n    \"Leaf 2.1\": 20\n    \"Leaf 2.2\": 25",
    );
    let t = texts(&s);
    assert!(t.contains(&"Section 1  24") && t.contains(&"Section 2  45"), "{t:?}");
    assert!(t.iter().any(|s| s.starts_with("Leaf 2.2")), "{t:?}");
    inside(&s);
}

#[test]
fn squarified_rectangles_fill_their_area() {
    let values = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
    let items: Vec<usize> = (0..values.len()).collect();
    let area = kernel::Rect::new(0.0, 0.0, 600.0, 400.0);
    let placed = super::squarify(&items, &values, area);
    assert_eq!(placed.len(), values.len());
    let total: f32 = placed.iter().map(|(_, r)| r.size.width * r.size.height).sum();
    assert!((total - 240_000.0).abs() < 1.0, "{total}");
    for (i, r) in &placed {
        let share = r.size.width * r.size.height / 240_000.0;
        assert!((share - values[*i] as f32 / 24.0).abs() < 1e-3);
        assert!(r.max_x() <= 600.01 && r.max_y() <= 400.01);
    }
}
