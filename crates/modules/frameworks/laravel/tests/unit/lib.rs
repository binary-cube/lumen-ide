use super::*;

#[test]
fn knows_what_a_string_names() {
    let at = |src: &str, word: &str| {
        let content = quoted_at(src, src.find(word).expect("the word") + 1)?;
        slot(src, &content)
    };
    assert_eq!(at("return view('users.index', $data);", "users"), Some(Slot::View));
    assert_eq!(at("@extends('layouts.app')", "layouts"), Some(Slot::View));
    assert_eq!(at("<a href=\"{{ route('users.show', $u) }}\">", "users"), Some(Slot::Route));
    assert_eq!(at("config('app.name')", "app"), Some(Slot::Config));
    assert_eq!(at("Route::view('/welcome', 'welcome')", "/welcome"), None);
    assert_eq!(view_name("users/index.blade.php"), "users.index");
}
