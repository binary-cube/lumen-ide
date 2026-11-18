//! The templates put in a project's extractors' folder the first time it's made ("Go to
//! Extractors Directory"), as starting points: SQL `INSERT`s many rows a statement, a Python
//! DataFrame, a PHP array, a Laravel seeder, a Markdown list.

use std::path::{Path, PathBuf};

/// The examples: their file's name and their text.
pub const EXAMPLES: &[(&str, &str)] = &[
    (
        "SQL-Insert-Multirow.extractor",
        "---
name: SQL Insert (multirow template)
extension: sql
---
{# Every row in one INSERT, the values written by the source's dialect. #}
INSERT INTO {{table|sql}} ({{#columns}}{{column.name|sql}}{{^last}}, {{/last}}{{/columns}})
VALUES
{{#rows}}
    ({{#columns}}{{value|sql}}{{^last}}, {{/last}}{{/columns}}){{^last}},{{/last}}{{#last}};{{/last}}
{{/rows}}
",
    ),
    (
        "Python-DataFrame.extractor",
        "---
name: Python DataFrame
extension: py
---
import pandas as pd

{{table|lower}} = pd.DataFrame([
{{#rows}}
    { {{#columns}}{{column.name|py}}: {{value|py}}{{^last}}, {{/last}}{{/columns}} },
{{/rows}}
])
",
    ),
    (
        "PHP-Array.extractor",
        "---
name: PHP Array
extension: php
---
<?php

return [
{{#rows}}
    [{{#columns}}{{column.name|php}} => {{value|php}}{{^last}}, {{/last}}{{/columns}}],
{{/rows}}
];
",
    ),
    (
        "Laravel-Seeder.extractor",
        "---
name: Laravel Seeder
extension: php
---
<?php

namespace Database\\Seeders;

use Illuminate\\Database\\Seeder;
use Illuminate\\Support\\Facades\\DB;

class DataSeeder extends Seeder
{
    public function run(): void
    {
        DB::table({{table|php}})->insert([
{{#rows}}
            [{{#columns}}{{column.name|php}} => {{value|php}}{{^last}}, {{/last}}{{/columns}}],
{{/rows}}
        ]);
    }
}
",
    ),
    (
        "Markdown-List.extractor",
        "---
name: Markdown List
extension: md
---
## {{table|md}}

{{#rows}}
- {{#columns}}**{{column.name|md}}**: {{#null}}_NULL_{{/null}}{{^null}}{{value|md}}{{/null}}{{^last}}, {{/last}}{{/columns}}
{{/rows}}
",
    ),
];

/// The examples written in `dir` (made if missing), those already there left as they are:
/// the files written.
pub fn write(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for (name, text) in EXAMPLES {
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        std::fs::write(&path, text)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
#[path = "../tests/unit/examples.rs"]
mod tests;
