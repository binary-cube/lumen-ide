use super::super::scene::Prim;
use super::super::tests::{scene_of, texts};

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
fn requirements_with_their_fields_and_relations() {
    let s = scene_of(
        "requirementDiagram\n\n    requirement test_req {\n    id: 1\n    text: the test text.\n    risk: high\n    verifymethod: test\n    }\n\n    element test_entity {\n    type: simulation\n    }\n\n    test_entity - satisfies -> test_req",
    );
    let t = texts(&s);
    for want in ["«requirement»", "test_req", "id: 1", "risk: high", "«element»", "«satisfies»"]
    {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}

#[test]
fn c4_people_systems_boundaries_and_rels() {
    let s = scene_of(
        "C4Context\n  title System Context diagram for Internet Banking System\n  Enterprise_Boundary(b0, \"BankBoundary0\") {\n    Person(customerA, \"Banking Customer A\", \"A customer of the bank, with personal bank accounts.\")\n    System(SystemAA, \"Internet Banking System\", \"Allows customers to view information about their bank accounts, and make payments.\")\n    System_Ext(SystemE, \"Mainframe Banking System\", \"Stores all of the core banking information.\")\n  }\n  Rel(customerA, SystemAA, \"Uses\")\n  Rel(SystemAA, SystemE, \"Uses\", \"HTTPS\")",
    );
    let t = texts(&s);
    for want in [
        "Banking Customer A",
        "[Person]",
        "Internet Banking System",
        "[Software System]",
        "Uses",
        "[HTTPS]",
        "BankBoundary0",
    ] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
    let c = scene_of(
        "C4Container\n  Container(spa, \"Single-Page App\", \"JavaScript, Angular\", \"Provides banking.\")\n  ContainerDb(db, \"Database\", \"SQL Database\", \"Stores data.\")\n  Rel(spa, db, \"Reads\")",
    );
    let t = texts(&c);
    assert!(t.contains(&"[Container: JavaScript, Angular]") && t.contains(&"Database"), "{t:?}");
}

#[test]
fn architecture_services_groups_and_junctions() {
    let s = scene_of(
        "architecture-beta\n    group api(cloud)[API]\n\n    service db(database)[Database] in api\n    service disk1(disk)[Storage] in api\n    service disk2(disk)[Storage] in api\n    service server(server)[Server] in api\n    junction mid in api\n\n    db:L -- R:server\n    disk1:T -- B:server\n    disk2:T -- B:db\n    server:R --> L:mid",
    );
    let t = texts(&s);
    for want in ["API", "Database", "Storage", "Server"] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}

#[test]
fn blocks_on_their_grid() {
    let s = scene_of(
        "block-beta\ncolumns 3\n  a[\"A label\"] b:2 c:2 d\n  block:group1:2\n    columns 2\n    h i j k\n  end\n  e\n  a --> e",
    );
    let t = texts(&s);
    for want in ["A label", "b", "d", "h", "k", "e"] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}

#[test]
fn block_spans_share_the_columns() {
    use diagram_text::mermaid::{self, Diagram};
    let parsed = mermaid::parse("block-beta\ncolumns 3\n  a b:2\n  c:3").expect("parsed");
    let Diagram::Block(d) = parsed.diagram else { panic!() };
    let sizes = vec![(60.0, 38.0); d.blocks.len()];
    let mut rects = vec![kernel::Rect::default(); d.blocks.len()];
    let (w, h) = super::grid_size(&d, &d.top, d.columns, &sizes);
    super::place(&d, &d.top, d.columns, kernel::Rect::new(0.0, 0.0, w, h), &sizes, &mut rects);
    let [a, b, c] = [rects[d.top[0]], rects[d.top[1]], rects[d.top[2]]];
    assert!((a.min_y() - b.min_y()).abs() < 0.01 && c.min_y() > a.max_y());
    assert!((b.size.width - (2.0 * a.size.width + super::BLOCK_GAP)).abs() < 0.01);
    assert!((c.size.width - w).abs() < 0.01);
}

#[test]
fn kanban_columns_and_cards() {
    let s = scene_of(
        "kanban\n  Todo\n    [Create Documentation]\n    docs[Create Blog about the new diagram]\n  [In progress]\n    id6[Create renderer so that it works in all cases]\n  id9[Ready for deploy]\n    id8[Design grammar]@{ assigned: 'knsv', ticket: MC-2038, priority: 'High' }",
    );
    let t = texts(&s);
    for want in [
        "Todo",
        "In progress",
        "Ready for deploy",
        "Create Documentation",
        "Design grammar",
        "MC-2038 · knsv",
    ] {
        assert!(t.contains(&want), "{want} in {t:?}");
    }
    inside(&s);
}
