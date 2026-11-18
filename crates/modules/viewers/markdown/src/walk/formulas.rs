//! Formulas (inline and as blocks, typeset once each) and the diagrams' numbering.

use super::*;

impl Walk<'_> {
    /// A formula set (cached by what it's made of): `display` for a block's.
    pub(super) fn formula(&mut self, tex: &str, display: bool) -> Option<crate::math::Typeset> {
        let s = self.b.s;
        let palette = self.b.palette;
        let size = (self.b.body / s) * if display { 1.2 } else { 1.05 };
        let (color, error) = (palette.text, palette.danger);
        let key = crate::jobs::key(&(
            tex,
            display,
            size.to_bits(),
            s.to_bits(),
            format!("{color:?}{error:?}"),
        ));
        // The math font, while the document has formulas.
        let font = self.caches.math_text.get_or_insert_with(crate::math::math_text).clone();
        self.uses_math = true;
        let source = tex.to_owned();
        let t = self.caches.math.get(key, move || {
            let mut text = font.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            crate::math::typeset(&source, display, size, color, error, &mut text, s)
        })?;
        // Its masks freed out of sight (the budget, the renderer): set again.
        if t.as_ref().is_ok_and(|f| f.masks.iter().any(|(i, _)| i.is_lost())) {
            self.caches.math.remove(key);
            return None;
        }
        Some(t)
    }

    /// `$…$` in the text.
    pub(super) fn inline_math(&mut self, tex: &str) {
        match self.formula(tex, false) {
            // Its source is what it copies.
            Some(Ok(f)) => self.spans.push(Span {
                math: Some(f),
                ..Span::text(format!("${tex}$"), self.style, self.link.clone())
            }),
            // Being set: its source meanwhile; one that can't be: its source, as code.
            _ => {
                let style = Style { code: true, ..self.style };
                self.push(&format!("${tex}$"), style);
            }
        }
    }

    /// `$$…$$` or a ```` ```math ```` block: centered on its own line.
    pub(super) fn display_math(&mut self, tex: &str) {
        // The text before it, as its own lines.
        if !self.spans.is_empty() {
            let (x0, color) = (self.indent(), self.text_color());
            self.flow_body(x0, color);
        }
        let x0 = self.indent();
        match self.formula(tex.trim(), true) {
            Some(Ok(f)) => {
                let s = self.b.s;
                let room = self.b.right - x0;
                let w = (f.scene.width * s).round();
                let x = x0 + ((room - w) / 2.0).max(0.0);
                self.b.y += self.b.px(6.0);
                let at = kernel::Point::new(x, self.b.y);
                self.b.formula(&f, at);
                let h = (f.scene.height * s).round();
                self.b.whole(Rect::new(x, self.b.y, w, h), format!("$${}$$", tex.trim()));
                self.b.y += (f.scene.height * s).round() + self.b.px(6.0);
            }
            None => {
                // Being set: a line of room meanwhile.
                self.b.y += (self.b.body * 2.0).round();
            }
            Some(Err(_)) => {
                let style = Style { code: true, ..Style::default() };
                let spans = [Span::text(format!("$${}$$", tex.trim()), style, None)];
                let (size, right, danger) = (self.b.body, self.b.right, self.b.palette.danger);
                self.b.flow(
                    &spans,
                    Run {
                        x0,
                        x1: right,
                        size,
                        weight: Font::NORMAL,
                        color: danger,
                        line: 1.6,
                        align: Align::Center,
                    },
                );
            }
        }
    }

    /// The next diagram's place among the document's.
    pub fn diagram_ordinal(&mut self) -> usize {
        self.diagrams += 1;
        self.diagrams - 1
    }
}
