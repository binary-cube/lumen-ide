//! How the caret looks and moves: a theme describes it (`[caret]`), the user may choose
//! their own. The editor's carets and the text fields' follow it; the terminal its own
//! shape, the rest the same. Its color is the theme's `editor.cursor`.

/// The caret's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaretShape {
    /// A thin bar before the character.
    #[default]
    Bar,
    /// A block over the character.
    Block,
    /// A line under the character.
    Underline,
}

/// How the caret blinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blink {
    /// It doesn't: always shown.
    Off,
    /// On and off.
    #[default]
    Classic,
    /// It fades out and in.
    Smooth,
}

/// How the caret goes to a new place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Motion {
    /// At once.
    #[default]
    Jump,
    /// It glides there.
    Glide,
}

/// The caret's look, complete.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretLook {
    pub shape: CaretShape,
    /// The bar's (or the underline's) thickness, in logical pixels (1–4).
    pub width: f32,
    pub blink: Blink,
    /// A halo around it (0: none, 1: strong).
    pub glow: f32,
    /// How long the trail it leaves when it moves takes to fade, in milliseconds (0: none).
    pub trail_ms: u32,
    pub motion: Motion,
}

impl Default for CaretLook {
    /// As it always was: a 2 px bar, blinking on and off, no effects.
    fn default() -> Self {
        Self {
            shape: CaretShape::Bar,
            width: 2.0,
            blink: Blink::Classic,
            glow: 0.0,
            trail_ms: 0,
            motion: Motion::Jump,
        }
    }
}

/// What a theme says of the caret (each part optional: the rest from the theme it extends,
/// then [`CaretLook::default`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CaretSpec {
    pub shape: Option<CaretShape>,
    pub width: Option<f32>,
    pub blink: Option<Blink>,
    pub glow: Option<f32>,
    pub trail_ms: Option<u32>,
    pub motion: Option<Motion>,
}

impl CaretSpec {
    /// `self` over `under` (what `self` doesn't say, `under` does).
    pub fn over(self, under: Self) -> Self {
        Self {
            shape: self.shape.or(under.shape),
            width: self.width.or(under.width),
            blink: self.blink.or(under.blink),
            glow: self.glow.or(under.glow),
            trail_ms: self.trail_ms.or(under.trail_ms),
            motion: self.motion.or(under.motion),
        }
    }

    /// The complete look (limits kept).
    pub fn look(self) -> CaretLook {
        let base = CaretLook::default();
        CaretLook {
            shape: self.shape.unwrap_or(base.shape),
            width: self.width.unwrap_or(base.width).clamp(1.0, 4.0),
            blink: self.blink.unwrap_or(base.blink),
            glow: self.glow.unwrap_or(base.glow).clamp(0.0, 1.0),
            trail_ms: self.trail_ms.unwrap_or(base.trail_ms).min(1000),
            motion: self.motion.unwrap_or(base.motion),
        }
    }
}

impl CaretShape {
    /// As written in a theme / the settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Block => "block",
            Self::Underline => "underline",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bar" => Self::Bar,
            "block" => Self::Block,
            "underline" => Self::Underline,
            _ => return None,
        })
    }
}

impl Blink {
    /// As written in a theme / the settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "none",
            Self::Classic => "classic",
            Self::Smooth => "smooth",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "none" | "off" => Self::Off,
            "classic" => Self::Classic,
            "smooth" => Self::Smooth,
            _ => return None,
        })
    }
}

impl Motion {
    /// As written in a theme / the settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::Jump => "jump",
            Self::Glide => "glide",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "jump" | "none" => Self::Jump,
            "glide" | "smooth" => Self::Glide,
            _ => return None,
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/caret.rs"]
mod tests;
