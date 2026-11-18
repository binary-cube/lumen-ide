use super::*;

#[test]
fn idle_sessions_close_after_ten_minutes_unless_set_otherwise() {
    assert_eq!(IDLE_CLOSE.default_value(), 10);
    assert!(IDLE_CHOICES.contains(&IDLE_CLOSE.default_value()));
    // The only test touching the configured value (it's the module's, for every project).
    assert_eq!(idle_close(), Some(Duration::from_secs(600)));
    configure(5);
    assert_eq!(idle_close(), Some(Duration::from_secs(300)));
    configure(0);
    assert_eq!(idle_close(), None, "never");
    configure(IDLE_CLOSE.default_value());
}

#[test]
fn the_page_size_reaches_the_grids_with_a_new_generation() {
    assert_eq!(PAGE_SIZE.default_value(), 500);
    assert!(PAGE_CHOICES.contains(&PAGE_SIZE.default_value()));
    let generation = page_generation();
    configure_page(100);
    assert_eq!(page_size(), Some(100));
    assert!(page_generation() > generation);
    let same = page_generation();
    configure_page(100);
    assert_eq!(page_generation(), same, "the same size: nothing new");
    configure_page(0);
    assert_eq!(page_size(), None, "all rows");
    configure_page(PAGE_SIZE.default_value());
    configure_sample(5000);
    assert_eq!(csv_sample(), 5000);
    configure_sample(CSV_SAMPLE.default_value());
    assert!(confirm_delete());
}

#[test]
fn every_choice_offered_is_a_valid_value() {
    for n in PAGE_CHOICES {
        assert!(PAGE_SIZE.accepts(&n), "{n}");
    }
    for n in CSV_SAMPLE_CHOICES {
        assert!(CSV_SAMPLE.accepts(&n), "{n}");
    }
    for n in IDLE_CHOICES {
        assert!(IDLE_CLOSE.accepts(&n), "{n}");
    }
}
