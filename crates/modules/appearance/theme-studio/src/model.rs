//! What the studio holds, apart from how it's drawn: its categories (the navigation's), the
//! groups each shows, every setting it has ([`catalog`]: what the search looks through and
//! the changes are counted on), whether a setting differs from the theme it started from
//! (its "↺", the dot on its category, the number at the top) and the search (any case,
//! with or without diacritics).

use super::*;
use rows::{BackdropRow, CaretRow, FloatingRow};

/// The studio's categories, in the navigation's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Category {
    #[default]
    General,
    Interface,
    Syntax,
    Caret,
    Effects,
    /// The theme's own settings (only for a theme that has some).
    Options,
}

impl Category {
    pub(crate) const ALL: [Self; 6] =
        [Self::General, Self::Interface, Self::Syntax, Self::Caret, Self::Effects, Self::Options];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Interface => "interface",
            Self::Syntax => "syntax",
            Self::Caret => "caret",
            Self::Effects => "effects",
            Self::Options => "options",
        }
    }

    pub(crate) fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::General => "tag",
            Self::Interface => "palette",
            Self::Syntax => "braces",
            Self::Caret => "text-cursor",
            Self::Effects => "sparkles",
            Self::Options => "gear",
        }
    }

    pub(crate) fn label(self) -> String {
        t(&format!("studio.cat_{}", self.id()))
    }

    /// One line under its title.
    pub(crate) fn detail(self) -> String {
        t(&format!("studio.cat_{}_detail", self.id()))
    }

    /// The categories a draft has (its own settings only when it has some).
    pub(crate) fn shown(draft: &ThemeDraft) -> impl Iterator<Item = Self> + '_ {
        let has_effects =
            workbench_model::presence::request::<contracts::decorations::SetOwnDecorations>();
        Self::ALL.into_iter().filter(move |c| match c {
            Self::Options => !draft.options.is_empty(),
            Self::Effects => has_effects,
            _ => true,
        })
    }
}

/// A group of colors (a section of a category: its id names it, `studio.group_<id>`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TokenGroup {
    pub id: &'static str,
    pub keys: &'static [&'static str],
}

/// The interface's colors, by group (the state colors only where the theme has them).
pub(crate) const INTERFACE: &[TokenGroup] = &[
    TokenGroup { id: "backgrounds", keys: &["base", "card", "panel"] },
    TokenGroup { id: "text", keys: &["fg", "muted", "code"] },
    TokenGroup { id: "accent", keys: &["accent"] },
    TokenGroup { id: "states", keys: &["red", "yellow", "green"] },
    TokenGroup { id: "borders", keys: &["line"] },
];

/// The syntax's colors, by group.
pub(crate) const SYNTAX: &[TokenGroup] = &[
    TokenGroup { id: "keywords", keys: &["syntax.keyword"] },
    TokenGroup { id: "literals", keys: &["syntax.string", "syntax.number"] },
    TokenGroup { id: "comments", keys: &["syntax.comment"] },
    TokenGroup { id: "types", keys: &["syntax.type", "syntax.function"] },
    TokenGroup { id: "variables", keys: &["syntax.variable", "syntax.property"] },
];

/// The caret's rows, by group.
pub(crate) const CARET: &[(&str, &[CaretRow])] = &[
    ("shape", &[CaretRow::Shape, CaretRow::Width]),
    ("animation", &[CaretRow::Blink, CaretRow::Motion, CaretRow::Glow, CaretRow::Trail]),
];

/// The settings of the effects that aren't the theme's but the decorations' (the same for
/// every theme: saved at once, not with the theme).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Global {
    /// Animated, still or off.
    Mode,
    /// Still after this long without input.
    Idle,
    /// Still when the system asks for less motion (or saves power).
    ReduceMotion,
    /// The most frames a second.
    Fps,
}

impl Global {
    pub(crate) const ALL: [Self; 4] = [Self::Mode, Self::Idle, Self::ReduceMotion, Self::Fps];

    /// Its key in the decorations' settings block.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Mode => "mode",
            Self::Idle => "idle",
            Self::ReduceMotion => "reduce_motion",
            Self::Fps => "fps",
        }
    }
}

/// A setting of the studio (what a row is, whatever draws it).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SettingId {
    Name,
    Base,
    /// A color: an interface's (`base`) or the syntax's (`syntax.string`).
    Token(&'static str),
    Caret(CaretRow),
    /// The backdrop's color.
    BackdropColor,
    Backdrop(BackdropRow),
    /// The floating panels' glass.
    Floating(FloatingRow),
    /// An effect (a layer of decorations), with every option it has.
    Layer(usize),
    /// The theme's own setting `n`.
    Option(usize),
    Global(Global),
}

/// A setting as the search finds it: where it is, and what it's called.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub id: SettingId,
    pub category: Category,
    /// Its group's name.
    pub group: String,
    pub label: String,
    /// More words it's found by (its description, its choices).
    pub words: String,
}

/// The name of a color (`studio.token.<key>`, the syntax's without `syntax.`).
pub(crate) fn token_name(key: &str) -> String {
    t(&format!("studio.token.{}", key.trim_start_matches("syntax.")))
}

/// A color's description (`studio.token_detail.<key>`).
pub(crate) fn token_detail(key: &str) -> String {
    t(&format!("studio.token_detail.{}", key.trim_start_matches("syntax.")))
}

/// A group's name (`studio.group_<id>`).
pub(crate) fn group_name(id: &str) -> String {
    t(&format!("studio.group_{id}"))
}

/// The keys a group shows for a draft (those it has).
pub(crate) fn keys_of<'a>(
    group: &'a TokenGroup,
    draft: &'a ThemeDraft,
) -> impl Iterator<Item = &'static str> + 'a {
    group.keys.iter().copied().filter(|k| draft.get(k).is_some())
}

