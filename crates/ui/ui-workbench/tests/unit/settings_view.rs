use super::*;
use workbench_model::{SettingsGroup, SettingsPage};

fn model() -> SettingsModel {
    let preview = ThemePreview::from_palette(&Palette::dark());
    SettingsModel {
        themes: vec![
            ThemeOption { id: "a".into(), name: "A".into(), dark: true, preview },
            ThemeOption { id: "b".into(), name: "B".into(), dark: false, preview },
        ],
        current_theme: "a".into(),
        follow_system: true,
        languages: vec![("ro".into(), "Română".into()), ("en".into(), "English".into())],
        current_language: "ro".into(),
        file_icon_set: None,
        icon_samples: Vec::new(),
        file_icon_sets: Vec::new(),
        icon_look: icons::IconLook::default(),
        icon_looks: icons::IconStyle::ALL
            .into_iter()
            .map(|style| IconChoice {
                look: icons::IconLook::Style(style),
                name: style.id().to_owned(),
                detail: String::new(),
            })
            .collect(),
        keymaps: vec![("default".into(), "Default".into()), ("vscode".into(), "VS Code".into())],
        current_keymap: "default".into(),
        bindings: std::sync::Arc::new(vec![crate::BindingRow {
            command: "file.save".into(),
            label: "Save".into(),
            shortcuts: vec!["CmdOrCtrl+S".into()],
            shown: vec!["⌘S".into()],
            ..crate::BindingRow::default()
        }]),
        modules: vec![ModuleRow {
            id: "lang.php".into(),
            name: "PHP".into(),
            description: "Limbajul PHP".into(),
            enabled: true,
            active: true,
            ..Default::default()
        }],
        modules_at_startup: false,
        project: Some("shop".into()),
        frameworks: vec![FrameworkRow {
            id: "laravel".into(),
            name: "Laravel".into(),
            on: true,
            detected: Some("composer.json".into()),
            ..Default::default()
        }],
        blocks: vec![history_block(), remote_block(), semantic_block()],
        code_languages: Vec::new(),
        code_language: None,
    }
}

/// A row of number choices.
fn choices(key: &'static str, values: &[i64], current: i64) -> workbench_model::SettingsRow {
    workbench_model::SettingsRow::Choices {
        key,
        label: key.to_owned(),
        detail: String::new(),
        choices: values.iter().map(|&v| (v, v.to_string())).collect(),
        current,
        default: None,
    }
}

/// The local history's block, on the Files page.
fn history_block() -> ContributedBlock {
    ContributedBlock {
        id: "history",
        page: "files",
        page_info: Some(SettingsPage::FILES),
        order: 30,
        advanced: false,
        language: None,
        keywords: "history".into(),
        block: workbench_model::SettingsBlock {
            title: "History".into(),
            detail: "Versions".into(),
            rows: vec![
                choices("days", &[7, 30, 90, 365], 30),
                choices("versions", &[50, 200, 500, 1000], 200),
                workbench_model::SettingsRow::Status {
                    text: "2 KB".into(),
                    button: Some(("clear", "Delete".into())),
                },
            ],
        },
    }
}

/// The remote transfers' page of its own, among the tools.
const REMOTE: SettingsPage = SettingsPage {
    id: "remote",
    label: "settings.remote_section",
    icon: "cloud",
    group: SettingsGroup::Tools,
    order: 40,
};

/// The remote transfers' block, on a page of its own.
fn remote_block() -> ContributedBlock {
    ContributedBlock {
        id: "remote",
        page: "remote",
        page_info: Some(REMOTE),
        order: 100,
        advanced: false,
        language: None,
        keywords: "remote ftp".into(),
        block: workbench_model::SettingsBlock {
            title: "Remote".into(),
            detail: "Transfers".into(),
            rows: vec![
                choices("uploads", &[1, 2, 4], 2),
                choices("downloads", &[2, 4, 8], 4),
                choices("retries", &[2, 4], 4),
            ],
        },
    }
}

/// Semantic search's block, on the Search page: a switch card and its notes.
fn semantic_block() -> ContributedBlock {
    ContributedBlock {
        id: "semantic",
        page: "search",
        page_info: Some(SettingsPage {
            id: "search",
            label: "settings.search",
            icon: "search",
            group: SettingsGroup::Tools,
            order: 50,
        }),
        order: 100,
        advanced: false,
        language: None,
        keywords: "semantic meaning".into(),
        block: workbench_model::SettingsBlock {
            title: "Semantic search".into(),
            detail: "By meaning".into(),
            rows: vec![
                workbench_model::SettingsRow::SwitchCard {
                    key: "on",
                    label: "Use it".into(),
                    detail: String::new(),
                    on: true,
                    status: "Active".into(),
                    tone: workbench_model::StatusTone::Accent,
                },
                workbench_model::SettingsRow::Notes(vec!["Local".into(), "Lexical".into()]),
            ],
        },
    }
}

