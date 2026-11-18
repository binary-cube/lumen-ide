use super::*;

/// The name and description, named or by position, on one line or several; where the name
/// is written.
#[test]
fn commands_from_their_attribute() {
    let text = "<?php\nnamespace App\\Command;\n\n#[AsCommand(\n    name:        'app:auth:flush',\n    description: 'Sign every admin out',\n)]\nfinal class Flush {}\n\n#[AsCommand('app:x', 'Does x')]\nclass X {}\n";
    let file = std::path::Path::new("/p/src/Command/Flush.php");
    let found = from_attributes(text, file);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].name, "app:auth:flush");
    assert_eq!(found[0].description.as_deref(), Some("Sign every admin out"));
    assert_eq!(&text[found[0].offset..found[0].offset + 14], "app:auth:flush");
    assert_eq!(found[0].line, 4);
    assert_eq!(
        (found[1].name.as_str(), found[1].description.as_deref()),
        ("app:x", Some("Does x"))
    );
}

/// A positional description keeps what it says, colons and named arguments after it too.
#[test]
fn a_positional_description_with_a_colon() {
    let file = std::path::Path::new("src/Command/Import.php");
    let text = "#[AsCommand('app:import', 'Import: users', aliases: ['app:i'])]";
    let found = from_attributes(text, file);
    assert_eq!(found[0].description.as_deref(), Some("Import: users"));
}
