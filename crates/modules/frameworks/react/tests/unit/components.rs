use super::*;

#[test]
fn exported_components() {
    let src = "import x from 'y';\nexport function UserCard() {}\nexport default function Page() {}\nexport const Button = () => null;\nexport const helper = 1;\nexport default class App {}\n";
    let names: Vec<_> =
        exported(src, std::path::Path::new("a.jsx")).into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["UserCard", "Page", "Button", "App"]);
}