/// The Search page holds the modules' blocks: a click on the switch card turns it off
/// (a `Choose`), a search leading there highlights the card; without them, no page.
#[test]
fn the_search_section_shows_its_switch_card() {
    let model = model();
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 2400.0), 1.0);
    view.open();
    view.set_sections(&model);
    view.section = Some("search");
    let mut scene = Scene::default();
    let mut text = TextSystem::new();
    view.paint(&model, &Icons::builtin(), &mut text, &mut scene, &Palette::dark());
    let off = SettingsAction::Choose { block: "semantic", key: "on", value: 0 };
    let hit = view.block_hits.iter().find(|(_, a)| *a == off).map(|(r, _)| *r).expect("switch");
    let card =
        view.anchors.iter().find(|(a, _)| *a == "semantic").map(|(_, r)| *r).expect("anchor");
    assert_eq!(view.hit(Point::new(hit.min_x() + 3.0, hit.min_y() + 3.0)), Some(off));
    assert_eq!(card.min_y(), hit.min_y(), "the card is what a search highlights");
    assert!(view.blocks_bottom > card.max_y(), "the notes under the card");
    let mut without = model.clone();
    without.blocks.retain(|b| b.id != "semantic");
    assert!(!pages(&without.blocks).iter().any(|p| p.page.id == "search"));
}

#[test]
fn clicks_select_themes_toggle_and_close() {
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    assert_eq!(view.hit(Point::new(5.0, 5.0)), None, "closed: it doesn't react");
    view.open();
    view.paint(
        &model(),
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let (second, _) = view.cards[1].clone();
    assert!(view.languages.is_empty(), "the Appearance section is active");
    let center = Point::new(second.min_x() + 10.0, second.min_y() + 10.0);
    assert_eq!(view.hit(center), Some(SettingsAction::SelectTheme("b".into())));
    let t = view.toggle;
    assert_eq!(
        view.hit(Point::new(t.min_x() + 5.0, t.min_y() + 5.0)),
        Some(SettingsAction::ToggleFollowSystem)
    );
    assert_eq!(view.hit(Point::new(5.0, 5.0)), None, "a click outside doesn't close it");
    assert!(view.step_section(6), "↓↓↓↓↓↓: the „Module” section");
    view.paint(
        &model(),
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let (row, _) = view.framework_rows[0].clone();
    assert_eq!(
        view.hit(Point::new(row.min_x() + 4.0, row.min_y() + 4.0)),
        Some(SettingsAction::ToggleFramework("laravel".into()))
    );
    assert!(view.step_section(1), "↓: the Language section");
    view.paint(
        &model(),
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let (english, _) = view.languages[1].clone();
    assert_eq!(
        view.hit(Point::new(english.min_x() + 4.0, english.min_y() + 4.0)),
        Some(SettingsAction::SelectLanguage("en".into()))
    );
}

/// A module's block: a choice is what a click on it asks for (its block, row and value),
/// on the module's own page.
#[test]
fn a_module_block_s_choices_are_clicked() {
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 2400.0), 1.0);
    view.open();
    let paint = |view: &mut SettingsView| {
        view.paint(
            &model(),
            &Icons::builtin(),
            &mut TextSystem::new(),
            &mut Scene::default(),
            &Palette::dark(),
        );
    };
    let wanted = [
        SettingsAction::Choose { block: "remote", key: "uploads", value: 1 },
        SettingsAction::Choose { block: "remote", key: "downloads", value: 8 },
        SettingsAction::Choose { block: "remote", key: "retries", value: 2 },
    ];
    paint(&mut view);
    let mut found = false;
    for _ in 0..7 {
        if view.block_hits.iter().any(|(_, a)| *a == wanted[0]) {
            found = true;
            break;
        }
        view.step_section(1);
        paint(&mut view);
    }
    assert!(found, "a page shows the remote transfers");
    // The last block: scrolled down to it.
    view.scroll_by(10_000.0);
    paint(&mut view);
    for action in wanted {
        let r = view.block_hits.iter().find(|(_, a)| *a == action).map(|(r, _)| *r).unwrap();
        assert_eq!(view.hit(Point::new(r.min_x() + 3.0, r.min_y() + 3.0)), Some(action));
    }
}

/// The pages join the navigation in their groups and places; the history's block is on the
/// Files page, its button a `Press`; a search finds a block by its words.
#[test]
fn module_pages_and_blocks_take_their_places() {
    let model = model();
    let ids: Vec<_> = pages(&model.blocks).iter().map(|p| p.page.id).collect();
    assert_eq!(
        ids,
        ["appearance", "icons", "files", "remote", "search", "keymap", "modules", "language"]
    );
    let mut without = model.clone();
    without.blocks.retain(|b| b.id != "remote");
    assert_eq!(pages(&without.blocks).len(), 7, "a module turned off: no page");

    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 2400.0), 1.0);
    view.open();
    let paint = |view: &mut SettingsView| {
        let mut scene = Scene::default();
        view.paint(&model, &Icons::builtin(), &mut TextSystem::new(), &mut scene, &Palette::dark());
    };
    paint(&mut view);
    assert!(view.step_section(2), "↓↓: the Files page");
    paint(&mut view);
    // Its last block: scrolled down to it.
    view.scroll_by(10_000.0);
    paint(&mut view);
    let press = SettingsAction::Press { block: "history", action: "clear" };
    let r = view.block_hits.iter().find(|(_, a)| *a == press).map(|(r, _)| *r);
    let r = r.expect("the history's button, on the Files page");
    assert_eq!(view.hit(Point::new(r.min_x() + 3.0, r.min_y() + 3.0)), Some(press));
    let entries = SettingsView::entries(&model);
    assert!(entries.iter().any(|e| e.anchor == "remote" && e.section == "remote"));
}

