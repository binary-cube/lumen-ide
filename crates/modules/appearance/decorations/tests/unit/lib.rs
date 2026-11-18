//! The provider: nothing made when there's nothing to draw or they're off; the user's own
//! decorations over a theme's, instead of them, or over a theme without any; every change
//! counted (the windows make theirs again).

use super::*;
use settings::{Mine, Overlay};
use theming::decorations::{LayerKind, LayerSpec, OverlayMode};

/// The settings are process-wide: the tests touching them take turns.
static TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn christmas() -> Arc<Decorations> {
    Arc::new(Decorations {
        layers: vec![LayerSpec::new(LayerKind::Garland), LayerSpec::new(LayerKind::Particles)],
        dark: true,
    })
}

#[test]
fn decorations_are_made_only_when_there_are_some_and_they_are_on() {
    let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
    let provider = Provider;
    let none = Arc::new(Decorations::default());
    let some = christmas();
    settings::set_overlay(Overlay::default());
    settings::set_mode(DecorationMode::On);
    assert!(provider.create(&none).is_none(), "no decorations: nothing made");
    let made = provider.create(&some).unwrap();
    assert!(made.moves());
    assert!(made.memory() > 0);
    assert_eq!(made.gpu_memory(), 0, "nothing on the GPU before it's drawn");

    let before = provider.version();
    settings::set_mode(DecorationMode::Static);
    assert_eq!(provider.mode(), DecorationMode::Static);
    assert!(provider.version() > before, "a change is counted");
    assert!(provider.create(&some).is_some(), "static: still drawn");
    let again = provider.version();
    settings::set_mode(DecorationMode::Static);
    assert_eq!(provider.version(), again, "the same choice changes nothing");
    settings::set_mode(DecorationMode::Off);
    assert!(provider.create(&some).is_none(), "off: nothing made");
    settings::set_mode(DecorationMode::On);
}

#[test]
fn the_user_s_own_go_over_any_theme() {
    let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
    settings::set_mode(DecorationMode::On);
    let none = Arc::new(Decorations { layers: Vec::new(), dark: false });
    let autumn = Overlay { mine: Mine::Preset("autumn"), ..Overlay::default() };
    let before = Provider.version();
    settings::set_overlay(autumn.clone());
    assert!(Provider.version() > before, "the windows make theirs again");
    assert!(Provider.create(&none).is_some(), "a theme without decorations gets the user's");
    let drawn = with_overlay(&none, &autumn);
    assert_eq!(drawn.layers, theming::decorations::presets::layers("autumn", false).unwrap());
    // Over the theme's, or instead of them.
    let theme = christmas();
    let over = with_overlay(&theme, &autumn);
    assert_eq!(over.layers.len(), 2 + drawn.layers.len());
    let instead = with_overlay(&theme, &Overlay { with: OverlayMode::Replace, ..autumn });
    assert_eq!(instead.layers, theming::decorations::presets::layers("autumn", true).unwrap());
    // None: the theme's own, the same `Arc`.
    assert!(Arc::ptr_eq(&with_overlay(&theme, &Overlay::default()), &theme));
    // The studio's layers.
    let custom = Overlay {
        mine: Mine::Custom,
        custom: vec![LayerSpec::new(LayerKind::Glow)],
        ..Overlay::default()
    };
    assert_eq!(with_overlay(&none, &custom).kinds().collect::<Vec<_>>(), [LayerKind::Glow]);
    settings::set_overlay(Overlay::default());
    assert!(Provider.create(&none).is_none());
}

#[test]
fn the_settings_accept_only_what_they_know() {
    let valid = |s: &str| settings::DECORATIONS.accepts(&s.to_owned());
    assert!(valid("on") && valid("static") && valid("off"));
    assert!(!valid("blinking"));
    let mine = |s: &str| settings::MY_DECORATIONS.accepts(&s.to_owned());
    assert!(mine("none") && mine("custom") && mine("autumn") && mine("new-year"));
    assert!(!mine("easter"));
    for id in ["none", "custom", "winter"] {
        assert_eq!(Mine::parse(id).map(Mine::id), Some(id));
    }
    let with = |s: &str| settings::MY_DECORATIONS_WITH.accepts(&s.to_owned());
    assert!(with("add") && with("replace") && !with("both"));
    let layers = |l: &[&str]| {
        settings::MY_DECORATIONS_LAYERS.accepts(&l.iter().map(|s| (*s).to_owned()).collect())
    };
    assert!(layers(&["kind=glow;opacity=0.2", "kind=particles;shape=leaves"]));
    assert!(!layers(&["kind=glow;opacity=2"]));
    assert!(!layers(&["kind=glow"; 9]), "at most so many layers");
}

/// The digital rain set by the user over every theme (the "Matrix" preset, plan 45) stays
/// whatever the theme — it's theirs, not Matrix's —; over Matrix's own rain it's added (or
/// replaces it), and with nothing of theirs a plain theme has none.
#[test]
fn the_user_s_rain_stays_over_any_theme() {
    let rain = Overlay { mine: Mine::Preset("matrix"), ..Overlay::default() };
    let plain = Arc::new(Decorations { layers: Vec::new(), dark: true });
    let matrix_theme = Arc::new(Decorations {
        layers: theming::decorations::presets::layers("matrix", true).unwrap(),
        dark: true,
    });
    let kinds = |d: &Decorations| d.kinds().collect::<Vec<_>>();
    assert_eq!(kinds(&with_overlay(&plain, &rain)), [LayerKind::Matrix]);
    assert!(with_overlay(&plain, &Overlay::default()).is_empty(), "nothing of theirs: none");
    assert_eq!(kinds(&with_overlay(&matrix_theme, &rain)), [LayerKind::Matrix, LayerKind::Matrix]);
    let instead = Overlay { with: OverlayMode::Replace, ..rain };
    assert_eq!(kinds(&with_overlay(&matrix_theme, &instead)), [LayerKind::Matrix]);
}

/// "Pause theme effects when idle": each choice saved and handed to the windows (20 s by
/// default; 10 s, offered before, still read; "never": no pause).
#[test]
fn the_idle_pause_takes_each_choice() {
    let _turn = TURNS.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(settings::DECORATIONS_IDLE.default_value(), 20);
    for secs in settings::IDLE_CHOICES.into_iter().chain([10]) {
        assert!(settings::DECORATIONS_IDLE.accepts(&secs), "{secs}");
        settings::set_pace(20, secs, true);
        let expected = (secs > 0).then(|| std::time::Duration::from_secs(secs as u64));
        assert_eq!(settings::idle(), expected, "{secs}");
    }
    assert!(!settings::DECORATIONS_IDLE.accepts(&7));
    settings::set_pace(20, 20, true);
}