/// Every setting the studio shows for `draft` (`effects`: the decorations' own settings are
/// there to show).
pub(crate) fn catalog(draft: &ThemeDraft, input: &StudioInput, effects: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut push = |id, category, group: String, label: String, words: String| {
        out.push(Entry { id, category, group, label, words });
    };
    let general = group_name("theme");
    push(SettingId::Name, Category::General, general.clone(), t("studio.name"), String::new());
    let names: Vec<&str> = input.themes.iter().map(|th| th.name.as_str()).collect();
    push(SettingId::Base, Category::General, general, t("studio.start_from"), names.join(" "));
    for (category, groups) in [(Category::Interface, INTERFACE), (Category::Syntax, SYNTAX)] {
        for group in groups {
            for key in keys_of(group, draft) {
                push(
                    SettingId::Token(key),
                    category,
                    group_name(group.id),
                    token_name(key),
                    token_detail(key),
                );
            }
        }
    }
    for (group, caret_rows) in CARET {
        for row in *caret_rows {
            let r = Row::Caret(*row);
            push(
                SettingId::Caret(*row),
                Category::Caret,
                group_name(group),
                r.label(draft),
                r.choices(draft).join(" "),
            );
        }
    }
    let backdrop = group_name("backdrop");
    push(
        SettingId::BackdropColor,
        Category::Interface,
        backdrop.clone(),
        t("studio.backdrop_color"),
        t("studio.backdrop_color_detail"),
    );
    for row in BackdropRow::ALL {
        let r = Row::Backdrop(row);
        push(
            SettingId::Backdrop(row),
            Category::Interface,
            backdrop.clone(),
            r.label(draft),
            r.choices(draft).join(" "),
        );
    }
    let floating = group_name("floating");
    for row in FloatingRow::ALL {
        let r = Row::Floating(row);
        push(
            SettingId::Floating(row),
            Category::Interface,
            floating.clone(),
            r.label(draft),
            r.choices(draft).join(" "),
        );
    }
    for (i, layer) in draft.decorations.layers.iter().enumerate() {
        let kind = layer.spec.kind();
        let words: Vec<String> =
            kind.fields().iter().map(|f| t(&format!("studio.field_{}", f.key))).collect();
        push(
            SettingId::Layer(i),
            Category::Effects,
            group_name("active"),
            rows::kind_label(kind),
            words.join(" "),
        );
    }
    if effects {
        for g in Global::ALL {
            push(
                SettingId::Global(g),
                Category::Effects,
                group_name("performance"),
                t(&format!("studio.global_{}", g.key())),
                t(&format!("studio.global_{}_detail", g.key())),
            );
        }
    }
    let locale = i18n::locale();
    for (n, o) in draft.options.iter().enumerate() {
        push(
            SettingId::Option(n),
            Category::Options,
            group_name("options"),
            o.label.get(&locale).to_owned(),
            String::new(),
        );
    }
    out
}

/// Whether setting `id` differs in `draft` from the theme it started from (`base`).
pub(crate) fn modified(draft: &ThemeDraft, base: &ThemeDraft, id: &SettingId) -> bool {
    match id {
        SettingId::Name | SettingId::Base | SettingId::Global(_) => false,
        SettingId::Token(key) => draft.get(key) != base.get(key),
        SettingId::BackdropColor => {
            let key = theming::studio::BACKDROP_COLOR;
            draft.get(key) != base.get(key)
        }
        SettingId::Backdrop(row) => rows::differs(Row::Backdrop(*row), draft, base),
        SettingId::Floating(row) => rows::differs(Row::Floating(*row), draft, base),
        SettingId::Caret(row) => {
            let r = Row::Caret(*row);
            r.current(draft) != r.current(base) || r.said(draft) != r.said(base)
        }
        SettingId::Layer(i) => draft.decorations.layers.get(*i) != base.decorations.layers.get(*i),
        SettingId::Option(n) => {
            draft.options.get(*n).map(|o| &o.default) != base.options.get(*n).map(|o| &o.default)
        }
    }
}

/// How many changes `draft` has from `base`: each setting changed, and each effect of the
/// base removed.
pub(crate) fn changes(entries: &[Entry], draft: &ThemeDraft, base: &ThemeDraft) -> usize {
    let changed = entries.iter().filter(|e| modified(draft, base, &e.id)).count();
    let removed = base.decorations.layers.len().saturating_sub(draft.decorations.layers.len());
    changed + removed
}

/// Whether category `c` has changes (its dot).
pub(crate) fn category_changed(
    entries: &[Entry],
    draft: &ThemeDraft,
    base: &ThemeDraft,
    c: Category,
) -> bool {
    entries.iter().any(|e| e.category == c && modified(draft, base, &e.id))
        || (c == Category::Effects
            && base.decorations.layers.len() > draft.decorations.layers.len())
}

/// Text as the search compares it: lower case, without diacritics.
pub(crate) fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ă' | 'â' | 'à' | 'á' | 'ä' => 'a',
            'î' | 'í' | 'ï' => 'i',
            'ș' | 'ş' => 's',
            'ț' | 'ţ' => 't',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'ó' | 'ö' | 'ô' => 'o',
            'ú' | 'ü' => 'u',
            other => other,
        })
        .collect()
}

/// The entries every word of `query` is found in (their label, group, category or other
/// words), in the studio's order; nothing for an empty query.
pub(crate) fn search<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_owned).collect();
    if words.is_empty() {
        return Vec::new();
    }
    entries
        .iter()
        .filter(|e| {
            let hay = fold(&format!("{} {} {} {}", e.label, e.group, e.category.label(), e.words));
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/model.rs"]
mod tests;
