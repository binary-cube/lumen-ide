//! `@var` in Twig's and Blade's comments: the type's class names are PHP code, the
//! variable and everything else text.

use super::*;

/// What's at the first `marker` of `text` (the marker taken out).
fn at(text: &str, marker: &str) -> Option<Target> {
    let offset = text.find(marker).expect("the marker");
    let text = text.replacen(marker, "", 1);
    class_at(&text, offset)
}

fn class(name: &str) -> Option<Target> {
    Some(Target::Class(name.to_owned()))
}

#[test]
fn twig_s_var_names_its_class_in_either_order() {
    assert_eq!(at("{# @var user \\App\\Entity\\Us|er #}", "|"), class("\\App\\Entity\\User"));
    assert_eq!(at("{# @var \\App\\Entity\\Us|er user #}", "|"), class("\\App\\Entity\\User"));
    assert_eq!(at("{# @var users Us|er[] #}", "|"), class("User"));
    assert_eq!(at("{# @var us|er \\App\\User #}", "|"), None, "the variable");
    assert_eq!(at("{# @var user Us|er #}", "|"), class("User"), "Twig's order: the name first");
    assert_eq!(at("{# @var count in|t #}", "|"), None, "a scalar");
}

#[test]
fn blade_s_var_is_phpdoc_s_order() {
    let text = "{{-- @var \\App\\Models\\Us|er $user --}}";
    assert_eq!(at(text, "|"), class("\\App\\Models\\User"));
    assert_eq!(at("{{-- @var $us|er \\App\\User --}}", "|"), None);
    assert_eq!(at("{{-- @var Us|er|null $user --}}", "|"), class("User"));
}

#[test]
fn only_inside_a_var_comment() {
    assert_eq!(at("{# a User| here #}", "|"), None, "no @var");
    assert_eq!(at("{# @var user User #} Us|er", "|"), None, "after the comment");
    assert_eq!(at("{{ us|er.name }}", "|"), None);
    let multi = "{#\n  @var a \\App\\A\n  @var b \\App\\B|ee\n#}";
    assert_eq!(at(multi, "|"), class("\\App\\Bee"), "each @var of a long comment");
}

#[test]
fn a_template_has_no_class_of_its_own() {
    assert_eq!(at("{# @var item se|lf #}", "|"), None);
    assert_eq!(at("{{-- @var stat|ic $x --}}", "|"), None);
}