/// The wheel over the side navigation is the navigation's: at its end it stops there and
/// never moves the content instead.
#[test]
fn the_wheel_over_the_navigation_never_scrolls_the_content() {
    let model = model();
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 440.0), 1.0);
    view.open();
    let paint = |view: &mut SettingsView| {
        let mut scene = Scene::default();
        view.paint(&model, &Icons::builtin(), &mut TextSystem::new(), &mut scene, &Palette::dark());
    };
    paint(&mut view);
    let nav = view.modal.nav();
    assert!(nav.size.width > 0.0, "the side navigation");
    view.hover(Point::new(nav.min_x() + 40.0, nav.min_y() + 20.0));
    for _ in 0..3 {
        view.scroll_by(10_000.0);
        paint(&mut view);
    }
    assert_eq!(view.scroll, 0.0, "the content didn't move");
    // Elsewhere the wheel is the content's.
    view.hover(Point::new(nav.max_x() + 200.0, nav.min_y() + 20.0));
    view.scroll_by(10_000.0);
    assert!(view.scroll > 0.0);
}

/// A slider row: a click picks the nearest stop, a drag goes on to the next ones, a
/// release lets it go.
#[test]
fn a_slider_row_is_clicked_and_dragged() {
    let mut model = model();
    model.blocks = vec![ContributedBlock {
        id: "remote",
        page: "remote",
        page_info: Some(REMOTE),
        order: 100,
        advanced: false,
        language: None,
        keywords: "".into(),
        block: workbench_model::SettingsBlock {
            title: "Remote".into(),
            detail: String::new(),
            rows: vec![workbench_model::SettingsRow::Slider {
                key: "uploads",
                label: "Uploads".into(),
                detail: String::new(),
                default: None,
                stops: [1, 2, 4, 8].iter().map(|&v| (v, v.to_string())).collect(),
                current: 2,
            }],
        },
    }];
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    view.open();
    view.set_sections(&model);
    assert!(view.step_section(2), "the Remote page");
    view.paint(
        &model,
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let (area, ..) = view.sliders[0].clone();
    let y = area.rect.min_y() + 10.0;
    let at = |i: usize| Point::new(area.stops[i], y);
    let choose = |value| Some(SettingsAction::Choose { block: "remote", key: "uploads", value });
    assert_eq!(view.hit(at(2)), choose(4));
    assert!(view.slider_held());
    assert_eq!(view.drag(Point::new(area.stops[2] + 2.0, y)), None, "the same stop: nothing");
    assert_eq!(view.drag(at(3)), choose(8));
    assert_eq!(view.drag(Point::new(-500.0, y)), choose(1), "past the start: the first");
    assert!(view.release());
    assert!(!view.slider_held());
}

/// A module's text field: a click starts writing it (the keys go there, not to the
/// search), Enter saves what was written, Esc gives it up, a click elsewhere saves it.
#[test]
fn a_block_s_text_field_is_written_then_saved_or_given_up() {
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    view.open();
    view.paint(
        &model(),
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    // Clicks count only in the content shown (not under the header).
    let content = view.modal.content();
    let on = Point::new(content.min_x() + 20.0, content.min_y() + 20.0);
    let elsewhere = Point::new(content.min_x() + 20.0, content.min_y() + 200.0);
    let field = Rect::new(content.min_x() + 10.0, content.min_y() + 10.0, 200.0, 30.0);
    view.text_fields.push((field, "terminal-shell", "custom_shell", "/bin/old".into()));
    assert_eq!(view.hit(on), None, "writing starts");
    assert!(view.editing_text());
    view.type_text("/opt/new");
    view.type_text(" -l");
    assert!(!view.has_query(), "the keys went to the field");
    assert_eq!(
        view.commit_text(),
        Some(SettingsAction::SetText {
            block: "terminal-shell",
            key: "custom_shell",
            value: "/opt/new -l".into(),
        }),
        "the old value was selected: replaced"
    );
    assert!(!view.editing_text());
    // Esc: given up.
    view.hit(on);
    view.type_text("x");
    assert!(view.cancel_text());
    assert_eq!(view.commit_text(), None);
    // A click elsewhere: saved (that click does nothing else).
    view.hit(on);
    view.backspace(false);
    let saved = view.hit(elsewhere);
    assert_eq!(
        saved,
        Some(SettingsAction::SetText {
            block: "terminal-shell",
            key: "custom_shell",
            value: String::new()
        })
    );
}

/// Rows scrolled under the header keep their rects but are hidden there: a click on esc
/// closes the window instead of reaching them.
#[test]
fn esc_closes_even_over_rows_scrolled_under_the_header() {
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    view.open();
    view.paint(
        &model(),
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    let esc = (0..800)
        .flat_map(|y| (0..1200).map(move |x| Point::new(x as f32, y as f32)))
        .find(|&p| view.modal.hit(p) == Some(ModalHit::Close))
        .expect("the esc button");
    let everywhere = Rect::new(0.0, 0.0, 1200.0, 800.0);
    view.text_fields.push((everywhere, "terminal-shell", "custom_shell", String::new()));
    view.result_rects.push(everywhere);
    assert_eq!(view.hit(esc), Some(SettingsAction::Close));
    assert!(!view.editing_text());
}

/// Closed, settings let go of the semantic model (and the entries' vectors): it's freed when
/// nothing else uses it, given again at the next opening.
#[test]
fn closing_lets_go_of_the_semantic_model() {
    struct Model;
    impl text_search::Embedder for Model {
        fn embed(&self, _: &str) -> Vec<f32> {
            vec![1.0]
        }
        fn dims(&self) -> usize {
            1
        }
    }
    let model: std::sync::Arc<dyn text_search::Embedder> = std::sync::Arc::new(Model);
    let mut view = SettingsView::default();
    view.open();
    view.set_embedder(Some(std::sync::Arc::clone(&model)));
    assert_eq!(std::sync::Arc::strong_count(&model), 2);
    view.close();
    assert_eq!(std::sync::Arc::strong_count(&model), 1);
    assert!(view.vectors.is_empty() && view.vectors.capacity() == 0);
}

/// With the semantic model, the entries' vectors are made in the background: the search
/// finds by words meanwhile (frames keep coming), then by meaning too once they're here.
#[test]
fn the_entries_vectors_are_made_in_the_background() {
    // A "model" for which every text means the same.
    struct Same;
    impl text_search::Embedder for Same {
        fn embed(&self, _: &str) -> Vec<f32> {
            vec![1.0]
        }
        fn dims(&self) -> usize {
            1
        }
    }
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    view.open();
    view.set_embedder(Some(std::sync::Arc::new(Same)));
    view.type_text("zzqx");
    let paint = |view: &mut SettingsView| {
        view.paint(
            &model(),
            &Icons::builtin(),
            &mut TextSystem::new(),
            &mut Scene::default(),
            &Palette::dark(),
        );
    };
    paint(&mut view);
    assert!(view.results.is_empty(), "no word matches, no vectors yet");
    assert!(view.is_animating(), "frames until the vectors come");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while view.results.is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        paint(&mut view);
    }
    assert!(!view.results.is_empty(), "found by meaning");
    assert!(view.making.is_none(), "nothing more on its way");
}

/// The navigation and the blocks' order come from the pages and places the blocks name,
/// never from the order the modules registered in: shuffled, the same.
#[test]
fn pages_and_blocks_do_not_depend_on_the_modules_order() {
    let block =
        |id: &'static str, page: SettingsPage, order: u16, advanced: bool| ContributedBlock {
            id,
            page: page.id,
            page_info: Some(page),
            order,
            advanced,
            language: None,
            keywords: String::new(),
            block: workbench_model::SettingsBlock {
                title: id.into(),
                detail: String::new(),
                rows: vec![choices("k", &[1, 2], 1)],
            },
        };
    let blocks = vec![
        block("quality", SettingsPage::LANGUAGES, 20, false),
        block("language-servers", SettingsPage::LANGUAGES, 10, false),
        block("sql_format", SettingsPage::LANGUAGES, 30, false),
        block("language-servers-add", SettingsPage::LANGUAGE_SERVERS, 30, true),
        block("language-servers-parts", SettingsPage::LANGUAGE_SERVERS, 10, false),
        block("database-console-profile", SettingsPage::DATABASE, 40, true),
        block("database", SettingsPage::DATABASE, 10, false),
        block("history", SettingsPage::FILES, 30, false),
        block("caret", SettingsPage::EDITOR, 10, false),
        remote_block(),
        semantic_block(),
    ];
    let mut sorted = blocks.clone();
    order_blocks(&mut sorted);
    let ids = |blocks: &[ContributedBlock]| blocks.iter().map(|b| b.id).collect::<Vec<_>>();
    let nav =
        |blocks: &[ContributedBlock]| pages(blocks).iter().map(|p| p.page.id).collect::<Vec<_>>();
    let expected_nav = nav(&sorted);
    assert_eq!(
        expected_nav,
        [
            "appearance",
            "icons",
            "editor",
            "files",
            "languages",
            "language-servers",
            "database",
            "remote",
            "search",
            "keymap",
            "modules",
            "language"
        ]
    );
    let on =
        |page: &str| sorted.iter().filter(|b| b.page == page).map(|b| b.id).collect::<Vec<_>>();
    assert_eq!(on("languages"), ["language-servers", "quality", "sql_format"]);
    assert_eq!(on("language-servers"), ["language-servers-parts", "language-servers-add"]);
    assert_eq!(on("database"), ["database", "database-console-profile"], "Advanced last");
    // A few shuffles (a fixed, cheap sequence: the same every run).
    let mut shuffled = blocks;
    let mut seed = 0x2545_f491_u32;
    for _ in 0..12 {
        for i in (1..shuffled.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            shuffled.swap(i, seed as usize % (i + 1));
        }
        assert_eq!(nav(&shuffled), expected_nav, "the navigation");
        let mut ordered = shuffled.clone();
        order_blocks(&mut ordered);
        assert_eq!(ids(&ordered), ids(&sorted), "the blocks");
    }
}

/// The navigation's groups: a caption over each group's pages, in the groups' order.
#[test]
fn the_navigation_groups_its_pages() {
    let model = model();
    let pages = pages(&model.blocks);
    let groups = group_labels();
    let sections = modal_sections(&pages, &groups);
    let captions: Vec<&str> = sections.iter().map(|s| s.group).collect();
    let interface = &groups[SettingsGroup::Interface as usize];
    let tools = &groups[SettingsGroup::Tools as usize];
    let application = &groups[SettingsGroup::Application as usize];
    assert_eq!(captions[..2], [interface.as_str(), interface.as_str()]);
    assert_eq!(captions[3..5], [tools.as_str(), tools.as_str()]);
    assert_eq!(captions[5..], [application.as_str(); 3]);
    // Painted wide: every page has its row; narrow: tabs, still every page reachable.
    for width in [1400.0, 700.0] {
        let mut view = SettingsView::default();
        view.set_viewport(Rect::new(0.0, 0.0, width, 900.0), 1.0);
        view.open();
        view.paint(
            &model,
            &Icons::builtin(),
            &mut TextSystem::new(),
            &mut Scene::default(),
            &Palette::dark(),
        );
        for page in &pages {
            let found = (0..900)
                .step_by(4)
                .flat_map(|y| (0..width as i32).step_by(4).map(move |x| (x, y)))
                .any(|(x, y)| {
                    view.modal.hit(Point::new(x as f32, y as f32))
                        == Some(ModalHit::Section(page.page.id))
                });
            // Narrow, only the chosen group's pages have tabs (the others' group tab).
            if width > 1000.0 || page.page.group == SettingsGroup::Interface {
                assert!(found, "{} reachable at {width}", page.page.id);
            }
        }
    }
}

/// A page with a block, an "Advanced" one and SQL's own (languages page).
fn languages_model() -> SettingsModel {
    let block = |id: &'static str, page: SettingsPage, advanced, language: Option<&str>, rows| {
        ContributedBlock {
            id,
            page: page.id,
            page_info: Some(page),
            order: 10,
            advanced,
            language: language.map(str::to_owned),
            keywords: String::new(),
            block: workbench_model::SettingsBlock { title: id.into(), detail: String::new(), rows },
        }
    };
    let toggle = |key: &'static str, label: &str, on: bool| workbench_model::SettingsRow::Toggle {
        key,
        label: label.into(),
        detail: format!("{label} detail"),
        on,
        default: Some(true),
    };
    let mut model = model();
    model.blocks = vec![
        block(
            "engine",
            SettingsPage::LANGUAGES,
            false,
            None,
            vec![toggle("on", "Engine on", true)],
        ),
        block(
            "sql",
            SettingsPage::LANGUAGES,
            false,
            Some("sql"),
            vec![toggle("upper", "Keywords upper", false)],
        ),
        block(
            "parts",
            SettingsPage::LANGUAGE_SERVERS,
            false,
            None,
            vec![toggle("all", "Download all", true)],
        ),
        block(
            "limits",
            SettingsPage::LANGUAGE_SERVERS,
            true,
            None,
            vec![toggle("trace", "Protocol traffic", false)],
        ),
    ];
    model.code_languages = vec![("php".into(), "PHP".into()), ("sql".into(), "SQL".into())];
    model.code_language = Some("php".into());
    model
}

