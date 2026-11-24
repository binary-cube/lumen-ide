//! A theme's decorations as it declares them: layers whose options follow something, drawn
//! under a condition, combined with what the theme extends and resolved into what's drawn.

use kernel::Rgba;

use super::{Decorations, FieldKind, FieldValue, LayerSpec, MAX_LAYERS, valid_name};

/// What a layer's option follows instead of its own value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindTo {
    /// A name of the theme's palette (`"$name"`): a theme option may change it.
    Palette(String),
    /// A theme option's value (`"@id"`).
    Option(String),
}

/// One of a layer's options following something: the option, which of its colors (a
/// list's; 0 otherwise), and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub key: &'static str,
    pub index: usize,
    pub to: BindTo,
}

/// When a layer is drawn: a theme option on (`"id"`), off (`"!id"`), or a choice
/// (`"id=value"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub option: String,
    pub value: Option<String>,
    pub negate: bool,
}

impl Condition {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (negate, s) = match s.strip_prefix('!') {
            Some(rest) => (true, rest.trim()),
            None => (false, s),
        };
        let (option, value) = match s.split_once('=') {
            Some((o, v)) => (o.trim(), Some(v.trim().to_owned())),
            None => (s, None),
        };
        (valid_name(option) && value.as_deref().is_none_or(valid_name)).then(|| Self {
            option: option.to_owned(),
            value,
            negate,
        })
    }

    /// As written in a theme.
    pub fn text(&self) -> String {
        let not = if self.negate { "!" } else { "" };
        match &self.value {
            Some(v) => format!("{not}{}={v}", self.option),
            None => format!("{not}{}", self.option),
        }
    }
}

/// A layer as a theme declares it: its values (with its defaults for what it doesn't say),
/// when it's drawn and what of it follows the theme's palette or options.
#[derive(Debug, Clone, PartialEq)]
pub struct DecorationLayer {
    pub spec: LayerSpec,
    pub when: Option<Condition>,
    pub bindings: Vec<Binding>,
    /// Drawn (`enabled = false` in a theme: kept, its options too, but not drawn — the
    /// studio's switch).
    pub enabled: bool,
}

impl DecorationLayer {
    /// A layer with only its own values.
    pub fn plain(spec: LayerSpec) -> Self {
        Self { spec, when: None, bindings: Vec::new(), enabled: true }
    }

    /// What option `key`'s color `index` follows.
    pub fn binding(&self, key: &str, index: usize) -> Option<&BindTo> {
        self.bindings.iter().find(|b| b.key == key && b.index == index).map(|b| &b.to)
    }

    /// Option `key` set to a value of its own: it follows nothing any more.
    pub fn unbind(&mut self, key: &str) {
        self.bindings.retain(|b| b.key != key);
    }

    /// Option `key`'s color `index` set to a value of its own.
    pub fn unbind_at(&mut self, key: &str, index: usize) {
        self.bindings.retain(|b| b.key != key || b.index != index);
    }
}

/// How a theme's decorations combine with those of the theme it extends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Merge {
    /// It declares none: the extended theme's.
    #[default]
    Inherit,
    /// The first format's sections (`[decorations.garland]`…): each kind it declares
    /// replaces the extended theme's of that kind, the rest stay.
    ByKind,
    /// A list of layers: it replaces the extended theme's whole.
    Replace,
}

/// What a theme says of its decorations.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DecorationsSpec {
    pub layers: Vec<DecorationLayer>,
    pub merge: Merge,
}

/// What a theme's layers are resolved with: its options' values, the names of its palette
/// they change, and whether it's dark.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolveWith<'a> {
    pub values: &'a [(String, crate::options::OptionValue)],
    pub palette: &'a [(String, Rgba)],
    pub dark: bool,
}

impl DecorationsSpec {
    /// A list of plain layers (replacing what a theme extends).
    pub fn of(layers: impl IntoIterator<Item = LayerSpec>) -> Self {
        Self {
            layers: layers.into_iter().map(DecorationLayer::plain).collect(),
            merge: Merge::Replace,
        }
    }

    /// `self` over `under` (see [`Merge`]).
    pub fn over(self, under: Self) -> Self {
        match self.merge {
            Merge::Inherit => under,
            Merge::Replace => self,
            Merge::ByKind => {
                let mut layers = under.layers;
                for layer in self.layers {
                    let kind = layer.spec.kind();
                    match layers.iter().position(|l| l.spec.kind() == kind) {
                        Some(at) => layers[at] = layer,
                        None => layers.push(layer),
                    }
                }
                Self { layers, merge: Merge::Replace }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The decorations to draw: each layer whose condition holds, what it follows applied,
    /// limits kept.
    pub fn resolve(self, with: ResolveWith<'_>) -> Decorations {
        use crate::options::OptionValue;
        let value = |id: &str| with.values.iter().find(|(k, _)| k == id).map(|(_, v)| v);
        let layers = self
            .layers
            .into_iter()
            .filter(|layer| {
                if !layer.enabled {
                    return false;
                }
                let Some(c) = &layer.when else { return true };
                let holds = match (value(&c.option), &c.value) {
                    // An option the theme doesn't have: drawn (its file got a note).
                    (None, _) => return true,
                    (Some(OptionValue::Bool(b)), None) => *b,
                    (Some(OptionValue::Choice(chosen)), Some(v)) => chosen == v,
                    (Some(OptionValue::Choice(_)), None) => true,
                    (Some(OptionValue::Number(n)), None) => *n > 0.0,
                    (Some(_), Some(_)) => false,
                    (Some(OptionValue::Color(_)), None) => true,
                };
                holds != c.negate
            })
            .take(MAX_LAYERS)
            .map(|layer| {
                let mut spec = layer.spec;
                for b in &layer.bindings {
                    match &b.to {
                        BindTo::Palette(name) => {
                            if let Some((_, c)) = with.palette.iter().find(|(n, _)| n == name) {
                                spec.set_color_at(b.key, b.index, *c);
                            }
                        }
                        BindTo::Option(id) => match value(id) {
                            Some(OptionValue::Number(n)) => {
                                let max = match spec.kind().field(b.key).map(|f| f.kind) {
                                    Some(FieldKind::Amount(a)) => a.max(),
                                    _ => 1.0,
                                };
                                let _ = spec.set(b.key, &FieldValue::Number(n.clamp(0.0, max)));
                            }
                            Some(OptionValue::Color(c)) => {
                                spec.set_color_at(b.key, b.index, *c);
                            }
                            Some(OptionValue::Bool(on)) => {
                                let _ = spec
                                    .set(b.key, &FieldValue::Number(if *on { 1.0 } else { 0.0 }));
                            }
                            _ => {}
                        },
                    }
                }
                spec.clamped()
            })
            .collect();
        Decorations { layers, dark: with.dark }
    }
}
