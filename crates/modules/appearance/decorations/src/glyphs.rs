//! Matrix's characters, drawn once as icons of the app's set (strokes on a 10 × 14 grid, in
//! a square): the renderer keeps them in its glyph atlas and draws every falling character
//! in one pass, each in its own color and brightness. Katakana mirrored (as in the film),
//! digits, a few Latin capitals; and the holographic signs' kanji-like glyphs (plan 45).
//! Contributed by the module as icons named `fx-<set>-<n>`;
//! nothing is drawn until an effect asks for one.

use languages::IconSvg;

/// A glyph as an icon: its strokes in a 14 × 14 square (the 10 wide grid centered),
/// mirrored for the katakana.
macro_rules! glyph {
    ($name:literal, mirrored, $d:literal) => {
        IconSvg {
            name: $name,
            svg: concat!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 14 14\" fill=\"none\" stroke=\"#000\" stroke-width=\"1.25\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path transform=\"translate(12 0) scale(-1 1)\" d=\"",
                $d,
                "\"/></svg>"
            ),
        }
    };
    ($name:literal, $d:literal) => {
        IconSvg {
            name: $name,
            svg: concat!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 14 14\" fill=\"none\" stroke=\"#000\" stroke-width=\"1.25\" stroke-linecap=\"round\" stroke-linejoin=\"round\"><path transform=\"translate(2 0)\" d=\"",
                $d,
                "\"/></svg>"
            ),
        }
    };
}

