//! Plural rules (CLDR) and number formatting, per language.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl PluralCategory {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "zero" => Self::Zero,
            "one" => Self::One,
            "two" => Self::Two,
            "few" => Self::Few,
            "many" => Self::Many,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

/// Languages with known rules (the rest use the English rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    Ro,
    En,
}

impl Locale {
    pub fn from_tag(tag: &str) -> Self {
        match tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase().as_str() {
            "ro" => Self::Ro,
            _ => Self::En,
        }
    }

    pub fn plural(self, n: f64) -> PluralCategory {
        let integer = n.fract() == 0.0;
        let i = n.abs().trunc() as u64;
        match self {
            // CLDR: one: i = 1 and v = 0; few: v ≠ 0 or n = 0 or n ≠ 1 and n % 100 = 1..19.
            Self::Ro => {
                if integer && i == 1 {
                    PluralCategory::One
                } else if !integer || i == 0 || (1..=19).contains(&(i % 100)) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Other
                }
            }
            Self::En => {
                if integer && i == 1 {
                    PluralCategory::One
                } else {
                    PluralCategory::Other
                }
            }
        }
    }

    /// The number, with the language's decimal separator (`1,5` in Romanian).
    pub fn format_number(self, n: f64) -> String {
        let text = if n.fract() == 0.0 && n.abs() < 1e15 {
            format!("{}", n as i64)
        } else {
            let s = format!("{n:.2}");
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        match self {
            Self::Ro => text.replace('.', ","),
            Self::En => text,
        }
    }
}
