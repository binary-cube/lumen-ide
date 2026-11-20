use super::*;
use workbench_model::running::StoppingProgram;

fn of(count: usize, ended: usize) -> Stopping {
    Stopping {
        programs: (0..count)
            .map(|i| StoppingProgram {
                name: "zsh".into(),
                pid: u32::try_from(i).unwrap(),
                state: if i < ended { ProgramState::Ended } else { ProgramState::Asked },
            })
            .collect(),
    }
}

/// Counted as they end; done once all have; the list scrolls only past its visible rows.
#[test]
fn how_far_and_how_much_the_list_scrolls() {
    assert_eq!((of(4, 1).ended(), of(4, 1).done()), (1, false));
    assert!(of(4, 4).done());
    assert!(of(0, 0).done(), "nothing to stop is done");
    assert_eq!(max_scroll(&of(VISIBLE_ROWS, 0)), 0.0);
    assert_eq!(max_scroll(&of(VISIBLE_ROWS + 2, 0)), ROW_H * 2.0);
    assert_eq!(height(&of(0, 0)), height(&of(1, 0)), "an empty list keeps one row");
}
