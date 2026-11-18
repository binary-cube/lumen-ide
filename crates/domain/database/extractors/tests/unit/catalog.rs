use super::*;
use crate::extractor::{Input, Stream};

#[derive(Debug)]
struct Mine(&'static str, Group);

impl Extractor for Mine {
    fn id(&self) -> &str {
        self.0
    }
    fn name(&self) -> String {
        self.0.to_owned()
    }
    fn group(&self) -> Group {
        self.1
    }
    fn extension(&self) -> &str {
        "txt"
    }
    fn problem(&self) -> Option<String> {
        (self.0 == "broken").then(|| "1:3: unclosed".to_owned())
    }
    fn open(&self, _: &Input) -> Box<dyn Stream> {
        unreachable!()
    }
}

fn ids(c: &Catalog, project: Option<ProjectId>) -> Vec<String> {
    c.all(project).iter().map(|e| e.id().to_owned()).collect()
}

#[test]
fn the_built_in_ones_come_by_group() {
    let c = Catalog::new();
    assert_eq!(
        ids(&c, None),
        [
            "sql",
            "sql-multirow",
            "sql-update",
            "sql-where",
            "csv",
            "tsv",
            "tsv-header",
            "pipe",
            "semicolon",
            "json",
            "markdown",
            "html",
            "xml",
            "one-row",
            "pretty"
        ]
    );
    assert_eq!(c.chosen(None).id(), DEFAULT_ID);
}

#[test]
fn modules_and_projects_add_theirs() {
    let c = Catalog::new();
    let (a, b) = (ProjectId::new(), ProjectId::new());
    c.register(Arc::new(Mine("yaml", Group::Other)));
    c.set_scripted(a, vec![Arc::new(Mine("template:a", Group::Scripted))]);
    let of_a = ids(&c, Some(a));
    assert_eq!(of_a.iter().position(|i| i == "yaml"), Some(of_a.len() - 2));
    assert_eq!(of_a.last().map(String::as_str), Some("template:a"));
    // Another project doesn't see a's templates.
    assert!(!ids(&c, Some(b)).contains(&"template:a".to_owned()));
    c.forget(a);
    assert!(!ids(&c, Some(a)).contains(&"template:a".to_owned()));
}

#[test]
fn each_project_keeps_its_choice() {
    let c = Catalog::new();
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let before = c.generation();
    c.choose(Some(a), "json");
    assert!(c.generation() > before);
    assert_eq!(c.chosen(Some(a)).id(), "json");
    assert_eq!(c.chosen(Some(b)).id(), DEFAULT_ID);
    // One not offered (any more), or one with a problem: the default meanwhile.
    c.choose(Some(b), "gone");
    assert_eq!(c.chosen(Some(b)).id(), DEFAULT_ID);
    assert_eq!(c.chosen_id(Some(b)).as_deref(), Some("gone"));
    c.set_scripted(b, vec![Arc::new(Mine("broken", Group::Scripted))]);
    c.choose(Some(b), "broken");
    assert_eq!(c.chosen(Some(b)).id(), DEFAULT_ID);
}

#[test]
fn the_csv_formats_configured_replace_the_defaults() {
    let c = Catalog::new();
    let mine = crate::CsvFormat::new("csv-1", "Mine", "#", true);
    c.set_csv_formats(Some(vec![mine.clone()]));
    let all = ids(&c, None);
    assert!(all.contains(&"csv-1".to_owned()) && !all.contains(&"csv".to_owned()));
    assert_eq!(c.csv_formats(), vec![mine]);
    // TSV gone: the first separated format is the default.
    assert_eq!(c.chosen(None).id(), "csv-1");
    c.set_csv_formats(None);
    assert!(ids(&c, None).contains(&"tsv".to_owned()));
}

#[test]
fn options_are_kept() {
    let c = Catalog::new();
    let o = Options { skip_computed: true, skip_generated: false };
    c.set_options(o);
    assert_eq!(c.options(), o);
}
