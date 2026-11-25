//! The cards laid out at every width (long texts cut with "…", two actions, the count of
//! merged ones), the ring of the time left and none for a notification that stays, the
//! stack placed at each edge with the newest nearest it, what's under the mouse, the layout
//! kept until something changes.

use std::time::{Duration, Instant};

use render_api::Scene;
use workbench_model::{Notification, Notifications};

use super::*;

const SCALE: f32 = 2.0;

fn place(position: ToastPosition, width: f32) -> StackPlace {
    StackPlace {
        area: Rect::new(0.0, 100.0, width, 1200.0),
        position,
        scale: SCALE,
        backdrop: true,
    }
}

/// A stack with `notifications`, their transitions over.
fn stack(notifications: Vec<Notification>) -> Notifications {
    let mut n = Notifications::new();
    for notification in notifications {
        n.show(notification, Instant::now());
    }
    n.advance(Duration::from_millis(400), true);
    n
}

fn ends_cut(line: &ShapedLine, text: &mut TextSystem, font: &Font, cut: &str) -> bool {
    text.shape_line(cut, font).width == line.width && cut.ends_with('…')
}

#[test]
fn long_texts_are_cut_to_the_card_at_every_width() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let long = "A very long title that never fits on one line of a notification card at all, \
                nor on two of them, however wide the window is made for it to show";
    let detail = "Why it failed, at length: the server closed the connection while the rows were \
                  being written, after three attempts and a long wait, so nothing was kept";
    let n = stack(vec![Notification::error(long).with_detail(detail)]);
    for width in [300.0, 600.0, 1200.0, 3000.0] {
        let place = place(ToastPosition::TopRight, width);
        let card_width = ToastStack::card_width(&place);
        assert!(card_width <= WIDTH * SCALE && card_width <= width, "{width}: {card_width}");
        let mut stack = ToastStack::default();
        let card = stack.layout(&mut text, &palette, &n, &place)[0].clone();
        assert_eq!(card.title.len(), 2, "{width}: the title in two lines at most");
        for line in &card.title {
            assert!(line.width <= card.text_width + 0.5, "{width}: the title fits");
        }
        assert!(card.text_x + card.text_width < card.width);
        assert_eq!(card.detail.len(), 2, "{width}: two lines at most");
        for line in &card.detail {
            assert!(line.width <= card.text_width + 0.5, "{width}: a line fits");
        }
        // Cut with "…".
        let ctx = Ctx::new(&palette, SCALE);
        let (title_font, detail_font, ..) = fonts(&ctx);
        let lines = ds::wrap(&mut text, long, &title_font, card.text_width, 2);
        assert!(ends_cut(&card.title[1], &mut text, &title_font, &lines[1]), "{width}: {lines:?}");
        let details = ds::wrap(&mut text, detail, &detail_font, card.text_width, 2);
        assert!(ends_cut(&card.detail[1], &mut text, &detail_font, &details[1]), "{details:?}");
    }
}

#[test]
fn two_actions_sit_under_the_text_and_make_the_card_taller() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let place = place(ToastPosition::BottomRight, 1600.0);
    let plain = stack(vec![Notification::warning("Deleted 3 files")]);
    let acted = stack(vec![
        Notification::warning("Deleted 3 files")
            .with_action("Undo", CommandId("edit.undo"))
            .with_action("Show", CommandId("view.show"))
            .with_action("Third", CommandId("never")),
    ]);
    let mut stack = ToastStack::default();
    let short = stack.layout(&mut text, &palette, &plain, &place)[0].height;
    let card = stack.layout(&mut text, &palette, &acted, &place)[0].clone();
    assert_eq!(card.actions.len(), 2);
    assert!(card.height > short);
    let (_, first_x, first_w, first) = &card.actions[0];
    let (_, second_x, _, second) = &card.actions[1];
    assert!(second_x > &(first_x + first_w), "side by side, apart");
    assert_eq!((*first, *second), (CommandId("edit.undo"), CommandId("view.show")));
    // Drawn: a click on each reaches its command, on × closes, elsewhere is the card's.
    let mut scene = Scene::default();
    let icons = icons::Icons::builtin();
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    stack.paint(painter, &acted, place);
    let bounds = stack.bounds().unwrap();
    let id = acted.shown().next().unwrap().id();
    let actions_y = bounds.max_y() - (PAD_Y + ACTION_HEIGHT / 2.0) * SCALE;
    let x0 = bounds.min_x() + card.text_x;
    let at = |dx: f32| Point::new(x0 + dx, actions_y);
    assert_eq!(stack.hit(at(first_x + 4.0)), Some(ToastHit::Action(id, CommandId("edit.undo"))));
    assert_eq!(stack.hit(at(second_x + 4.0)), Some(ToastHit::Action(id, CommandId("view.show"))));
    let close = Point::new(
        bounds.max_x() - (10.0 + CLOSE / 2.0) * SCALE,
        bounds.min_y() + (10.0 + CLOSE / 2.0) * SCALE,
    );
    assert_eq!(stack.hit(close), Some(ToastHit::Close(id)));
    assert_eq!(
        stack.hit(Point::new(bounds.min_x() + 4.0, bounds.min_y() + 4.0)),
        Some(ToastHit::Card(id))
    );
    assert_eq!(stack.hit(Point::new(bounds.min_x() - 4.0, bounds.min_y())), None);
    assert!(stack.hover(close) && stack.over_button() && stack.hovered());
    assert!(!stack.hover(close), "the same: no redraw");
}

