//! The parameters' modal: a value's kind (following a number typed until one is chosen),
//! what can't be written in, the keys (↵ gives, Esc and × give up, Tab moves), values only
//! for the question they were asked for, its height.

use super::*;
use database::console::AskedParam;
use render_api::Scene;

fn asked(label: &str, name: ParamName) -> AskedParam {
    AskedParam { name, label: label.into(), uses: 1 }
}

fn request() -> ParamsRequest {
    let mut r = ParamsRequest::new(Purpose::Run, "console", "shop");
    r.statements = vec!["SELECT * FROM t WHERE a = :a AND b = :b".into()];
    r.places =
        vec![(0, 26..28, ParamName::Named("a".into())), (0, 37..39, ParamName::Named("b".into()))];
    let kept = Remembered {
        name: ParamName::Named("b".into()),
        kind: ValueKind::Raw,
        text: "now()".into(),
    };
    r.entries = vec![
        ParamEntry::new(&asked(":a", ParamName::Named("a".into())), None),
        ParamEntry::new(&asked(":b", ParamName::Named("b".into())), Some(&kept)),
    ];
    r
}

#[test]
fn a_value_s_kind_follows_what_s_typed_until_chosen() {
    let mut e = ParamEntry::new(&asked("?", ParamName::Positional(1)), None);
    assert_eq!(e.effective_kind(), ValueKind::Text);
    e.text = "42".into();
    assert_eq!(e.value(), Some(ParamValue::Literal(Value::Int(42))));
    e.text = "2.5".into();
    assert_eq!(e.value(), Some(ParamValue::Literal(Value::Float(2.5))));
    e.kind = ValueKind::Text;
    e.kind_chosen = true;
    assert_eq!(e.value(), Some(ParamValue::Literal(Value::Text("2.5".into()))));
    e.kind = ValueKind::Number;
    e.text = "twelve".into();
    assert_eq!(e.problem(), Some(Problem::NotANumber));
    assert_eq!(values(std::slice::from_ref(&e)), None);
    e.kind = ValueKind::Null;
    assert_eq!(e.value(), Some(ParamValue::Literal(Value::Null)));
    e.kind = ValueKind::Raw;
    e.text = "  ".into();
    assert_eq!(e.problem(), Some(Problem::EmptyExpression));
    e.text = " now() ".into();
    assert_eq!(e.value(), Some(ParamValue::Raw("now()".into())));
}

#[test]
fn the_modal_gives_its_values_once_they_can_be_written() {
    let mut view = ParamsView::default();
    view.set_data(&request());
    // The first one left empty is focused; the kept one shows its value and kind.
    assert_eq!(view.focus(), Some(0));
    assert_eq!(view.entries()[1].text, "now()");
    assert!(view.text_input("x\u{7}1"));
    assert_eq!(view.entries()[0].text, "x1", "control characters dropped");
    view.set_kind(0, ValueKind::Number);
    // A number that isn't one keeps it open, the row focused.
    assert!(!view.give());
    assert_eq!(view.problem().map(|(i, _)| i), Some(0));
    view.command("value:0:7");
    view.edit_key(ViewerKey::Tab);
    assert_eq!(view.focus(), Some(1));
    view.edit_key(ViewerKey::Tab);
    assert_eq!(view.focus(), Some(0), "around");
    assert!(view.command("remember:1"));
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(view.closed());
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
    let entries = view.entries();
    assert_eq!(entries[0].value(), Some(ParamValue::Literal(Value::Int(7))));
    assert!(!entries[1].remember);
}

#[test]
fn esc_gives_up_and_nothing_is_done() {
    let mut view = ParamsView::default();
    view.set_data(&request());
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
}

#[test]
fn the_modal_is_as_tall_as_its_rows_and_draws() {
    let one = request();
    let mut many = request();
    for i in 0..10 {
        many.entries.push(ParamEntry::new(
            &asked(&format!(":p{i}"), ParamName::Named(format!("p{i}"))),
            None,
        ));
    }
    assert!(content_height(&many) > content_height(&one));
    // Past a few rows they scroll: never taller.
    let mut more = many.clone();
    more.entries.push(more.entries[0].clone());
    assert_eq!(content_height(&many), content_height(&more));
    let mut view = ParamsView::default();
    view.set_data(&many);
    let mut text = text_layout::TextSystem::new();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut frame = |view: &mut ParamsView| {
        let mut scene = Scene::default();
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1700.0, content_height(&many) * 2.0),
            scale: 2.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    frame(&mut view);
    // Its last row brought into view when focused.
    view.edit_key(ViewerKey::Up);
    assert_eq!(view.focus(), Some(many.entries.len() - 1));
    frame(&mut view);
    assert!(view.scroll(0.0, -1e9));
}
