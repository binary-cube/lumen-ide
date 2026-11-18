//! The view over a fake source: read in the background, a table's neighbors, the filter,
//! clicks centering, double clicks opening, ← going back, its menus.

use std::time::{Duration, Instant};

use database::diagram::{Origin, SchemaAccess, SchemaGraph};
use database::relations::{Confidence, RelationKinds, RelationNotes};
use database::{
    ColumnInfo, DataSourceId, FkAction, ForeignKey, Structure, TableInfo, TableKind, TableRef,
};
use kernel::EventBus;

use super::*;

struct Fake {
    id: DataSourceId,
    tables: Vec<(TableInfo, Structure)>,
    /// What the source keeps of the decisions.
    notes: Mutex<RelationNotes>,
}

impl SchemaAccess for Fake {
    fn source(&self) -> DataSourceId {
        self.id
    }

    fn name(&self) -> String {
        "shop".into()
    }

    fn read(
        &self,
        _schema: Option<&str>,
        _cancel: &Cancel,
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<Vec<(TableInfo, Structure)>, DbError> {
        progress(self.tables.len(), self.tables.len());
        Ok(self.tables.clone())
    }

    fn open(&self, table: &TableRef) -> Option<Opening> {
        Some(Opening::View(database::diagram::TableView {
            title: format!("open {}", table.name),
            extension: "view-table",
            data: std::sync::Arc::new(()),
        }))
    }

    fn notes(&self) -> RelationNotes {
        self.notes.lock().unwrap().clone()
    }

    fn set_notes(&self, notes: RelationNotes) {
        *self.notes.lock().unwrap() = notes;
    }
}

fn table(name: &str, keys: &[(&str, &str)]) -> (TableInfo, Structure) {
    let table = TableRef::new(name);
    let mut id = ColumnInfo::new("id", "INTEGER");
    id.in_key = true;
    let mut columns = vec![id];
    let foreign_keys = keys
        .iter()
        .map(|(c, t)| {
            columns.push(ColumnInfo::new(*c, "INTEGER"));
            ForeignKey {
                name: None,
                columns: vec![(*c).into()],
                table: TableRef::new(*t),
                references: vec!["id".into()],
                on_update: FkAction::NoAction,
                on_delete: FkAction::NoAction,
            }
        })
        .collect();
    let info = TableInfo { table: table.clone(), kind: TableKind::Table, rows: Some(2) };
    (info, Structure { table, columns, foreign_keys, ..Structure::default() })
}

fn tables() -> Vec<(TableInfo, Structure)> {
    vec![
        table("users", &[]),
        table("orders", &[("user_id", "users")]),
        table("lines", &[("order_id", "orders"), ("product_id", "products")]),
        table("products", &[]),
        table("audit", &[]),
    ]
}

/// The tables, and reviews and payments only naming what they point to.
fn named() -> Vec<(TableInfo, Structure)> {
    let mut all = tables();
    let mut plain = |name: &str, columns: &[&str]| {
        let (info, mut structure) = table(name, &[]);
        structure.columns.extend(columns.iter().map(|c| ColumnInfo::new(*c, "INTEGER")));
        all.push((info, structure));
    };
    plain("reviews", &["productId", "userid"]);
    plain("payments", &["order_id", "paid"]);
    all
}

fn request(focus: Option<&str>, events: Option<EventBus>) -> DiagramRequest {
    DiagramRequest {
        access: Arc::new(Fake {
            id: DataSourceId::new(),
            tables: tables(),
            notes: Mutex::default(),
        }),
        events,
        schema: None,
        focus: focus.map(TableRef::new),
    }
}

/// The view as a tab shows it (its area known), the background waited for.
fn opened(focus: Option<&str>) -> RelationsView {
    let mut v = RelationsView::default();
    v.set_data(&request(focus, None));
    settle(&mut v);
    v.view.scale = 1.0;
    v.view.bounds = Rect::new(0.0, 0.0, 1600.0, 900.0);
    v
}

fn settle(v: &mut RelationsView) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while v.busy() && Instant::now() < deadline {
        v.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!v.busy(), "read and laid out");
}

