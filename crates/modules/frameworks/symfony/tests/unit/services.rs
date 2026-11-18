use super::*;

#[test]
fn reads_the_compiled_container() {
    let xml = "<container>\n  <services>\n    <service id=\"mailer.mailer\" class=\"Symfony\\Component\\Mailer\\Mailer\" public=\"true\"/>\n    <service id=\"mailer\" alias=\"mailer.mailer\"/>\n    <service id=\".service_locator.x\" class=\"X\"/>\n  </services>\n</container>\n";
    let all = from_container(xml, Path::new("c.xml"));
    let ids: Vec<(&str, Option<&str>)> =
        all.iter().map(|s| (s.id.as_str(), s.class.as_deref())).collect();
    assert_eq!(
        ids,
        [
            ("mailer", Some("Symfony\\Component\\Mailer\\Mailer")),
            ("mailer.mailer", Some("Symfony\\Component\\Mailer\\Mailer"))
        ]
    );
    assert_eq!(&xml[all[1].offset..all[1].offset + 13], "mailer.mailer");
}

#[test]
fn reads_services_yaml_and_the_classes_it_loads() {
    let yaml = "parameters:\n    x: 1\n\nservices:\n    _defaults:\n        autowire: true\n\n    App\\:\n        resource: '../src/'\n\n    app.slugger:\n        class: App\\Util\\Slugger\n    'App\\Mail\\Sender': ~\n";
    let sources = vec!["Util/Slugger.php".to_owned(), "Controller/Blog.php".to_owned()];
    let all = from_yaml(yaml, Path::new("/p/config/services.yaml"), Path::new("/p/src"), &sources);
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        ["App\\Controller\\Blog", "App\\Mail\\Sender", "App\\Util\\Slugger", "app.slugger"]
    );
    let slugger = all.iter().find(|s| s.id == "app.slugger").unwrap();
    assert_eq!(slugger.class.as_deref(), Some("App\\Util\\Slugger"));
    assert_eq!(&yaml[slugger.offset..slugger.offset + 11], "app.slugger");
    let sender = all.iter().find(|s| s.id == "App\\Mail\\Sender").unwrap();
    assert_eq!(&yaml[sender.offset..sender.offset + 3], "App", "its key, inside its quotes");
}

#[test]
fn finds_the_ids_named() {
    let yaml = "    arguments: ['@app.slugger', '@@escaped', '@mailer']\n";
    let named: Vec<&str> = uses(yaml).into_iter().map(|r| &yaml[r]).collect();
    assert_eq!(named, ["app.slugger", "mailer"]);
    let php = "$container->get('app.slugger'); $request->get('page'); service('mailer');\n#[Autowire(service: 'cache.app')]";
    let named: Vec<&str> = uses(php).into_iter().map(|r| &php[r]).collect();
    assert_eq!(named, ["app.slugger", "mailer", "cache.app"]);
    let at = php.find("slugger").unwrap();
    assert_eq!(named_at(php, at).map(|r| &php[r]), Some("app.slugger"));
}
