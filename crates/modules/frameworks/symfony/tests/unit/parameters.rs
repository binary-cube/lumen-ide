use super::*;

#[test]
fn reads_the_container_and_finds_the_names() {
    let xml = "<parameters>\n  <parameter key=\"kernel.project_dir\">/app</parameter>\n  <parameter key=\"kernel.bundles\" type=\"collection\">\n    <parameter key=\"x\">y</parameter>\n  </parameter>\n</parameters>\n";
    let all = from_container(xml, Path::new("c.xml"));
    let dir = all.iter().find(|p| p.name == "kernel.project_dir").unwrap();
    assert_eq!(dir.value.as_deref(), Some("/app"));
    assert!(all.iter().find(|p| p.name == "kernel.bundles").unwrap().value.is_none());
    let yaml =
        "    arguments: ['%kernel.project_dir%/var', '%%literal%%', '%env(DATABASE_URL)%']\n";
    let named: Vec<&str> = uses(yaml).into_iter().map(|r| &yaml[r]).collect();
    assert_eq!(named, ["kernel.project_dir"]);
    let php = "#[Autowire(param: 'app.email')] $this->getParameter('locale'); sprintf('%s %d', 1);";
    let named: Vec<&str> = uses(php).into_iter().map(|r| &php[r]).collect();
    assert_eq!(named, ["app.email", "locale"]);
    let at = php.find("locale").unwrap() + 1;
    assert_eq!(named_at(php, at).map(|r| &php[r]), Some("locale"));
}