fn index(v: &RelationsView, name: &str) -> usize {
    v.graph().unwrap().find(&TableRef::new(name)).unwrap()
}

fn names(v: &RelationsView) -> Vec<String> {
    let g = v.graph().unwrap();
    v.shown().tables.iter().map(|&t| g.tables[t].table().name.clone()).collect()
}

#[test]
fn the_structure_is_read_and_laid_out_in_the_background() {
    let v = opened(None);
    assert_eq!(v.graph().unwrap().tables.len(), 5);
    assert_eq!(v.layout().layout.cards.len(), 5);
    assert_eq!(v.shown().links.len(), 3);
    assert!(v.view.fit_pending, "fitted at its first paint");
    assert_eq!(v.status().unwrap(), tr!("relations.status", tables = 5, links = 3));
}

#[test]
fn a_table_asked_first_shows_its_neighbors_at_the_depth_chosen() {
    let mut v = opened(Some("orders"));
    let orders = index(&v, "orders");
    assert_eq!(v.focus(), Some((orders, 1)));
    assert_eq!(v.selected(), Some(orders));
    assert_eq!(names(&v), ["lines", "orders", "users"]);
    assert!(v.command("depth:2"));
    settle(&mut v);
    assert_eq!(names(&v), ["lines", "orders", "products", "users"]);
    assert!(v.command("all"));
    settle(&mut v);
    assert_eq!(names(&v).len(), 5);
    // ← back through each.
    assert!(v.back());
    settle(&mut v);
    assert_eq!(v.focus(), Some((orders, 2)));
    assert!(v.back());
    settle(&mut v);
    assert_eq!(v.focus(), Some((orders, 1)));
    assert!(!v.back());
}

#[test]
fn the_filter_narrows_the_tables_by_name() {
    let mut v = opened(None);
    assert!(v.command("find"));
    assert!(v.wants_text());
    v.text_input("ord");
    settle(&mut v);
    assert_eq!(names(&v), ["orders"]);
    assert!(v.shown().links.is_empty());
    v.edit_key(ViewerKey::Escape);
    settle(&mut v);
    assert_eq!(names(&v).len(), 5, "Esc clears it");
    v.edit_key(ViewerKey::Escape);
    assert!(!v.wants_text(), "then gives the keys back");
    // Enter centers the first match.
    v.command("find");
    v.text_input("prod");
    settle(&mut v);
    v.edit_key(ViewerKey::Enter);
    assert_eq!(v.selected(), Some(index(&v, "products")));
}

#[test]
fn a_click_centers_a_card_a_double_click_opens_its_data() {
    let mut v = opened(None);
    let users = index(&v, "users");
    let card = v.layout().card_of(users).unwrap();
    let header = v.view.rect_to_screen(v.layout().layout.cards[card].rect);
    v.hits = vec![(header, Hit::Card(users))];
    let at = Point::new(header.min_x() + 5.0, header.min_y() + 5.0);
    assert_eq!(v.pointer_down(at, 1, PressMods::default()), None);
    assert!(v.release());
    assert_eq!(v.selected(), Some(users));
    // Centered: its middle is the area's.
    let moved = v.view.rect_to_screen(v.layout().layout.cards[card].rect);
    let area = v.view.area();
    assert!(
        (moved.min_x() + moved.size.width / 2.0 - (area.min_x() + area.size.width / 2.0)).abs()
            <= 1.0
    );
    // The second click (where the card was): its data.
    assert!(matches!(
        v.pointer_down(at, 2, PressMods::default()),
        Some(ViewerAction::OpenView(o)) if o.title == "open users"
    ));
    // A drag pans, selecting nothing.
    v.pointer_down(Point::new(10.0, 300.0), 1, PressMods::default());
    assert!(v.drag(Point::new(80.0, 300.0)));
    v.release();
    assert_eq!(v.selected(), Some(users));
}

