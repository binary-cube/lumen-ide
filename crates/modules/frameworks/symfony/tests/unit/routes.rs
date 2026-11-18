use super::*;

#[test]
fn reads_attributes_and_yaml() {
    let php = "<?php\n#[Route('/blog', name: 'blog_')]\nclass BlogController\n{\n    #[Route('/{slug}', name: 'show', methods: ['GET'])]\n    public function show() {}\n    #[Route(path: '/', name: \"index\")]\n    public function index() {}\n}\n";
    let routes = from_attributes(php, std::path::Path::new("a.php"));
    let names: Vec<_> = routes.iter().map(|r| (r.name.as_str(), r.path.as_deref())).collect();
    assert_eq!(names, [("blog_show", Some("/blog/{slug}")), ("blog_index", Some("/blog/"))]);
    assert_eq!(&php[routes[0].offset..routes[0].offset + 4], "show");
    let yaml =
        "home:\n    path: /\n    controller: App\\Home\n\ncontrollers:\n    resource: ../src/\n";
    let routes = from_yaml(yaml, std::path::Path::new("r.yaml"));
    assert_eq!(routes[0].name, "home");
    assert_eq!(routes[0].path.as_deref(), Some("/"));
}

#[test]
fn finds_the_routes_used_and_declared_whole() {
    let twig = "<a href=\"{{ path('blog_show', {slug: s}) }}\">{{ url( \"home\" ) }}</a> path(x)";
    let used: Vec<&str> = uses(twig).into_iter().map(|r| &twig[r]).collect();
    assert_eq!(used, ["blog_show", "home"]);
    let prefixed = "<?php\n#[Route('/blog', name: 'blog_')]\nclass B\n{\n    #[Route('/x', name: 'show')]\n    public function show() {}\n}\n";
    let php =
        "<?php\nclass H\n{\n    #[Route('/', name: 'home')]\n    public function home() {}\n}\n";
    let home = declarations("src/H.php", php, "home");
    assert_eq!(home.len(), 1);
    assert_eq!(&php[home[0].clone()], "home");
    assert!(
        declarations("src/B.php", prefixed, "blog_show").is_empty(),
        "through its class's prefix"
    );
    let yaml = "home:\n    path: /\n";
    assert_eq!(declarations("config/routes.yaml", yaml, "home"), vec![0..4]);
    assert!(declarations("config/packages/x.yaml", yaml, "home").is_empty(), "not a route file");
    assert!(is_route_file("config/routes/admin.yml"));
}
