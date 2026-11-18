//! The bar's layout: everything when it fits, the run's words cut first, then the lower
//! controls into "…", never overlapping; Run, Stop, the transaction and the schema always.

use super::*;

fn spec(width: f32, status: Option<f32>) -> BarSpec {
    BarSpec {
        min_x: 0.0,
        max_x: width,
        metrics: Metrics::new(|v| v),
        transaction: false,
        tx_w: 80.0,
        mode_w: 100.0,
        place_w: 90.0,
        status_w: status,
        limited_w: None,
        read_only_w: None,
        production_w: None,
        source_w: None,
    }
}

fn overlaps(l: &BarLayout) -> bool {
    let mut parts: Vec<(f32, f32)> = l.placed.iter().map(|&(_, x, w)| (x, x + w)).collect();
    parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    parts.windows(2).any(|w| w[1].0 < w[0].1 - 0.5)
}

#[test]
fn everything_fits_on_a_wide_bar() {
    let l = layout(&spec(1200.0, Some(150.0)));
    assert!(l.collapsed.is_empty() && !l.shows(Tool::More));
    for tool in [
        Tool::Run,
        Tool::History,
        Tool::Tx,
        Tool::Stop,
        Tool::Mode,
        Tool::Settings,
        Tool::Services,
        Tool::Place,
        Tool::Status,
    ] {
        assert!(l.shows(tool), "{tool:?}");
    }
    assert_eq!(l.status_room, 150.0);
    assert!(!overlaps(&l));
    // Left to right; the schema at the right edge.
    let x = |t| l.get(t).unwrap().0;
    assert!(x(Tool::Run) < x(Tool::History) && x(Tool::History) < x(Tool::Tx));
    assert!(x(Tool::Tx) < x(Tool::Stop) && x(Tool::Stop) < x(Tool::Mode));
    let (place, w) = l.get(Tool::Place).unwrap();
    assert_eq!(place + w, 1200.0 - 8.0);
}

#[test]
fn narrower_the_lower_controls_go_into_the_menu() {
    for width in [900.0, 600.0, 450.0, 380.0, 340.0] {
        let l = layout(&spec(width, Some(200.0)));
        assert!(!overlaps(&l), "{width}: {l:?}");
        for tool in [Tool::Run, Tool::Tx, Tool::Stop, Tool::Place] {
            assert!(l.shows(tool), "{width}: {tool:?}");
        }
        if !l.collapsed.is_empty() {
            assert!(l.shows(Tool::More));
        }
    }
    let narrow = layout(&spec(450.0, Some(200.0)));
    assert!(narrow.collapsed.contains(&Tool::Services), "{narrow:?}");
    // The status shrinks before anything goes.
    let cut = layout(&spec(720.0, Some(400.0)));
    assert!(cut.collapsed.is_empty() && cut.status_room < 400.0, "{cut:?}");
}

#[test]
fn an_open_transaction_shows_commit_and_rollback() {
    let mut s = spec(1200.0, None);
    s.transaction = true;
    let l = layout(&s);
    assert!(l.shows(Tool::Commit) && l.shows(Tool::Rollback));
    assert!(!l.shows(Tool::Status));
}

#[test]
fn a_production_badge_stays_whatever_the_width() {
    for width in [1400.0, 700.0, 420.0] {
        let spec = BarSpec { production_w: Some(48.0), ..spec(width, Some(120.0)) };
        let l = layout(&spec);
        assert!(l.shows(Tool::Production), "at {width}");
        assert!(!l.collapsed.contains(&Tool::Production));
        assert!(!overlaps(&l), "at {width}: {l:?}");
        // Beside the schema, on its left.
        let (px, pw) = l.get(Tool::Production).unwrap();
        let (sx, _) = l.get(Tool::Place).unwrap();
        assert!(px + pw <= sx, "at {width}");
    }
}

/// An attached SQL file's bar (39n): its source's pill left of the schema, always kept.
#[test]
fn an_attached_file_s_source_stays_beside_the_schema() {
    for width in [1200.0, 600.0, 380.0] {
        let mut s = spec(width, Some(150.0));
        s.source_w = Some(110.0);
        let l = layout(&s);
        assert!(!overlaps(&l), "{width}: {l:?}");
        let (source, w) = l.get(Tool::Source).unwrap_or_else(|| panic!("{width}: {l:?}"));
        let (place, _) = l.get(Tool::Place).unwrap();
        assert!(source + w <= place, "{width}: left of the schema");
        assert!(l.shows(Tool::Run) && l.shows(Tool::Tx), "{width}");
    }
    assert!(!layout(&spec(1200.0, None)).shows(Tool::Source), "a console has none");
}
