use super::*;

const SYMFONY: EnvFileOrder = EnvFileOrder {
    name: "Symfony",
    markers: &["symfony.lock"],
    files: &[".env", ".env.local", ".env.{env}", ".env.{env}.local"],
};
const LARAVEL: EnvFileOrder =
    EnvFileOrder { name: "Laravel", markers: &["artisan"], files: &[".env", ".env.{env}"] };

fn sorted(names: &[&'static str], order: &EnvFileOrder) -> Vec<&'static str> {
    let mut names = names.to_vec();
    sort(&mut names, order, |n| n);
    names
}

#[test]
fn symfonys_files_in_the_order_it_loads_them() {
    let names = [".env.example", ".env.prod.local", ".env", ".env.prod", ".env.local", ".env.dev"];
    assert_eq!(
        sorted(&names, &SYMFONY),
        [".env", ".env.local", ".env.dev", ".env.prod", ".env.prod.local", ".env.example"]
    );
}

#[test]
fn laravels_environment_file_comes_after_the_main_one() {
    assert_eq!(
        sorted(&[".env.testing", ".env", ".env.example"], &LARAVEL),
        [".env", ".env.testing", ".env.example"]
    );
}

#[test]
fn the_projects_order_is_its_frameworks() {
    let orders = [SYMFONY, LARAVEL];
    assert_eq!(of_project(&orders, &|m| m == "artisan").name, "Laravel");
    assert_eq!(of_project(&orders, &|_| false).files, COMMON.files);
}

#[test]
fn documentation_files() {
    for name in [".env.example", ".env.dist", ".env.sample", ".env.template", "prod.env.example"] {
        assert!(documents(name), "{name}");
    }
    for name in [".env", ".env.local", ".env.prod", "docker.env"] {
        assert!(!documents(name), "{name}");
    }
}
