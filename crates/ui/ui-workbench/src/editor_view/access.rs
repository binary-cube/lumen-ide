//! The code editor to a screen reader: a text area holding the lines it shows (never the
//! whole file: a long file costs no more than a short one), each where it is on screen,
//! with the primary selection in them. Its own drawing (tokens, line numbers) is silent.

use super::*;

impl EditorView {
    /// The editor's node, made only while a tree is: the shown lines as its value, their
    /// boxes, the primary selection (a caret alone when its anchor is scrolled away, none
    /// when its head is) and the caret's box.
    pub(super) fn described(
        &mut self,
        f: &Frame<'_>,
        folds: &Folds,
        text: &mut TextSystem,
        palette: &Palette,
    ) -> ui_kit::access::Described {
        use ui_kit::access::{Described, Role, TextLines};
        let buffer = f.doc.buffer();
        let primary = f.doc.selections().primary();
        let (mut anchor, mut focus) = (None, None);
        let mut value = String::new();
        let mut lines: Vec<Rect> = Vec::with_capacity(self.lines.len());
        let mut line_text = String::new();
        let mut told: Option<u32> = None;
        for (i, &line) in self.lines.iter().enumerate() {
            // A wrapped line is told once: its rows' room is its line's.
            if told == Some(line) {
                if let Some(last) = lines.last_mut() {
                    let bottom = f.row_top(i) + f.layout.line_height;
                    last.size.height = bottom - last.min_y();
                }
                continue;
            }
            told = Some(line);
            let line = line as usize;
            if i > 0 {
                value.push('\n');
            }
            let start = buffer.line_start(line);
            buffer.write_line(line, &mut line_text);
            let end = start + line_text.len();
            let at = |offset: usize| {
                (start..=end).contains(&offset).then(|| value.len() + offset - start)
            };
            anchor = anchor.or_else(|| at(primary.anchor));
            focus = focus.or_else(|| at(primary.head));
            value.push_str(&line_text);
            lines.push(Rect::new(
                f.text_area.min_x(),
                f.row_top(i),
                f.text_area.size.width,
                f.layout.line_height,
            ));
        }
        let caret = focus.map(|_| self.caret_rect(f.doc, folds, text, palette));
        let focus_at = focus.unwrap_or(0);
        let lines = TextLines { lines, anchor: anchor.unwrap_or(focus_at), focus: focus_at, caret };
        let mut node = Described::new(Role::TextArea, i18n::t("access.code_editor"), f.bounds)
            .value(value)
            .lines(lines)
            .focused(f.focused);
        // Its text is the file's: set whole by a screen reader it would replace the file.
        node.actions.set_value = false;
        node
    }
}
