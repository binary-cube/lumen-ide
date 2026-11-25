use super::*;
use crate::PROTOCOL;
use crate::framing::{Reader, write};

/// Every message there is, as the app and the host send them.
fn commands() -> Vec<Command> {
    let page = PageId(7);
    let size = Size { width: 1280, height: 800, scale: 2.0 };
    let modifiers = Modifiers(Modifiers::SHIFT | Modifiers::META);
    vec![
        Command::Hello(Hello {
            protocol: PROTOCOL,
            pid: 42,
            mach_service: Some("svc".into()),
            adapter: Some(Adapter { high: -1, low: 77 }),
        }),
        Command::Open(Open {
            page,
            source: Source::App { root: "/data/drawio".into(), entry: "index.html?embed=1".into() },
            size,
            hidden: true,
            background: [30, 31, 34, 255],
            pixels: None,
        }),
        Command::Open(Open {
            page: PageId(8),
            source: Source::Html("<p>ă</p>".into()),
            size,
            hidden: false,
            background: [255; 4],
            pixels: None,
        }),
        Command::Resize { page, size },
        Command::Hidden { page, hidden: true },
        Command::Close { page },
        Command::LoadHtml { page, html: "<b>x</b>".into() },
        Command::Post { page, message: "{\"a\":1}".into() },
        Command::BeginFrame { page },
        Command::FrameDone { page, frame: u64::MAX },
        Command::Mouse {
            page,
            mouse: Mouse {
                action: MouseAction::Down(Button::Right),
                x: 10.5,
                y: -3.0,
                clicks: 2,
                modifiers,
            },
        },
        Command::Wheel { page, wheel: Wheel { x: 1.0, y: 2.0, dx: 0.0, dy: -120.0, modifiers } },
        Command::Key {
            page,
            key: Key {
                action: KeyAction::Char,
                windows_code: 65,
                native_code: 0,
                character: 'ț' as u32,
                modifiers,
            },
        },
        Command::Focus { page, focused: true },
    ]
}

fn events() -> Vec<Event> {
    let page = PageId(7);
    vec![
        Event::Welcome(Welcome {
            protocol: PROTOCOL,
            pid: 9,
            engine: "156.0.8078.100-1".into(),
            adapter: None,
        }),
        Event::Frame(Frame {
            page,
            frame: 3,
            width: 2560,
            height: 1600,
            texture: Texture::Windows { handle: 0x1234 },
            popup: true,
            changed: vec![Changed { x: 1, y: 2, width: 30, height: 40 }],
        }),
        Event::Frame(Frame {
            page,
            frame: 4,
            width: 1,
            height: 1,
            texture: Texture::MachPort,
            popup: false,
            changed: Vec::new(),
        }),
        Event::Frame(Frame {
            page,
            frame: 5,
            width: 64,
            height: 64,
            texture: Texture::Dmabuf {
                format: 0x3432_5241,
                modifier: 0,
                planes: vec![Plane { offset: 0, stride: 256 }],
            },
            popup: false,
            changed: Vec::new(),
        }),
        Event::Frame(Frame {
            page,
            frame: 6,
            width: 200,
            height: 90,
            texture: Texture::Pixels { stride: 800 },
            popup: false,
            changed: Vec::new(),
        }),
        Event::Popup { page, rect: Some(PopupRect { x: 10, y: 20, width: 200, height: 90 }) },
        Event::Popup { page, rect: None },
        Event::Cursor { page, cursor: Cursor::Text },
        Event::Message { page, text: "hi".into() },
        Event::OpenLink { page, url: "https://example.com".into() },
        Event::Loaded { page },
        Event::Crashed { page },
    ]
}

/// Each message comes back as it went, all of them through one stream, its bytes arriving
/// in pieces of any size.
#[test]
fn every_message_crosses_a_stream_as_it_went() {
    let mut bytes = Vec::new();
    for command in commands() {
        write(&command, &mut bytes).unwrap();
    }
    for event in events() {
        write(&event, &mut bytes).unwrap();
    }
    for piece in [1, 3, 7, 64, bytes.len()] {
        let mut reader = Reader::default();
        let (mut got_commands, mut got_events) = (Vec::new(), Vec::new());
        let mut chunks = bytes.chunks(piece);
        loop {
            match if got_commands.len() < commands().len() {
                reader.message::<Command>().map(|m| m.map(|m| got_commands.push(m)))
            } else {
                reader.message::<Event>().map(|m| m.map(|m| got_events.push(m)))
            } {
                Ok(Some(())) => continue,
                Ok(None) => match chunks.next() {
                    Some(chunk) => reader.push(chunk),
                    None => break,
                },
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(got_commands, commands(), "pieces of {piece}");
        assert_eq!(got_events, events(), "pieces of {piece}");
    }
}

/// A GPU's LUID written as Chromium's switch reads it (decimal halves, the high one signed),
/// and read back.
#[test]
fn an_adapter_is_written_as_chromium_reads_it() {
    let adapter = Adapter { high: -2, low: 4_000_000_000 };
    assert_eq!(adapter.switch_value(), "-2,4000000000");
    assert_eq!(Adapter::parse(&adapter.switch_value()), Some(adapter));
    assert_eq!(Adapter::parse("0x1,2"), None);
    assert_eq!(Adapter::parse("12"), None);
}

/// A popup's pixels go beside its page's, named after them.
#[test]
fn a_popups_pixels_sit_beside_its_pages() {
    let page = std::path::Path::new("/run/u/frames-1.bgra");
    assert_eq!(popup_pixels(page), std::path::Path::new("/run/u/frames-1.popup.bgra"));
}

/// Changed rectangles are cut to their frame (one left with nothing dropped), and too many
/// are told as the whole frame.
#[test]
fn changed_rectangles_are_cut_to_their_frame() {
    let rect = |x, y, width, height| Changed { x, y, width, height };
    let told = [rect(0, 0, 10, 10), rect(95, 5, 10, 200), rect(100, 0, 5, 5), rect(0, 0, 0, 3)];
    assert_eq!(Changed::within(&told, (100, 50)), [rect(0, 0, 10, 10), rect(95, 5, 5, 45)]);
    let near_max = rect(u32::MAX, u32::MAX, u32::MAX, u32::MAX);
    assert!(Changed::within(&[near_max], (100, 50)).is_empty(), "outside: nothing");
    let many = vec![rect(0, 0, 1, 1); MAX_CHANGED + 1];
    assert!(Changed::within(&many, (100, 50)).is_empty(), "too many: the whole frame");
    let most = vec![rect(0, 0, 1, 1); MAX_CHANGED];
    assert_eq!(Changed::within(&most, (100, 50)).len(), MAX_CHANGED);
}
