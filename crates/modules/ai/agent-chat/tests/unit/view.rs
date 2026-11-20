use super::*;
use serde_json::json;

fn view() -> ChatView {
    ChatView {
        start: Some(ChatStart {
            agent: "none",
            session: Some("s-1".into()),
            folder: PathBuf::from("/p"),
            title: "t".into(),
        }),
        ..ChatView::default()
    }
}

/// A change in a tool's update is compared: as it was, as proposed, neither side written.
#[test]
fn a_change_is_compared_read_only() {
    let mut v = view();
    v.conversation
        .update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t", "title": "Edit",
        "content": [{"type": "diff", "path": "/p/a.php", "oldText": "a", "newText": "b"}]}));
    v.compare(0, 0);
    let Some(ViewerAction::Compare(input, title)) = v.action.take() else {
        panic!("no comparison")
    };
    assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("a", "b"));
    assert_eq!(input.read_only, [true, true]);
    assert!(title.starts_with("a.php"));
    v.compare(0, 5);
    assert!(v.action.is_none(), "no such change");
}

/// Nothing's sent before the agent runs; Esc stops nothing then.
#[test]
fn nothing_is_sent_before_the_agent_runs() {
    let mut v = view();
    v.input.insert("hello");
    assert!(!v.edit_key_with(ViewerKey::Enter, KeyReach::default()));
    assert_eq!(v.input.text(), "hello", "kept");
    assert!(!v.stop());
    assert!(v.wants_text());
}

/// The items laid out again only where they changed.
#[test]
fn only_what_changed_is_laid_out_again() {
    let mut v = view();
    let mut text = text_layout::TextSystem::new();
    let font = text_layout::Font::new(text_layout::FontFamily::SansSerif, 14.0);
    v.conversation.typed("first");
    v.conversation.update(
        &json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "a"}}),
    );
    v.lay_out(&mut text, &font, 500.0);
    let first = v.laid[0].clone();
    v.conversation.update(
        &json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "b"}}),
    );
    v.lay_out(&mut text, &font, 500.0);
    assert_eq!(v.laid[0], first, "the user's message kept");
    assert_eq!(v.laid[1].lines[0].text, "ab");
}

/// A tool updated far above the end is laid out again, the messages around it kept; another
/// width lays out everything, the same as laid out from nothing.
#[test]
fn a_tool_s_change_and_another_width_are_laid_out() {
    let mut v = view();
    let mut text = text_layout::TextSystem::new();
    let font = text_layout::Font::new(text_layout::FontFamily::SansSerif, 14.0);
    v.conversation.update(
        &json!({"sessionUpdate": "tool_call", "toolCallId": "t", "title": "Edit",
        "status": "in_progress", "content": [{"type": "diff", "path": "/p/a.php",
        "oldText": "a\nb", "newText": "a\nc\nd"}]}),
    );
    for chunk in ["one\n\n", "two ", "three"] {
        v.conversation.update(&json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": chunk}}));
        v.lay_out(&mut text, &font, 500.0);
    }
    let said = v.laid[1].clone();
    assert_eq!(v.laid[0].lines[0].icon, Some("play"));
    assert_eq!(v.laid[0].lines[1].text, "a.php — /p  +2 −1");
    v.conversation.update(
        &json!({"sessionUpdate": "tool_call_update", "toolCallId": "t", "status": "completed"}),
    );
    v.lay_out(&mut text, &font, 500.0);
    assert_eq!(v.laid[0].lines[0].icon, Some("check"), "the tool laid out again");
    assert_eq!(v.laid[1], said, "the message kept");
    let mut fresh = view();
    fresh.conversation = v.conversation.clone();
    v.lay_out(&mut text, &font, 40.0);
    fresh.lay_out(&mut text, &font, 40.0);
    assert_eq!(v.laid, fresh.laid, "another width: everything again");
    assert!(v.laid[1].lines.len() > said.lines.len(), "wrapped narrower");
}

/// A conversation taller than its tab, scrolled to its top: the scrollbar's thumb, pressed
/// and dragged, scrolls it (and it no longer follows the end).
#[test]
fn the_conversation_scrolls_by_its_thumb() {
    let mut v = view();
    // Already over (nothing started at the paint).
    v.phase = Phase::Failed("stopped".into());
    let said: String = (0..80).map(|i| format!("line {i}\n\n")).collect();
    v.conversation.update(&json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": said}}));
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    let (mut text, bounds) = (text_layout::TextSystem::new(), Rect::new(0.0, 0.0, 900.0, 500.0));
    let mut paint = |v: &mut ChatView| {
        let mut scene = render_api::Scene::default();
        v.paint(ViewerPaint {
            bounds,
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut v);
    assert!(v.max_scroll > 0.0, "taller than its tab");
    v.scroll(0.0, -v.max_scroll);
    paint(&mut v);
    let thumb = Point::new(bounds.max_x() - 4.0, bounds.min_y() + 6.0);
    assert_eq!(v.click(thumb), None);
    assert!(v.drag(Point::new(thumb.x, thumb.y + 40.0)));
    assert!(v.scroll > 0.0, "scrolled down");
    assert!(!v.following);
    assert!(v.release());
}

/// A resumed session's weight: 20 edits of 2,000 lines, a long message streaming. Timed:
/// one more chunk laid out, then another width (run with
/// `cargo test -p agent-chat --release -- --ignored measure_ --nocapture`).
#[test]
#[ignore = "a measurement, run by hand"]
fn measure_laying_out_a_long_session() {
    let mut v = view();
    let mut text = text_layout::TextSystem::new();
    let font = text_layout::Font::new(text_layout::FontFamily::SansSerif, 14.0);
    let file = |seed: usize| -> String {
        (0..2000).map(|l| format!("    $line{} = {};\n", l, (l * 7 + seed) % 1000)).collect()
    };
    for i in 0..20 {
        v.conversation.update(&json!({"sessionUpdate": "tool_call", "toolCallId": format!("t{i}"),
            "title": format!("Edit src/F{i}.php"), "kind": "edit", "status": "completed",
            "content": [{"type": "diff", "path": format!("/p/src/F{i}.php"),
                "oldText": file(i), "newText": file(i + 1)}]}));
    }
    let paragraph = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
        tempor incididunt ut labore et dolore magna aliqua. ";
    for p in 0..200 {
        let chunk = format!("{}{}\n\n", paragraph.repeat(3), p);
        v.conversation.update(&json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": chunk}}));
    }
    v.lay_out(&mut text, &font, 700.0);
    let timed = |v: &mut ChatView, text: &mut text_layout::TextSystem, width: f32, chunk: bool| {
        let runs = 10;
        let start = std::time::Instant::now();
        for r in 0..runs {
            if chunk {
                v.conversation.update(&json!({"sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "more "}}));
                v.lay_out(text, &font, width);
            } else {
                v.lay_out(text, &font, width + r as f32);
            }
        }
        start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs)
    };
    let chunk = timed(&mut v, &mut text, 700.0, true);
    let width = timed(&mut v, &mut text, 701.0, false);
    eprintln!("lay_out: one more chunk {chunk:.2} ms, another width {width:.2} ms");
}

/// A paste reaches the field on one line; what it has selected is copied, still after a
/// press beside it (it keeps the keys).
#[test]
fn a_paste_and_the_selection_are_the_field_s() {
    let mut view = view();
    assert!(view.paste("fix\nit"));
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("fix it"));
    let _ = view.click(Point::new(-1000.0, -1000.0));
    assert_eq!(view.selected_text().as_deref(), Some("fix it"), "the field kept the keys");
}
