use super::*;

#[test]
fn reads_routes_and_config() {
    let php = "<?php\nRoute::get('/users/{id}', [UserController::class, 'show'])->name('users.show');\nRoute::post('/login', fn () => 1)\n    ->middleware('guest')\n    ->name(\"login\");\n";
    let routes = route_names(php, Path::new("web.php"));
    let found: Vec<_> = routes.iter().map(|r| (r.name.as_str(), r.detail.as_deref())).collect();
    assert_eq!(found, [("users.show", Some("/users/{id}")), ("login", Some("/login"))]);
    let config = "<?php\nreturn [\n    // The name.\n    'name' => env('APP_NAME', 'x'),\n    'providers' => [\n        'inner' => 1,\n    ],\n];\n";
    let keys: Vec<_> = top_keys(config).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["name", "providers"]);
}