fn paint(view: &mut SettingsView, model: &SettingsModel) {
    view.paint(
        model,
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
}

/// "↺" shows only while a row differs from its default, and puts the default back.
#[test]
fn reset_puts_the_default_back() {
    let model = languages_model();
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1400.0, 900.0), 1.0);
    view.open();
    view.set_sections(&model);
    view.show_page("languages");
    paint(&mut view, &model);
    let reset = |view: &SettingsView| {
        view.block_hits.iter().find(|(_, a)| matches!(a, SettingsAction::Reset { .. })).cloned()
    };
    // PHP chosen: SQL's block isn't shown; the engine's row is at its default: no "↺".
    assert!(!view.row_anchors.iter().any(|(b, ..)| *b == "sql"));
    assert!(reset(&view).is_none());
    let mut changed = model.clone();
    changed.blocks[0].block.rows[0] = workbench_model::SettingsRow::Toggle {
        key: "on",
        label: "Engine on".into(),
        detail: String::new(),
        on: false,
        default: Some(true),
    };
    paint(&mut view, &changed);
    let (r, action) = reset(&view).expect("↺ while it differs");
    assert_eq!(action, SettingsAction::Reset { block: "engine", key: "on", value: 1 });
    assert_eq!(
        view.hit(Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)),
        Some(action)
    );
}