/// The katakana (mirrored).
pub const KATAKANA: &[IconSvg] = &[
    glyph!("fx-k-0", mirrored, "M1.5 2.5H8.5L6 6M5 4.5C5 8.5 4 10.5 2 12.5"),
    glyph!("fx-k-1", mirrored, "M8 1.5L2 7M5.5 4.5V13"),
    glyph!("fx-k-2", mirrored, "M5 1V3M1.5 4V6M1.5 4H8.5V6C8.5 9 6.5 11.5 3.5 13"),
    glyph!("fx-k-3", mirrored, "M2 3H8M5 3V11M1 11H9"),
    glyph!("fx-k-4", mirrored, "M1 4.5H9M6.5 1.5V12.5L5 11.5M6 5.5L1.5 11"),
    glyph!("fx-k-5", mirrored, "M1.5 4.5H8.5C8.5 9 8 12 6 12.5M5 1.5C5 7 3.5 10.5 1.5 12.5"),
    glyph!("fx-k-6", mirrored, "M2 4H8M1.5 7.5H8.5M4.5 1.5L6 13"),
    glyph!("fx-k-7", mirrored, "M4.5 1.5L1.5 6M3.5 3.5H8.5C8 8 6 11 2.5 13"),
    glyph!("fx-k-8", mirrored, "M3.5 1.5L1 6M2.5 4.5H9M6 4.5C6 8.5 5 11 3 13"),
    glyph!("fx-k-9", mirrored, "M1.5 3H8.5V11.5H1.5"),
    glyph!("fx-k-10", mirrored, "M1 5H9M3.5 1.5V8M7 1.5V7.5C7 10 6 11.5 3.5 13"),
    glyph!("fx-k-11", mirrored, "M2 2L4 3.5M1.5 5.5L3.5 7M2 12.5C5 11 7.5 8 9 3.5"),
    glyph!("fx-k-12", mirrored, "M2 2.5H8C7 7 4.5 10.5 1.5 12.5M5.5 8.5L8.5 12"),
    glyph!("fx-k-13", mirrored, "M1 6L9 4.5L7 7.5M3.5 1.5V11.5C3.5 12.5 4 12.8 5 12.8H8.5"),
    glyph!("fx-k-14", mirrored, "M1.5 3L3.5 6.5M8.5 2.5C8 7.5 5.5 11 2.5 13"),
    glyph!("fx-k-15", mirrored, "M4.5 1.5L1.5 6M3.5 3.5H8.5C8 8 6 11 2.5 13M3 7L7 9"),
    glyph!("fx-k-16", mirrored, "M7.5 1.5L2.5 3M1 6.5H9M5 3V9C5 11 4 12 2.5 13"),
    glyph!("fx-k-17", mirrored, "M1 3L2.5 6M4.5 2.5L5.5 5.5M9 2.5C8.5 7.5 6 11 2.5 13"),
    glyph!("fx-k-18", mirrored, "M2 2H8M1 5.5H9M5 5.5C5 9 4 11.5 2 13"),
    glyph!("fx-k-19", mirrored, "M3.5 1.5V13M3.5 6L8 8.5"),
    glyph!("fx-k-20", mirrored, "M1 5H9M5.5 1.5V8C5.5 10.5 4.5 12 2.5 13"),
    glyph!("fx-k-21", mirrored, "M2 2.5H8C7 7 4.5 10.5 1.5 12.5M3.5 6.5L8.5 12"),
    glyph!("fx-k-22", mirrored, "M5 1.5V3.5M1.5 3.5H8L2 10M5 7V13M6.5 8.5L9 11"),
    glyph!("fx-k-23", mirrored, "M3.5 3L1 11M6 3L9 11"),
    glyph!("fx-k-24", mirrored, "M2 1.5V11.5C2 12.5 2.5 12.8 3.5 12.8H8.5M2 6.5L8 4.5"),
    glyph!("fx-k-25", mirrored, "M1.5 2.5H8.5C8 7.5 5.5 11 2 13"),
    glyph!("fx-k-26", mirrored, "M1 8.5L4 4.5L9 10.5"),
    glyph!("fx-k-27", mirrored, "M1 4.5H9M5 1.5V13L4 12M3 7L1.5 10.5M7 7L8.5 10.5"),
    glyph!("fx-k-28", mirrored, "M1.5 2.5H8.5L4.5 8M3.5 7L6.5 12"),
    glyph!("fx-k-29", mirrored, "M2.5 2L7 3.5M2.5 5.5L7 7M1.5 10L7.5 12"),
    glyph!("fx-k-30", mirrored, "M4.5 1.5L1.5 11L8 10M6.5 7.5L8.5 12"),
    glyph!("fx-k-31", mirrored, "M7.5 1.5C7 7 4.5 10.5 1.5 12.5M2.5 5L8.5 11"),
    glyph!("fx-k-32", mirrored, "M2 2.5H8M1 6H9M4.5 2.5V11C4.5 12.5 5 12.8 6 12.8H8.5"),
    glyph!("fx-k-33", mirrored, "M1 5L9 3.5L7 7M3.5 1.5L6 13"),
    glyph!("fx-k-34", mirrored, "M2.5 3H7.5V11.5M1 11.5H9"),
    glyph!("fx-k-35", mirrored, "M1.5 2.5H8.5V12H1.5M2 7.2H8.5"),
    glyph!("fx-k-36", mirrored, "M2.5 1.5H7.5M1.5 4.5H8.5C8 8.5 5.5 11 2.5 13"),
    glyph!("fx-k-37", mirrored, "M2.5 2V8M7.5 1.5V7C7.5 10 6 12 3.5 13"),
    glyph!("fx-k-38", mirrored, "M3.5 2V8C3.5 10 2.5 11.5 1 12.5M6 1.5V12L9 9.5"),
    glyph!("fx-k-39", mirrored, "M2 1.5V12.5C5 11.5 7.5 9.5 9 7"),
    glyph!("fx-k-40", mirrored, "M1.5 2.5H8.5V12H1.5Z"),
    glyph!("fx-k-41", mirrored, "M1.5 2.5V6M1.5 2.5H8.5C8.5 8 6 11 2.5 13"),
];

/// The digits 0–9.
pub const DIGITS: &[IconSvg] = &[
    glyph!("fx-d-0", "M5 1.5C2.5 1.5 1.5 3.5 1.5 7S2.5 12.5 5 12.5 8.5 10.5 8.5 7 7.5 1.5 5 1.5Z"),
    glyph!("fx-d-1", "M3 3.5L5.5 1.5V12.5M2.5 12.5H8"),
    glyph!("fx-d-2", "M2 4C2 2.5 3.3 1.5 5 1.5S8 2.5 8 4C8 6.5 2 9 1.5 12.5H8.5"),
    glyph!("fx-d-3", "M2 2H8L5 6C7 6 8.5 7.2 8.5 9.3S7 12.5 5 12.5 1.5 11.5 1.5 10"),
    glyph!("fx-d-4", "M6.5 12.5V1.5L1.5 9H9"),
    glyph!(
        "fx-d-5",
        "M8 1.5H2.5L2 6.5C3 6 4 5.8 5 5.8 7 5.8 8.5 7.2 8.5 9.2S7 12.5 5 12.5 1.5 11.5 1.5 10.5"
    ),
    glyph!(
        "fx-d-6",
        "M7.5 2C4 2 1.5 4.5 1.5 8.5 1.5 11 3 12.5 5 12.5S8.5 11 8.5 9 7 5.8 5 5.8 2 7 1.5 8.5"
    ),
    glyph!("fx-d-7", "M1.5 1.5H8.5L4 12.5"),
    glyph!(
        "fx-d-8",
        "M5 6.8C3 6.8 2 5.7 2 4.2S3.3 1.5 5 1.5 8 2.7 8 4.2 7 6.8 5 6.8C2.8 6.8 1.5 8 1.5 9.7S3 12.5 5 12.5 8.5 11.4 8.5 9.7 7.2 6.8 5 6.8Z"
    ),
    glyph!(
        "fx-d-9",
        "M8.5 6C8 7.5 6.8 8.5 5 8.5S1.5 7 1.5 5 3 1.5 5 1.5 8.5 3 8.5 6C8.5 10 6.5 12.5 2.5 12.5"
    ),
];