#[test]
fn a_cards_menu_opens_shows_neighbors_and_copies() {
    let mut v = opened(None);
    let lines = index(&v, "lines");
    v.hits = vec![(Rect::new(0.0, 0.0, 100.0, 40.0), Hit::Card(lines))];
    let menu = v.context_menu(Point::new(5.0, 5.0)).unwrap();
    let ids: Vec<String> = menu
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        [format!("open:{lines}"), format!("neighbors:{lines}"), format!("copy:{lines}")]
    );
    assert!(v.command(&format!("neighbors:{lines}")));
    settle(&mut v);
    assert_eq!(names(&v), ["lines", "orders", "products"]);
    assert!(v.command(&format!("copy:{lines}")));
    assert_eq!(v.take_action(), Some(ViewerAction::Request(AppRequest::CopyText("lines".into()))));
    assert!(v.command(&format!("open:{lines}")));
    assert!(matches!(v.take_action(), Some(ViewerAction::OpenView(o)) if o.title == "open lines"));
}

#[test]
fn a_structure_changed_by_a_commit_is_read_again() {
    let bus = EventBus::new();
    let request = request(Some("orders"), Some(bus.clone()));
    let id = request.access.source();
    let mut v = RelationsView::default();
    v.set_data(&request);
    settle(&mut v);
    bus.publish(&database::events::DataChanged {
        source: id,
        table: TableRef::new("orders"),
        structure: true,
    });
    assert!(v.poll(), "read again");
    settle(&mut v);
    // Kept around the same table.
    assert_eq!(v.focus().map(|(f, _)| f), Some(index(&v, "orders")));
    // Another source's commit isn't this one's.
    bus.publish(&database::events::DataChanged {
        source: DataSourceId::new(),
        table: TableRef::new("x"),
        structure: true,
    });
    assert!(!v.changed.load(Ordering::Acquire));
}

#[test]
fn the_same_graph_lays_out_the_same() {
    let mut a = RelationsView::default();
    a.set_graph(SchemaGraph::new(tables()));
    a.lay_out_now();
    let mut b = RelationsView::default();
    b.set_graph(SchemaGraph::new(tables()));
    b.lay_out_now();
    assert_eq!(a.layout(), b.layout());
}

fn opened_named() -> (RelationsView, Arc<Fake>) {
    let fake = Arc::new(Fake { id: DataSourceId::new(), tables: named(), notes: Mutex::default() });
    let mut v = RelationsView::default();
    v.set_data(&DiagramRequest {
        access: Arc::clone(&fake) as _,
        events: None,
        schema: None,
        focus: None,
    });
    settle(&mut v);
    (v, fake)
}

fn origins(v: &RelationsView) -> Vec<(String, Origin)> {
    let g = v.graph().unwrap();
    v.shown().links.iter().map(|&l| (g.label(&g.links[l]), g.links[l].origin)).collect()
}

#[test]
fn relations_found_from_names_are_shown_dashed_and_chosen_by_kind() {
    let (mut v, _) = opened_named();
    let inferred: Vec<String> = origins(&v)
        .into_iter()
        .filter(|(_, o)| matches!(o, Origin::Inferred(_)))
        .map(|(l, _)| l)
        .collect();
    assert_eq!(
        inferred,
        [
            "payments.order_id → orders.id",
            "reviews.productId → products.id",
            "reviews.userid → users.id"
        ]
    );
    assert_eq!(v.undecided().len(), 3);
    assert!(v.command("kinds:real"));
    settle(&mut v);
    assert!(origins(&v).iter().all(|(_, o)| *o == Origin::Real));
    assert!(v.command("kinds:inferred"));
    settle(&mut v);
    assert!(origins(&v).iter().all(|(_, o)| matches!(o, Origin::Inferred(_))));
    assert!(v.command("kinds:both"));
    settle(&mut v);
    assert_eq!(origins(&v).len(), 6);
    // `userid` is less sure than `productId`.
    let sure = |label: &str| match origins(&v).into_iter().find(|(l, _)| l == label).unwrap().1 {
        Origin::Inferred(c) => c,
        other => panic!("{other:?}"),
    };
    assert_eq!(sure("reviews.productId → products.id"), Confidence::High);
    assert_eq!(sure("reviews.userid → users.id"), Confidence::Medium);
}