/// The Languages page's chooser: a click on a language asks for it; a language's own block
/// shows only while it's chosen.
#[test]
fn the_languages_chooser_picks_the_page_language() {
    let mut model = languages_model();
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1400.0, 900.0), 1.0);
    view.open();
    view.set_sections(&model);
    view.show_page("languages");
    paint(&mut view, &model);
    let pick = SettingsAction::SelectCodeLanguage("sql".into());
    let (r, _) = view.block_hits.iter().find(|(_, a)| *a == pick).cloned().expect("SQL's pill");
    assert_eq!(view.hit(Point::new(r.min_x() + 4.0, r.min_y() + 4.0)), Some(pick));
    model.code_language = Some("sql".into());
    paint(&mut view, &model);
    assert!(view.row_anchors.iter().any(|(b, k, _)| *b == "sql" && *k == "upper"));
}

/// The search finds rows, those under a folded "Advanced" too: opening one goes to its
/// page, opens the fold and lights the row; one of another language asks for it.
#[test]
fn the_search_finds_rows_in_folds_and_other_languages() {
    let model = languages_model();
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1400.0, 900.0), 1.0);
    view.open();
    view.set_sections(&model);
    view.show_page("language-servers");
    paint(&mut view, &model);
    assert!(!view.row_anchors.iter().any(|(b, ..)| *b == "limits"), "folded");
    view.type_text("protocol traffic");
    paint(&mut view, &model);
    assert!(!view.results.is_empty());
    assert!(view.confirm(&model));
    assert_eq!(view.active(), "language-servers");
    paint(&mut view, &model);
    let lit = view.flash_rect().expect("the row lit");
    let row = view.row_anchors.iter().find(|(b, k, _)| *b == "limits" && *k == "trace");
    assert_eq!(row.map(|r| r.2), Some(lit), "unfolded, the row itself");
    assert_eq!(view.take_wanted_language(), None);
    // SQL's row, while PHP is chosen: found, and SQL asked for.
    view.type_text("keywords upper");
    paint(&mut view, &model);
    assert!(view.confirm(&model));
    assert_eq!(view.active(), "languages");
    assert_eq!(view.take_wanted_language().as_deref(), Some("sql"));
    // A click on the fold's header closes it again.
    paint(&mut view, &model);
    view.show_page("language-servers");
    paint(&mut view, &model);
    let (header, _) = view
        .block_hits
        .iter()
        .find(|(_, a)| *a == SettingsAction::ToggleAdvanced("language-servers"))
        .cloned()
        .unwrap();
    assert_eq!(view.hit(Point::new(header.min_x() + 4.0, header.min_y() + 4.0)), None);
    paint(&mut view, &model);
    assert!(!view.row_anchors.iter().any(|(b, ..)| *b == "limits"), "folded again");
}