#[test]
fn the_ring_shows_the_time_left_and_none_stays_without_one() {
    assert_eq!(ring_steps(None), None);
    assert_eq!(ring_steps(Some(1.0)), Some(RING_STEPS));
    assert_eq!(ring_steps(Some(0.5)), Some(RING_STEPS / 2));
    assert_eq!(ring_steps(Some(0.0)), Some(0));
    let mut n = stack(vec![Notification::success("Saved."), Notification::error("Failed.")]);
    let ids: Vec<_> = n.shown().map(Toast::id).collect();
    n.freeze(true);
    n.set_elapsed(ids[0], Duration::from_secs(2));
    // The success counts down (half its 4 s left), the error stays: no ring.
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut stack = ToastStack::default();
    let paint = |n: &Notifications, text: &mut TextSystem, stack: &mut ToastStack| {
        let mut scene = Scene::default();
        let painter = Painter { text, scene: &mut scene, palette: &palette, icons: &icons };
        stack.paint(painter, n, place(ToastPosition::TopCenter, 1600.0));
        scene
            .layers()
            .iter()
            .map(|l| (l.quads.len(), l.images.iter().map(|i| (i.image.id, i.bounds)).collect()))
            .collect::<Vec<(usize, Vec<_>)>>()
    };
    let drawn = paint(&n, &mut text, &mut stack);
    assert_eq!(drawn.len(), 2, "a layer per card");
    // Newest first at the top: the error, then the success with its ring: one image, drawn
    // 1:1 on whole pixels, no quads for it.
    assert_eq!(drawn[0].0, drawn[1].0, "{drawn:?}");
    assert!(drawn[0].1.is_empty());
    let [(id, bounds)] = drawn[1].1[..] else { panic!("one ring: {drawn:?}") };
    let side = (CLOSE * SCALE).round();
    assert_eq!((bounds.size.width, bounds.size.height), (side, side));
    assert_eq!((bounds.min_x().fract(), bounds.min_y().fract()), (0.0, 0.0));
    assert_eq!(n.get(ids[1]).unwrap().remaining(), None);
    assert_eq!(stack.rings().count(), 1);
    assert!(stack.memory() > 0);
    // The next frame, nothing changed: the same image (nothing rasterized again).
    assert_eq!(paint(&n, &mut text, &mut stack)[1].1[0].0, id);
    // Less than a step later: still the same; a step later: another.
    // (A step: its 4 s over the ring's steps, ~11 ms.)
    n.set_elapsed(ids[0], Duration::from_millis(2003));
    assert_eq!(paint(&n, &mut text, &mut stack)[1].1[0].0, id);
    n.set_elapsed(ids[0], Duration::from_millis(2050));
    assert_ne!(paint(&n, &mut text, &mut stack)[1].1[0].0, id);
    // No toast left: the rings are let go.
    let empty = Notifications::new();
    paint(&empty, &mut text, &mut stack);
    assert_eq!(stack.rings().count(), 0);
    assert_eq!(stack.memory(), 0);
}

#[test]
fn the_newest_is_nearest_the_edge_chosen() {
    let mut text = TextSystem::new();
    let palette = Palette { editor_background: Rgba::WHITE, ..Palette::dark() };
    let icons = icons::Icons::builtin();
    let n = stack(vec![Notification::info("old"), Notification::info("new")]);
    let ids: Vec<_> = n.shown().map(Toast::id).collect();
    for position in ToastPosition::ALL {
        let place = place(position, 1600.0);
        let mut stack = ToastStack::default();
        let mut scene = Scene::default();
        let painter =
            Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
        stack.paint(painter, &n, place);
        let rect = |id| stack.placed.iter().find(|p| p.id == id).unwrap().card;
        let (old, new) = (rect(ids[0]), rect(ids[1]));
        if position.top() {
            assert_eq!(new.min_y(), place.area.min_y(), "{position:?}: the newest at the top");
            assert!(old.min_y() >= new.max_y() + GAP * SCALE - 1.0);
        } else {
            assert_eq!(new.max_y(), place.area.max_y(), "{position:?}: the newest at the bottom");
            assert!(old.max_y() <= new.min_y() - GAP * SCALE + 1.0);
        }
        if position.centered() {
            let mid = new.min_x() + new.size.width / 2.0;
            assert!((mid - place.area.size.width / 2.0).abs() <= 1.0, "{position:?}");
        } else {
            assert_eq!(new.max_x(), place.area.max_x() - MARGIN * SCALE, "{position:?}");
        }
    }
}

