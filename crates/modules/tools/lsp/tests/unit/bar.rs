use std::path::Path;
use std::sync::Arc;

use lsp_client::fake::Behaviour;

use super::*;
use crate::harness::{Harness, fresh_downloads};

/// Until `bar` (looking again) shows what `done` says (up to 5 s).
fn until(bar: &mut DownloadBar, what: &str, done: impl Fn(&DownloadBar) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        bar.poll();
        if done(bar) {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}: {bar:?}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn bar(language: &str) -> Option<Box<dyn EditorToolbar>> {
    let context = ToolbarContext {
        path: Path::new("/p/a.yaml"),
        language: Some(language),
        project: None,
        root: None,
        wake: Arc::new(|| {}),
    };
    OfferProvider.open(&context)
}

#[test]
fn the_bar_is_only_over_a_file_whose_server_can_be_downloaded() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("bar-open");
    let shown = bar("yaml").expect("a bar over a YAML file");
    assert_eq!(shown.height(), HEIGHT);
    assert!(bar("typescript").is_none());
    assert!(bar("plaintext").is_none());
    let none = ToolbarContext {
        path: Path::new("/p/a.yaml"),
        language: None,
        project: None,
        root: None,
        wake: Arc::new(|| {}),
    };
    assert!(OfferProvider.open(&none).is_none());
}

#[test]
fn it_says_the_language_what_is_downloaded_and_its_size() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("bar-text");
    let bar = DownloadBar::new("yaml", Arc::new(|| {}));
    let size = crate::downloads::size_of(&["node".to_owned(), "yaml-srv".to_owned()]);
    let text = bar.text();
    assert!(text.contains("yaml-srv + Node.js"), "{text}");
    assert!(text.contains(&i18n::bytes(size)), "{text}");
    assert_eq!(bar.button_list(), [Button::Download, Button::NotNow, Button::BuiltIn]);
}

#[test]
fn not_now_hides_the_language_s_bars_until_the_next_start() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("bar-not-now");
    let mut first = DownloadBar::new("yaml", Arc::new(|| {}));
    let mut second = DownloadBar::new("yaml", Arc::new(|| {}));
    assert!(first.press(Button::NotNow).is_none());
    assert!(!first.shown());
    assert_eq!(first.height(), 0.0);
    // Every bar of the language follows.
    assert!(second.poll());
    assert!(!second.shown());
    assert!(bar("yaml").is_none());
}

#[test]
fn the_built_in_engine_is_saved_for_the_language() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("bar-built-in");
    let mut bar = DownloadBar::new("yaml", Arc::new(|| {}));
    // Asked of the app (its command saves it in the user's settings).
    assert_eq!(
        bar.press(Button::BuiltIn),
        Some(ToolbarAction::View(ViewerAction::RunCommand(USE_BUILT_IN.into())))
    );
    let store = Arc::new(crate::harness::MemorySettings::default());
    let settings = settings_model::SettingsService::new(store);
    let choice =
        settings::Choice { engine: code_intelligence::servers::Engine::BuiltIn, server: None };
    settings::save_choice(&settings, "yaml", &choice);
    assert_eq!(settings.get(&settings::ENGINES), ["yaml=built-in"]);
    assert!(bar.poll());
    assert!(!bar.shown());
    assert!(offer("yaml").is_none());
}

#[test]
fn the_download_s_progress_and_its_stop() {
    let _h = Harness::new(Behaviour::default());
    let (_, memory) = fresh_downloads("bar-progress");
    let size =
        crate::downloads::manifest().named("node").unwrap().size(crate::downloads::platform());
    memory.stall_after(Some(size / 2));
    let mut bar = DownloadBar::new("yaml", Arc::new(|| {}));
    bar.press(Button::Download);
    until(&mut bar, "its progress", |b| b.text().contains(&i18n::bytes(size / 2)));
    assert_eq!(bar.button_list(), [Button::Stop]);
    bar.press(Button::Stop);
    memory.release();
    until(&mut bar, "stopped", |b| {
        b.button_list() == [Button::Download, Button::NotNow, Button::BuiltIn]
    });
    assert_eq!(crate::downloads::failure("node"), None, "a stop isn't a failure");
}

