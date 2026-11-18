use super::*;

#[test]
fn an_age_is_said_shortly() {
    assert_eq!(short(-5), i18n::t("agents.age_now"));
    assert_eq!(short(59), i18n::t("agents.age_now"));
    assert_eq!(short(5 * 60), i18n::tr!("agents.age_minutes", n = 5));
    assert_eq!(short(18 * 3_600 + 59), i18n::tr!("agents.age_hours", n = 18));
    assert_eq!(short(3 * 86_400), i18n::tr!("agents.age_days", n = 3));
    assert_eq!(short(15 * 86_400), i18n::tr!("agents.age_weeks", n = 2));
}