#[test]
fn accepted_and_ignored_relations_are_kept_with_the_source_and_undone() {
    let (mut v, fake) = opened_named();
    let waiting = v.undecided().to_vec();
    assert!(v.command(&format!("accept:{}", waiting[1])));
    assert!(v.command(&format!("ignore:{}", waiting[0])));
    settle(&mut v);
    let kept = fake.notes();
    assert_eq!(kept.virtual_keys.len(), 1);
    assert_eq!(kept.virtual_keys[0].columns, ["productId"]);
    assert_eq!(kept.ignored[0].columns, ["order_id"]);
    let all = origins(&v);
    assert!(all.contains(&("reviews.productId → products.id".into(), Origin::Virtual)));
    assert!(!all.iter().any(|(l, _)| l.starts_with("payments")), "set aside: hidden");
    assert_eq!(v.undecided().len(), 1);
    // The kind shown is kept too, and read back by the next view.
    v.show_kinds(RelationKinds::Real);
    assert_eq!(fake.notes().show, RelationKinds::Real);
    settle(&mut v);
    assert!(origins(&v).contains(&("reviews.productId → products.id".into(), Origin::Virtual)));
    let mut again = RelationsView::default();
    again.set_data(&DiagramRequest {
        access: Arc::clone(&fake) as _,
        events: None,
        schema: None,
        focus: None,
    });
    settle(&mut again);
    assert_eq!(again.notes(), &fake.notes());
    // Undone from the list: found again.
    v.show_kinds(RelationKinds::Both);
    assert!(v.command("unaccept:0"));
    assert!(v.command("unignore:0"));
    settle(&mut v);
    assert_eq!(v.undecided().len(), 3);
    assert!(fake.notes().virtual_keys.is_empty() && fake.notes().ignored.is_empty());
}

/// The view as a tab shows it.
fn sized(v: &mut RelationsView) {
    v.view.scale = 1.0;
    v.view.bounds = Rect::new(0.0, 0.0, 1600.0, 900.0);
}

/// The place in the graph of the link `label` (`orders.user_id → users.id`).
fn link(v: &RelationsView, label: &str) -> usize {
    let g = v.graph().unwrap();
    g.links.iter().position(|l| g.label(l) == label).unwrap()
}

/// A screen point on the middle of `link`'s line, the view panned so it's in view.
fn on_line(v: &mut RelationsView, link: usize) -> Point {
    let edge = v.layout().layout.edges.iter().find(|e| e.kind == link).unwrap().clone();
    let (a, b) = (edge.points[1], edge.points[2]);
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    v.view.center_on(Rect::new(mid.x, mid.y, 0.0, 0.0));
    v.view.to_screen(mid)
}

