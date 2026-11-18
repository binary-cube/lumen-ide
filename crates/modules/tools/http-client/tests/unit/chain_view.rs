//! A file's chain laid out: a card per request, an edge per variable taken from a response,
//! labeled by what of it is taken; a dot's place along an edge; the layout made on a worker,
//! the requests' states read once per change of the runs.

use std::time::Duration;

use super::*;

const FILE: &str = "# @name login\nPOST https://e.com/login\n\n###\n# @name me\nGET https://e.com/me\nAuthorization: Bearer {{login.response.body.$.token}}\n\n###\nGET https://e.com/x\n";

#[test]
fn the_chain_is_laid_out_left_to_right() {
    let parsed = http_requests::parse(FILE);
    let laid = lay_out(&parsed);
    assert_eq!(laid.cards.len(), 3);
    assert_eq!(laid.edges.len(), 1);
    let (from, to, points, label, _) = &laid.edges[0];
    assert_eq!((*from, *to), (0, 1));
    assert_eq!(label, "body.$.token");
    assert!(laid.cards[1].min_x() > laid.cards[0].max_x(), "the one using it to the right");
    assert_eq!(points.first().map(|p| p.x), Some(laid.cards[0].max_x()));
    assert_eq!(points.last().map(|p| p.x), Some(laid.cards[1].min_x()));
}

#[test]
fn a_dot_goes_along_an_edge() {
    let points = [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0)];
    assert_eq!(along(&points, 0.0), Point::new(0.0, 0.0));
    assert_eq!(along(&points, 0.5), Point::new(10.0, 0.0));
    assert_eq!(along(&points, 0.75), Point::new(10.0, 5.0));
    assert_eq!(along(&points, 1.0), Point::new(10.0, 10.0));
}

#[test]
fn it_opens_on_a_file_and_follows_its_runs() {
    let runs = Runs::of(Some(kernel::ProjectId::new()), None);
    let mut view = ChainView::default();
    view.set_data(&ChainData {
        runs: Arc::clone(&runs),
        file: "/p/a.http".into(),
        text: FILE.into(),
    });
    assert!(view.graph.parsed.requests.is_empty(), "laid out on a worker, not here");
    let started = Instant::now();
    while !view.poll() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(view.graph.parsed.requests.len(), 3, "ready by a poll");
    assert!(view.viewport.fit_pending, "fitted once it's there");
    assert!(view.state_of(0).is_none(), "not run yet");
    assert!(!view.running());
    assert!(!view.is_loading(), "no frames while nothing runs");
    assert!(!view.poll(), "nothing new: nothing to draw");
}