/// The footer says the keys that work now (as the list modals do): the pages, typing to
/// search and Esc; while searching, the results; while a field is written, save / cancel.
#[test]
fn the_footer_shows_the_keys_that_work_now() {
    let keys = |view: &SettingsView| -> Vec<Vec<&'static str>> {
        view.key_hints().into_iter().map(|(k, _)| k.to_vec()).collect()
    };
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    view.open();
    assert_eq!(keys(&view), [vec!["↑", "↓"], vec!["A–Z"], vec!["esc"]]);
    view.type_text("tema");
    assert_eq!(keys(&view), [vec!["↑", "↓"], vec!["↵"], vec!["esc"]]);
    view.clear_query();
    view.editing = Some(Editing {
        block: "terminal-shell",
        key: "custom_shell",
        input: ui_kit::TextInput::new("/bin/zsh"),
    });
    assert_eq!(keys(&view), [vec!["↵"], vec!["esc"]]);
    // Painted in the frame's footer: the content ends above it.
    view.editing = None;
    let model = model();
    view.paint(
        &model,
        &Icons::builtin(),
        &mut TextSystem::new(),
        &mut Scene::default(),
        &Palette::dark(),
    );
    assert!(view.modal.content().max_y() < 800.0 - 44.0, "{:?}", view.modal.content());
}

/// A long Appearance page (the backdrop's block — a style, two sliders of many stops — and
/// more under it, the menu's switch last) with Glass in use: scrolled far past the themes,
/// its height stays the same every paint (a regression: the blocks were laid at a fixed
/// place once the page's top went above the window, its height growing with each wheel
/// turn and its last block never reached); the last switch shown and hit; back up.
#[test]
fn the_appearance_page_scrolls_to_its_last_block_with_a_glass_backdrop() {
    use workbench_model::SettingsRow;
    let mut model = model();
    let slider = |key, n: i64| SettingsRow::Slider {
        key,
        label: key.into(),
        detail: "detail".into(),
        default: Some(-1),
        stops: (-1..n).map(|v| (v, v.to_string())).collect(),
        current: -1,
    };
    let block = |id, order, rows| ContributedBlock {
        id,
        page: "appearance",
        page_info: None,
        order,
        advanced: false,
        language: None,
        keywords: "".into(),
        block: workbench_model::SettingsBlock { title: id.into(), detail: "detail".into(), rows },
    };
    let ids = ["a", "b", "c", "d", "e", "f"];
    for (i, id) in ids.iter().enumerate() {
        model.blocks.push(block(
            id,
            100 + i as u16,
            vec![choices("style", &[0, 1, 2], 2), slider("opacity", 13), slider("blur", 13)],
        ));
    }
    model.blocks.push(block(
        "window_menu",
        200,
        vec![SettingsRow::Toggle {
            key: "shown",
            label: "Menu".into(),
            detail: "detail".into(),
            on: true,
            default: Some(true),
        }],
    ));
    let palette = Palette {
        backdrop: ui_components::BackdropStyle::resolve(
            ui_components::backdrop::BackdropSpec {
                style: Some(ui_components::backdrop::BackdropKind::Glass),
                ..Default::default()
            },
            Palette::dark().window_background,
        ),
        ..Palette::dark()
    };
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 700.0), 2.0);
    view.open();
    let mut scene = Scene::default();
    let mut text = TextSystem::new();
    let mut paint = |view: &mut SettingsView| {
        scene.reset(Rgba::BLACK);
        view.paint(&model, &Icons::builtin(), &mut text, &mut scene, &palette);
    };
    paint(&mut view);
    let content = view.content;
    let height = view.content_height;
    assert!(height > 3.0 * content.size.height, "a long page: {height}");
    for _ in 0..80 {
        view.scroll_by(120.0);
        paint(&mut view);
        assert_eq!(view.content_height, height, "the same height");
    }
    assert_eq!(view.scroll, height - content.size.height, "at its end");
    let toggle = view
        .block_hits
        .iter()
        .find(|(_, a)| matches!(a, SettingsAction::Choose { block: "window_menu", .. }))
        .map(|(r, _)| *r)
        .expect("the menu's switch");
    assert!(toggle.max_y() <= content.max_y() && toggle.min_y() >= content.min_y(), "shown");
    let at = Point::new(toggle.min_x() + 4.0, toggle.min_y() + 4.0);
    assert!(matches!(view.hit(at), Some(SettingsAction::Choose { block: "window_menu", .. })));
    for _ in 0..80 {
        view.scroll_by(-120.0);
        paint(&mut view);
    }
    assert_eq!(view.scroll, 0.0, "back up");
}