#[test]
fn a_failure_is_said_with_retry_and_the_end_is_told() {
    let _h = Harness::new(Behaviour::default());
    let (_, memory) = fresh_downloads("bar-retry");
    memory.set_offline(true);
    let mut bar = DownloadBar::new("yaml", Arc::new(|| {}));
    bar.press(Button::Download);
    until(&mut bar, "the failure", |b| b.button_list().first() == Some(&Button::Retry));
    let text = bar.text();
    assert!(text.contains("yaml-srv + Node.js"), "{text}");
    assert_eq!(
        text,
        i18n::tr!("lsp.offer_offline", language = "YAML", parts = "yaml-srv + Node.js")
    );
    memory.set_offline(false);
    bar.press(Button::Retry);
    until(&mut bar, "downloaded", |b| !b.shown());
    let told = bar.take_notifications();
    assert_eq!(told.len(), 1, "{told:?}");
}

/// A text too long beside the buttons goes on two lines: as many words on the first as fit.
#[test]
fn a_long_text_wraps_on_two_lines() {
    let fits = |s: &str| s.len() <= 12;
    assert_eq!(wrap_in_two("one two three four", fits), ["one two", "three four"]);
    assert_eq!(wrap_in_two("short", fits), ["short"]);
    assert_eq!(wrap_in_two("averyveryverylongword next", fits), ["averyveryverylongword", "next"]);
}

/// A language with several servers it can get offers them all, the first described first
/// (PHP: intelephense, installed with its licence agreed, then phpactor, downloaded); a pick
/// goes to the module's command (it saves the engine and gets the server).
#[test]
fn a_language_with_several_servers_offers_each() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("bar-several");
    crate::harness::fresh_installs("bar-several");
    crate::install::forget();
    crate::install::wait_known(std::time::Duration::from_secs(2));
    let offer = offer("markdown").expect("an offer");
    let servers: Vec<&str> = offer.choices.iter().map(|c| c.server.as_str()).collect();
    assert_eq!(servers, ["picky-install", "picky-download"]);
    assert!(offer.choices[0].install && !offer.choices[1].install);
    let mut bar = DownloadBar::new("markdown", Arc::new(|| {}));
    assert_eq!(bar.text(), tr!("lsp.offer_choose", language = language_name("markdown")));
    assert_eq!(
        bar.button_list(),
        [Button::Choose(0), Button::Choose(1), Button::NotNow, Button::BuiltIn]
    );
    assert!(matches!(
        bar.press(Button::Choose(1)),
        Some(ToolbarAction::View(ViewerAction::WithHost(_)))
    ));
}

/// A server that answers by default only once the user installs it (intelephense on
/// Windows, phpactor being the built-in engine's there): offered alone, to install.
#[test]
fn a_server_the_user_installs_is_offered_where_none_answers() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("bar-install-only");
    crate::harness::fresh_installs("bar-install-only");
    crate::install::forget();
    crate::install::wait_known(std::time::Duration::from_secs(2));
    let offer = offer("php").expect("an offer");
    assert_eq!(offer.choices.len(), 1);
    assert_eq!(offer.choices[0].server, "installable");
    let bar = DownloadBar::new("php", Arc::new(|| {}));
    assert_eq!(bar.button_list(), [Button::Choose(0), Button::NotNow, Button::BuiltIn]);
    assert!(bar.text().contains("installable"), "{}", bar.text());
}

/// Once the first is installed it's the default and nothing is offered; the status bar
/// names it, and the built-in engine once that's chosen.
#[test]
fn the_first_server_installed_is_the_default_and_named() {
    let _h = Harness::new(Behaviour::default());
    fresh_downloads("bar-installed");
    crate::harness::fresh_installs("bar-installed");
    crate::install::forget();
    crate::install::wait_known(std::time::Duration::from_secs(2));
    let project = kernel::ProjectId::new();
    assert_eq!(crate::engines::engine_label(project, "markdown"), None, "nothing to switch to yet");
    let spec = crate::catalog::catalog().named("picky-install").cloned().unwrap();
    crate::install::install(&spec, None).unwrap();
    assert!(offer("markdown").is_none());
    let choice = settings::default_choice("markdown");
    assert_eq!((choice.engine, choice.server), (code_intelligence::servers::Engine::Server, None));
    assert_eq!(crate::engines::engine_label(project, "markdown").as_deref(), Some("picky-install"));
    let store = Arc::new(crate::harness::MemorySettings::default());
    let service = settings_model::SettingsService::new(store);
    let built_in =
        settings::Choice { engine: code_intelligence::servers::Engine::BuiltIn, server: None };
    settings::save_choice(&service, "markdown", &built_in);
    assert_eq!(
        crate::engines::engine_label(project, "markdown"),
        Some(t("lsp.engine_status_built_in"))
    );
    settings::save_choice(&service, "markdown", &settings::default_choice("markdown"));
}