/// A few Latin capitals.
pub const LATIN: &[IconSvg] = &[
    glyph!("fx-l-0", "M1.5 12.5L5 1.5L8.5 12.5M3 8H7"),
    glyph!("fx-l-1", "M8 1.5H2V12.5H8M2 7H7"),
    glyph!("fx-l-2", "M8 1.5H2V12.5M2 7H7"),
    glyph!("fx-l-3", "M2 1.5V12.5M8 1.5V12.5M2 7H8"),
    glyph!("fx-l-4", "M2 1.5V12.5M8 1.5L2 8M4 6L8.5 12.5"),
    glyph!("fx-l-5", "M2 1.5V12.5H8"),
    glyph!("fx-l-6", "M1.5 12.5V1.5L5 8L8.5 1.5V12.5"),
    glyph!("fx-l-7", "M2 12.5V1.5L8 12.5V1.5"),
    glyph!("fx-l-8", "M1 1.5H9M5 1.5V12.5"),
    glyph!("fx-l-9", "M1.5 1.5L8.5 12.5M8.5 1.5L1.5 12.5"),
    glyph!("fx-l-10", "M1.5 1.5H8.5L1.5 12.5H8.5"),
    glyph!("fx-l-11", "M1.5 1.5L5 7L8.5 1.5M5 7V12.5"),
];

/// The holographic signs' glyphs: kanji-like shapes and abstract signs (not real words).
pub const HOLO: &[IconSvg] = &[
    glyph!("fx-h-0", "M2 2H8V12H2Z M2 7H8"),
    glyph!("fx-h-1", "M2 2H8V12H2Z M2 7H8 M5 2V12"),
    glyph!("fx-h-2", "M1.5 4V12H8.5V4 M5 1.5V12"),
    glyph!("fx-h-3", "M1.5 2H8.5 M5 2V12 M1 12H9"),
    glyph!("fx-h-4", "M1 5H9 M5 1.5V5C5 8 3.5 10.5 1 12.5 M5 5C5 8 6.5 10.5 9 12.5"),
    glyph!("fx-h-5", "M1 4.5H9 M5 1V13 M5 4.5L1.5 10 M5 4.5L8.5 10"),
    glyph!("fx-h-6", "M2 4H8V9H2Z M5 1V13"),
    glyph!("fx-h-7", "M1.5 2H8.5 M2.5 7H7.5 M1 12H9 M5 2V12"),
    glyph!("fx-h-8", "M1.5 13V1.5H4.5V6H1.5 M8.5 13V1.5H5.5V6H8.5"),
    glyph!(
        "fx-h-9",
        "M5 1.5V7C5 9.5 3.5 11.5 1.5 13 M5 7C5 9.5 6.5 11.5 8.5 13 M2 4L3 6.5 M8 4L7 6.5"
    ),
    glyph!("fx-h-10", "M5 1V12.5L3.5 11.5 M1 5H4L1.5 10 M9 4L6 7L9 12"),
    glyph!("fx-h-11", "M1 3H9 M3 3V7H7V3 M1 9.5H9 M5 7V13"),
];

/// Every glyph the module contributes.
pub fn all() -> impl Iterator<Item = &'static IconSvg> {
    KATAKANA.iter().chain(DIGITS).chain(LATIN).chain(HOLO)
}

/// The glyphs of a Matrix style (`katakana`: katakana and digits, as in the film).
pub fn of_style(style: &str) -> Vec<&'static str> {
    let names = |set: &'static [IconSvg]| set.iter().map(|g| g.name);
    match style {
        "digits" => names(DIGITS).collect(),
        "binary" => names(&DIGITS[..2]).collect(),
        "latin" => names(LATIN).collect(),
        _ => names(KATAKANA).chain(names(DIGITS)).collect(),
    }
}