/// A card preview that counts what it's asked.
#[derive(Debug, Default)]
struct Pictures {
    painted: std::sync::Mutex<Vec<(i64, bool)>>,
    released: std::sync::atomic::AtomicUsize,
}

impl ui_components::CardPreview for Pictures {
    fn id(&self) -> &'static str {
        "pictures"
    }
    fn paint(&self, value: i64, live: bool, p: ui_components::CardPaint<'_>) {
        self.painted.lock().unwrap().push((value, live));
        p.scene.push_layer(Some(p.clip)).quad(Quad::filled(p.bounds, Rgba::WHITE));
    }
    fn release(&self) {
        self.released.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A block of cards on a page of its own.
fn cards_block() -> ContributedBlock {
    let card = |value: i64, label: &str| workbench_model::SettingsCard {
        value,
        label: label.into(),
        tag: (value < 2).then(|| "3D".into()),
    };
    ContributedBlock {
        id: "cards",
        page: "cards",
        page_info: Some(SettingsPage {
            id: "cards",
            label: "settings.remote_section",
            icon: "cloud",
            group: SettingsGroup::Interface,
            order: 90,
        }),
        order: 10,
        advanced: false,
        language: None,
        keywords: "cards".into(),
        block: workbench_model::SettingsBlock {
            title: "Cards".into(),
            detail: String::new(),
            rows: vec![workbench_model::SettingsRow::Cards {
                key: "scene",
                label: "Scene".into(),
                detail: String::new(),
                cards: vec![card(0, "Stars"), card(1, "Waves"), card(2, "In turn")],
                current: 1,
                default: Some(0),
                preview: "pictures",
            }],
        },
    }
}

/// Cards: a click on one chooses it; the chosen one plays (frames asked while it shows), the
/// hovered one instead while hovered, the others still; another page or closing lets go.
#[test]
fn a_row_of_cards_is_chosen_and_plays_only_while_shown() {
    let pictures = std::sync::Arc::new(Pictures::default());
    let mut view = SettingsView::default();
    view.set_card_previews(vec![pictures.clone()]);
    view.set_viewport(Rect::new(0.0, 0.0, 1400.0, 1000.0), 1.0);
    view.open();
    let mut model = model();
    model.blocks.push(cards_block());
    let choose = |value| SettingsAction::Choose { block: "cards", key: "scene", value };
    paint(&mut view, &model);
    assert_eq!(view.card_deadline(), None, "another page: nothing plays");
    view.show_page("cards");
    pictures.painted.lock().unwrap().clear();
    paint(&mut view, &model);
    assert_eq!(*pictures.painted.lock().unwrap(), [(0, false), (1, true), (2, false)]);
    assert!(view.card_deadline().is_some(), "the chosen card plays");
    let card = |view: &SettingsView, value| {
        view.block_hits.iter().find(|(_, a)| *a == choose(value)).map(|(r, _)| *r).unwrap()
    };
    let third = card(&view, 2);
    let inside = Point::new(third.min_x() + 20.0, third.min_y() + 20.0);
    assert_eq!(view.hit(inside), Some(choose(2)), "a click chooses it");
    // Hovered: it plays instead (one at a time).
    view.hover(inside);
    pictures.painted.lock().unwrap().clear();
    paint(&mut view, &model);
    assert_eq!(*pictures.painted.lock().unwrap(), [(0, false), (1, false), (2, true)]);
    // Another page: no frames, what it made goes.
    view.show_page("appearance");
    paint(&mut view, &model);
    assert_eq!(view.card_deadline(), None);
    assert_eq!(pictures.released.load(std::sync::atomic::Ordering::Relaxed), 1);
    view.show_page("cards");
    paint(&mut view, &model);
    view.close();
    assert_eq!(view.card_deadline(), None, "closed: nothing plays");
    assert_eq!(pictures.released.load(std::sync::atomic::Ordering::Relaxed), 2);
}

/// A module's row: its switch, its "hidden" and its "at startup" are three places, each
/// asking for its own; "hidden" can't be changed while the module is off (it's kept).
#[test]
fn a_module_row_offers_on_hidden_and_at_startup_apart() {
    let paint = |view: &mut SettingsView, model: &SettingsModel| {
        view.paint(
            model,
            &Icons::builtin(),
            &mut TextSystem::new(),
            &mut Scene::default(),
            &Palette::dark(),
        );
    };
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 2400.0), 1.0);
    view.open();
    let mut shown = model();
    paint(&mut view, &shown);
    assert!(view.step_section(6), "the „Module” section");
    paint(&mut view, &shown);
    let id = "lang.php".to_owned();
    let hit = |view: &mut SettingsView, action: &SettingsAction| {
        let r = view.module_hits.iter().find(|(_, a)| a == action).map(|(r, _)| *r).unwrap();
        view.hit(Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0))
    };
    for action in [
        SettingsAction::ToggleModule(id.clone()),
        SettingsAction::ToggleModuleHidden(id.clone()),
        SettingsAction::TogglePreload(id.clone()),
    ] {
        assert_eq!(hit(&mut view, &action).as_ref(), Some(&action));
    }
    shown.modules[0].enabled = false;
    shown.modules[0].hidden = true;
    paint(&mut view, &shown);
    assert!(
        !view.module_hits.iter().any(|(_, a)| *a == SettingsAction::ToggleModuleHidden(id.clone())),
        "off: its „hidden” kept as it is"
    );
}