fn middle(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

#[test]
fn the_bar_and_the_list_are_left_free_when_fitting_and_centering() {
    let mut v = opened(None);
    v.update_insets();
    assert_eq!(v.view.insets.bottom, BAR_INSET);
    assert_eq!(v.view.insets.right, 0.0);
    assert!(v.command("found"));
    let list = list_width(v.view.area().size.width, 1.0);
    assert_eq!(v.view.insets.right, list + 24.0, "the list open covers the right");
    // A table selected: centered in what's left, not under the list.
    let users = index(&v, "users");
    v.select(users);
    let card = v.layout().card_of(users).unwrap();
    let at = middle(v.view.rect_to_screen(v.layout().layout.cards[card].rect));
    let free = middle(v.view.visible());
    assert!((at.x - free.x).abs() <= 1.0 && (at.y - free.y).abs() <= 1.0, "{at:?} {free:?}");
    // Fitted: every card above the bar, left of the list.
    v.view.fit(v.layout().layout.bounds, None, FIT_MIN, FIT_MAX);
    let visible = v.view.visible();
    for c in &v.layout().layout.cards {
        let r = v.view.rect_to_screen(c.rect);
        assert!(ui_components::diagram::contains_rect(visible, r), "{r:?} in {visible:?}");
    }
}

#[test]
fn labels_placed_on_the_layout_cover_no_card() {
    let (mut v, _) = opened_named();
    sized(&mut v);
    let layout = v.layout().layout.clone();
    let cards: Vec<Rect> = layout.cards.iter().map(|c| c.rect).collect();
    let spots: Vec<Vec<Rect>> = layout
        .edges
        .iter()
        .map(|e| ui_components::diagram::label_spots(&e.points, (110.0, 18.0), 5.0))
        .collect();
    let placed = ui_components::diagram::place_labels(&spots, &cards);
    let mut kept = Vec::new();
    for (spots, chosen) in spots.iter().zip(&placed) {
        let Some(i) = chosen else { continue };
        let r = spots[*i];
        assert!(cards.iter().all(|c| c.intersect(&r).is_none()), "{r:?} over a card");
        assert!(kept.iter().all(|k: &Rect| k.intersect(&r).is_none()), "{r:?} over a label");
        kept.push(r);
    }
    assert!(!kept.is_empty(), "the lines between columns have room");
}

#[test]
fn a_line_is_hit_within_a_few_pixels_scaled_with_the_zoom() {
    let (mut v, _) = opened_named();
    sized(&mut v);
    let l = link(&v, "orders.user_id → users.id");
    let edge = v.layout().layout.edges.iter().find(|e| e.kind == l).unwrap().clone();
    let vertical = (edge.points[1].x - edge.points[2].x).abs() < 0.5;
    let p = on_line(&mut v, l);
    let off = |d: f32| if vertical { Point::new(p.x + d, p.y) } else { Point::new(p.x, p.y + d) };
    assert_eq!(v.edge_at(p), Some(l));
    assert_eq!(v.edge_at(off(3.5)), Some(l));
    assert_eq!(v.edge_at(off(9.0)), None);
    // Zoomed in, the reach grows with it.
    v.view.zoom = 2.0;
    let p = on_line(&mut v, l);
    let off = |d: f32| if vertical { Point::new(p.x + d, p.y) } else { Point::new(p.x, p.y + d) };
    assert_eq!(v.edge_at(off(7.0)), Some(l));
    assert_eq!(v.hit(off(7.0)), Some(Hit::Edge(l)));
    // A card or a bar over it wins.
    v.hits = vec![(Rect::new(p.x - 20.0, p.y - 20.0, 40.0, 40.0), Hit::Bar)];
    assert_eq!(v.hit(p), Some(Hit::Bar));
}

/// A click at `p` (pressed and released there).
fn click(v: &mut RelationsView, p: Point) {
    v.pointer_down(p, 1, PressMods::default());
    v.release();
}

/// A click on a popover's button `hit`.
fn press(v: &mut RelationsView, hit: Hit) {
    let r = Rect::new(5.0, 300.0, 60.0, 20.0);
    v.hits = vec![(r, hit)];
    click(v, Point::new(10.0, 305.0));
    v.hits.clear();
}

#[test]
fn a_line_found_from_names_is_accepted_or_set_aside_from_its_popover() {
    let (mut v, fake) = opened_named();
    sized(&mut v);
    let found = link(&v, "reviews.productId → products.id");
    let p = on_line(&mut v, found);
    v.hits.clear();
    click(&mut v, p);
    assert_eq!(v.selected_edge(), Some(found));
    assert_eq!(v.popover_edge(), Some(found));
    press(&mut v, Hit::PopAccept);
    assert_eq!(fake.notes().virtual_keys[0].columns, ["productId"], "kept with the source");
    // Still open, the line now a virtual key: its decision undone from there.
    let accepted = v.popover_edge().unwrap();
    assert_eq!(v.graph().unwrap().links[accepted].origin, Origin::Virtual);
    press(&mut v, Hit::PopUndo);
    assert!(fake.notes().virtual_keys.is_empty());
    let again = v.popover_edge().unwrap();
    assert!(matches!(v.graph().unwrap().links[again].origin, Origin::Inferred(_)));
    // Set aside: kept, the line and its popover gone.
    press(&mut v, Hit::PopIgnore);
    assert_eq!(fake.notes().ignored[0].columns, ["productId"]);
    assert_eq!(v.popover_edge(), None);
    assert_eq!(v.selected_edge(), None);
    assert_eq!(v.undecided().len(), 2);
}

#[test]
fn a_keys_line_is_selected_and_leads_to_its_tables() {
    let (mut v, _) = opened_named();
    sized(&mut v);
    settle(&mut v);
    let real = link(&v, "orders.user_id → users.id");
    let p = on_line(&mut v, real);
    v.hits.clear();
    click(&mut v, p);
    assert_eq!(v.selected_edge(), Some(real));
    assert_eq!(v.popover_edge(), Some(real));
    // Esc closes the popover, then lets the line go.
    assert!(v.edit_key(ViewerKey::Escape));
    assert_eq!(v.popover_edge(), None);
    assert_eq!(v.selected_edge(), Some(real));
    assert!(v.edit_key(ViewerKey::Escape));
    assert_eq!(v.selected_edge(), None);
    assert!(!v.edit_key(ViewerKey::Escape));
    // A click outside closes it; one on nothing lets the line go.
    click(&mut v, p);
    assert_eq!(v.popover_edge(), Some(real));
    let nowhere = Point::new(1590.0, 60.0);
    assert_eq!(v.hit(nowhere), None);
    click(&mut v, nowhere);
    assert_eq!(v.popover_edge(), None);
    assert_eq!(v.selected_edge(), None);
    // To a table from it.
    click(&mut v, p);
    let users = index(&v, "users");
    press(&mut v, Hit::GoTo(users));
    assert_eq!(v.selected(), Some(users));
    assert_eq!(v.popover_edge(), None);
}

#[test]
fn tab_goes_through_the_selected_tables_lines() {
    let mut v = opened(None);
    let orders = index(&v, "orders");
    v.select(orders);
    let lines: Vec<usize> = {
        let g = v.graph().unwrap();
        (0..g.links.len())
            .filter(|&l| g.links[l].from == orders || g.links[l].to == orders)
            .collect()
    };
    assert_eq!(lines.len(), 2);
    assert!(v.edit_key(ViewerKey::Tab));
    let first = v.selected_edge().unwrap();
    assert!(v.edit_key(ViewerKey::Tab));
    let second = v.selected_edge().unwrap();
    assert_ne!(first, second);
    assert!(lines.contains(&first) && lines.contains(&second));
    assert!(v.edit_key(ViewerKey::Tab));
    assert_eq!(v.selected_edge(), Some(first), "round again");
    // Enter: its popover.
    assert!(v.edit_key(ViewerKey::Enter));
    assert_eq!(v.popover_edge(), Some(first));
    // By the column it starts from (the snapshot's).
    assert!(v.command("edge:orders.user_id"));
    assert_eq!(v.popover_edge(), Some(link(&v, "orders.user_id → users.id")));
    assert!(!v.command("edge:orders.nothing"));
}

/// The keys a command is bound to, as the tests' keymap has them.
fn test_keys(id: &str) -> Option<String> {
    match id {
        "database.diagram_back" => Some("⌫".into()),
        "database.diagram_find" => Some("⌘F".into()),
        _ => None,
    }
}

#[test]
fn every_control_has_a_tooltip_with_its_keys() {
    ui_components::shortcuts::set_reader(test_keys);
    let fake = Arc::new(Fake { id: DataSourceId::new(), tables: named(), notes: Mutex::default() });
    let mut v = RelationsView::default();
    v.set_data(&DiagramRequest {
        access: Arc::clone(&fake) as _,
        events: None,
        schema: None,
        focus: Some(TableRef::new("orders")),
    });
    settle(&mut v);
    // ← (a step back to go), the filter's ×, the list of those found open.
    assert!(v.command("depth:2"));
    settle(&mut v);
    v.filter.set_text("o");
    assert!(v.command("found"));
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    v.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1800.0, 900.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let controls = |h: &Hit| {
        matches!(
            h,
            Hit::Back | Hit::Clear | Hit::Field | Hit::Depth(_) | Hit::Kinds(_) | Hit::Found
        )
    };
    let mut seen = Vec::new();
    for (r, hit) in v.hits.clone().iter().filter(|(_, h)| controls(h)) {
        let tip = v.tooltip_at(middle(*r)).unwrap_or_else(|| panic!("no tooltip on {hit:?}"));
        assert!(!tip.label.trim().is_empty(), "{hit:?}");
        seen.push((*hit, tip));
    }
    let kinds = seen.iter().filter(|(h, _)| matches!(h, Hit::Kinds(_))).count();
    let depths = seen.iter().filter(|(h, _)| matches!(h, Hit::Depth(_))).count();
    let founds = seen.iter().filter(|(h, _)| *h == Hit::Found).count();
    assert_eq!((kinds, depths, founds), (3, DEPTHS.len(), 2), "the button and the list's ×");
    let keys =
        |hit: Hit| seen.iter().find(|(h, _)| *h == hit).and_then(|(_, t)| t.shortcut.clone());
    assert_eq!(keys(Hit::Back).as_deref(), Some("⌫"));
    assert_eq!(keys(Hit::Field).as_deref(), Some("⌘F"));
    assert!(seen.iter().any(|(h, _)| *h == Hit::Clear));
    // The legend: what each kind of line is.
    let legend: Vec<&Tip> = v.tips.iter().filter(|(h, _)| *h == Hit::Bar).map(|(_, t)| t).collect();
    assert!(!legend.is_empty());
    for tip in legend {
        assert_eq!(v.tooltip_at(middle(tip.anchor)).as_ref(), Some(tip));
    }
}

/// ⌘A, ⌘X, ⌘Z while the search has the keys: its text, the tables filtered again.
#[test]
fn the_app_s_editing_commands_reach_the_search() {
    let mut v = opened(None);
    v.command("find");
    v.text_input("ord");
    settle(&mut v);
    assert!(v.command("select_all"));
    assert_eq!(v.filter.selected_text(), "ord");
    assert!(v.command("cut"));
    settle(&mut v);
    assert_eq!(names(&v).len(), 5, "⌘X: every table again");
    assert!(v.command("undo"));
    settle(&mut v);
    assert_eq!(names(&v), ["orders"], "⌘Z: back");
}

/// What a frame draws for each row and card is found once per layout: the link each column
/// starts (among those shown), whether the cards name their schemas.
#[test]
fn each_row_s_link_is_known_from_its_layout() {
    let v = opened(None);
    let (lines, orders) = (index(&v, "lines"), index(&v, "orders"));
    let g = v.graph().unwrap();
    let link = v.row_links.get(&lines).and_then(|c| c.get("order_id")).copied();
    assert_eq!(link.map(|l| g.links[l].to), Some(orders));
    assert!(v.row_links.get(&lines).is_some_and(|c| !c.contains_key("id")), "no link: none");
    assert!(!v.several_schemas, "one schema: the cards show their rows");
}