#[test]
fn the_layout_is_kept_until_the_stack_or_the_width_change() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let mut n = stack(vec![Notification::info("one")]);
    let mut stack = ToastStack::default();
    let wide = place(ToastPosition::TopCenter, 1600.0);
    stack.layout(&mut text, &palette, &n, &wide);
    let key = *stack.layout.key().unwrap();
    n.advance(Duration::from_millis(100), false);
    stack.layout(&mut text, &palette, &n, &wide);
    assert_eq!(*stack.layout.key().unwrap(), key, "time alone: the same layout");
    n.show(Notification::info("one"), Instant::now());
    let cards = stack.layout(&mut text, &palette, &n, &wide);
    assert!(cards[0].count.is_some(), "merged: ×2");
    let narrow = place(ToastPosition::TopCenter, 500.0);
    let cards = stack.layout(&mut text, &palette, &n, &narrow);
    assert!(cards[0].width < 500.0);
    // Nothing shown: nothing drawn, nothing placed.
    let empty = Notifications::new();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    stack.paint(painter, &empty, wide);
    assert!(scene.layers().is_empty() && stack.bounds().is_none());
}

#[test]
fn kinds_have_their_icon_and_a_color_that_reads() {
    let dark = Palette::dark();
    for kind in NotificationKind::ALL {
        assert!(icons::Icons::builtin().id(kind_icon(kind)).is_some(), "{kind:?}");
        let c = kind_color(kind, &dark);
        assert!(c.a > 0.9);
    }
    assert_ne!(
        kind_color(NotificationKind::Success, &dark),
        kind_color(NotificationKind::Error, &dark)
    );
}

/// Work in progress has a bar under its texts (how far, else a sweep); a notification
/// telling how far too; the others none. The card grows by it.
#[test]
fn work_in_progress_has_its_bar() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let place = place(ToastPosition::TopRight, 1200.0);
    let card = |n: Notification, text: &mut TextSystem| {
        let mut stack = ToastStack::default();
        stack.layout(text, &palette, &stack_of(n), &place)[0].clone()
    };
    fn stack_of(n: Notification) -> Notifications {
        stack(vec![n])
    }
    let plain = card(Notification::info("Saved"), &mut text);
    let looking = card(Notification::progress("Looking for updates…"), &mut text);
    let downloading = card(Notification::progress("Downloading").with_progress(0.4), &mut text);
    assert_eq!(plain.progress, None);
    assert_eq!(looking.progress, Some(None), "not known: it sweeps");
    assert_eq!(downloading.progress, Some(Some(0.4)));
    assert!(looking.height > plain.height, "room for the bar");
    let done = card(Notification::success("Copied").with_progress(1.0), &mut text);
    assert_eq!(done.progress, Some(Some(1.0)), "told how far, whatever its kind");
}

/// A notification is told to a screen reader as an alert: its title its name, its texts not
/// read again on their own.
#[test]
fn a_notification_is_told_as_an_alert() {
    use ui_kit::access::Role;
    let n = stack(vec![Notification::success("Saved.")]);
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = Scene::default();
    let recording = ui_kit::access::begin();
    let painter = Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
    ToastStack::default().paint(painter, &n, place(ToastPosition::TopCenter, 1600.0));
    let frame = recording.take();
    let alerts: Vec<_> = frame.nodes().iter().filter(|n| n.role == Role::Alert).collect();
    assert_eq!(alerts.len(), 1);
    assert_eq!(&*alerts[0].name, "Saved.");
    assert!(!frame.nodes().iter().any(|n| n.role == Role::Label && &*n.name == "Saved."));
}

/// Over something it can blur, a card is frosted glass (a blur under a translucent
/// surface); on the layer over the system's pages, with nothing under it, it's opaque and
/// blurs nothing (never a clear pane over a page's text).
#[test]
fn a_card_is_frosted_where_it_can_blur_else_opaque() {
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let shown = stack(vec![Notification::warning("Deleted 3 files")]);
    for backdrop in [true, false] {
        let mut stack = ToastStack::default();
        let mut scene = Scene::default();
        let painter =
            Painter { text: &mut text, scene: &mut scene, palette: &palette, icons: &icons };
        let place = StackPlace { backdrop, ..place(ToastPosition::BottomRight, 1600.0) };
        stack.paint(painter, &shown, place);
        let blurs = scene.layers().iter().map(|l| l.blurs.len()).sum::<usize>();
        assert_eq!(blurs > 0, backdrop, "blurred only where something is under it");
        let card = stack.bounds().unwrap();
        let surface = scene
            .layers()
            .iter()
            .flat_map(|l| l.quads.iter())
            .filter(|q| q.bounds == card)
            .map(|q| q.color.a)
            .fold(0.0_f32, f32::max);
        if backdrop {
            assert!(surface < 1.0, "translucent over its blur: {surface}");
        } else {
            assert!(surface >= 0.999, "opaque with nothing to blur: {surface}");
        }
    }
}