/// A dropdown row with more choices than its list shows: the list opens on a click, its
/// scrollbar's thumb pressed and dragged scrolls it (the press chooses nothing), a choice
/// afterwards is chosen.
#[test]
fn a_dropdown_s_list_scrolls_by_its_thumb() {
    let mut model = model();
    model.blocks = vec![ContributedBlock {
        id: "logs",
        page: "remote",
        page_info: Some(REMOTE),
        order: 100,
        advanced: false,
        language: None,
        keywords: "".into(),
        block: workbench_model::SettingsBlock {
            title: "Levels".into(),
            detail: String::new(),
            rows: vec![workbench_model::SettingsRow::Dropdown {
                key: "level",
                label: "Level".into(),
                detail: String::new(),
                icon: Some("modules"),
                choices: (0..40).map(|v| (v, format!("level {v}"))).collect(),
                current: 0,
                default: None,
            }],
        },
    }];
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 800.0), 1.0);
    view.open();
    view.set_sections(&model);
    assert!(view.step_section(2), "the page of the block");
    let mut text = TextSystem::new();
    let paint = |view: &mut SettingsView, text: &mut TextSystem| {
        view.paint(&model, &Icons::builtin(), text, &mut Scene::default(), &Palette::dark());
    };
    paint(&mut view, &mut text);
    let field = view.dropdowns[0].field;
    assert_eq!(view.hit(Point::new(field.min_x() + 5.0, field.min_y() + 5.0)), None);
    assert!(view.picker_open(), "its list opened");
    paint(&mut view, &mut text);
    let list = view.picker.list_area();
    let edge = list.max_x() - 4.0;
    let thumb = Point::new(edge, list.min_y() + 6.0);
    view.hover(thumb);
    assert_eq!(view.hit(thumb), None, "the thumb takes the press: nothing chosen");
    assert!(view.picker_open());
    assert!(view.scrollbar_held(), "the thumb holds the mouse");
    assert!(view.drag_scrollbar(Point::new(edge, list.min_y() + 120.0)), "the list follows");
    assert!(view.release());
    assert!(!view.scrollbar_held());
}

/// A module's row says who made it, its version and when it changed, those known.
#[test]
fn a_module_s_row_says_its_author_version_and_date() {
    let row = ModuleRow {
        author: "Binary Cube".into(),
        version: "1.0.0".into(),
        updated: "2026-11-20".into(),
        ..ModuleRow::default()
    };
    assert_eq!(super::modules::module_about(&row), "Binary Cube · 1.0.0 · 2026-11-20");
    let unknown = ModuleRow { author: "Someone".into(), ..ModuleRow::default() };
    assert_eq!(super::modules::module_about(&unknown), "Someone");
}

/// On the keymap's page, its shortcuts' search and the settings' search are one focus: a
/// click in the shortcuts' search gives it the keys (only it shows a caret), what's typed
/// and ⌘A are its, the settings' search untouched; Esc leaves it and typing searches the
/// settings again.
#[test]
fn the_shortcuts_search_and_the_settings_search_have_one_focus() {
    let model = model();
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 900.0), 1.0);
    view.open();
    view.set_sections(&model);
    view.show_page("keymap");
    let mut text = TextSystem::new();
    let mut paint = |view: &mut SettingsView| {
        view.paint(&model, &Icons::builtin(), &mut text, &mut Scene::default(), &Palette::dark());
    };
    paint(&mut view);
    let field = view.shortcuts.filter.area();
    assert!(field.size.width > 0.0, "the shortcuts' search drawn");
    view.hit(Point::new(field.min_x() + 40.0, field.min_y() + field.size.height / 2.0));
    assert!(view.shortcuts_have_keys(), "its search has the keys");
    view.type_text("save");
    assert_eq!(view.shortcuts.filter.text(), "save");
    assert_eq!(view.query.text(), "", "the settings' search untouched");
    view.query_input_mut().select_all();
    assert_eq!(view.query_input().selected_text(), "save", "⌘A in the shortcuts' search");
    assert_eq!(view.query.input.selected_text(), "", "not in the settings' search");
    assert!(view.cancel_text(), "Esc leaves it");
    assert!(!view.shortcuts_have_keys());
    view.type_text("x");
    assert_eq!(view.query.text(), "x", "typing searches the settings again");
}

/// The shortcuts' search focused, then its page left: copy and paste reach the same field
/// (the settings' search, the one shown), and asking for it never moves the focus.
#[test]
fn copy_and_paste_reach_one_field_when_the_keymap_is_left() {
    let model = model();
    let mut view = SettingsView::default();
    view.set_viewport(Rect::new(0.0, 0.0, 1200.0, 900.0), 1.0);
    view.open();
    view.set_sections(&model);
    view.show_page("keymap");
    let mut text = TextSystem::new();
    view.paint(&model, &Icons::builtin(), &mut text, &mut Scene::default(), &Palette::dark());
    let field = view.shortcuts.filter.area();
    view.hit(Point::new(field.min_x() + 40.0, field.min_y() + field.size.height / 2.0));
    view.type_text("save");
    view.shortcuts.filter.input.select_all();
    view.show_page("appearance");
    view.query.set_text("abc");
    view.query.input.select_all();
    assert_eq!(view.query_input().selected_text(), "abc", "⌘C copies the shown search");
    view.query_input_mut().insert("z");
    assert_eq!(view.query.text(), "z", "⌘V pastes into the same field");
    assert_eq!(view.shortcuts.filter.text(), "save", "the hidden search untouched");
    assert_eq!(view.controls.focused(), Some(FILTER), "the focus not moved");
}
