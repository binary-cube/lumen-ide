//! HTML inside the Markdown: its tokens, the tags opened and closed, a details' summary.

use super::*;

impl Walk<'_> {
    /// HTML: its tokens, as the elements they stand for.
    pub(super) fn html(&mut self, html: &str) {
        for token in html::tokens(html) {
            if let Some(Skip::Details { nesting }) = &mut self.skip {
                match &token {
                    Token::Open { name, .. } if name == "details" => *nesting += 1,
                    Token::Close(name) if name == "details" => {
                        if *nesting == 0 {
                            self.skip = None;
                            self.details.pop();
                        } else {
                            *nesting -= 1;
                        }
                    }
                    _ => {}
                }
                continue;
            }
            self.html_token(&token);
        }
    }

    pub(super) fn html_token(&mut self, token: &Token) {
        match token {
            Token::Text(text) => {
                // Whitespace collapses, as in a browser.
                let collapsed = collapse(text);
                if collapsed.trim().is_empty() && self.spans.is_empty() {
                    return;
                }
                self.text(&collapsed);
            }
            Token::Open { name, closed, .. } => {
                self.html_open(name, token);
                if *closed {
                    self.html_close(name);
                }
            }
            Token::Close(name) => self.html_close(name),
        }
    }

    pub(super) fn html_open(&mut self, name: &str, token: &Token) {
        match name {
            "br" => self.push("\n", self.style),
            "b" | "strong" => self.style.bold = true,
            "i" | "em" => self.style.italic = true,
            "code" | "tt" | "samp" => self.style.code = true,
            "s" | "del" | "strike" => self.style.strike = true,
            "u" | "ins" => self.style.underline = true,
            "mark" => self.style.mark = true,
            "kbd" => self.style.kbd = true,
            "sup" => self.style.sup = true,
            "sub" => self.style.sub = true,
            "a" => {
                if let Some(href) = token.attr("href") {
                    self.link = Some(href.to_owned());
                    self.style.link = true;
                }
                if let Some(name) = token.attr("name").or_else(|| token.attr("id")) {
                    let y = self.b.y;
                    self.b.anchors.push((name.to_owned(), y));
                }
            }
            "img" => {
                let src = token.attr("src").unwrap_or_default();
                let alt = token.attr("alt").unwrap_or_default();
                let number = |attr: &str| {
                    token.attr(attr).and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
                };
                self.image(src, alt, number("width"), number("height"));
            }
            "p" | "div" | "center" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush_paragraph(name == "p");
                let align = match token.attr("align").map(str::to_ascii_lowercase).as_deref() {
                    Some("center") => Align::Center,
                    Some("right") => Align::Right,
                    _ if name == "center" => Align::Center,
                    _ => self.align(),
                };
                self.align.push(align);
                if let Some(level) = name.strip_prefix('h').and_then(|l| l.parse::<u8>().ok()) {
                    self.block = Block::Heading(level.clamp(1, 6));
                    if self.b.y > self.b.px(30.0) {
                        self.b.y += self.b.px(8.0);
                    }
                }
            }
            "hr" => self.rule(),
            "details" => {
                self.flush_paragraph(false);
                let index = self.folds;
                self.folds += 1;
                let open = token.attr("open").is_some() != self.toggled.contains(&index);
                self.details.push(open);
                self.summary = Some((index, open));
            }
            "summary" => self.flush_paragraph(false),
            "ul" | "ol" => {
                self.flush_paragraph(false);
                let start = (name == "ol")
                    .then(|| token.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1));
                self.lists.push(ListState { next: start });
            }
            "li" => self.item_start(),
            "blockquote" => self.quote_start(None, 0..0),
            "pre" => self.style.code = true,
            _ => {}
        }
    }

    pub(super) fn html_close(&mut self, name: &str) {
        match name {
            "b" | "strong" => self.style.bold = false,
            "i" | "em" => self.style.italic = false,
            "code" | "tt" | "samp" | "pre" => self.style.code = false,
            "s" | "del" | "strike" => self.style.strike = false,
            "u" | "ins" => self.style.underline = false,
            "mark" => self.style.mark = false,
            "kbd" => self.style.kbd = false,
            "sup" => self.style.sup = false,
            "sub" => self.style.sub = false,
            "a" => {
                self.link = None;
                self.style.link = false;
            }
            "p" | "div" | "center" => {
                self.flush_paragraph(name == "p");
                self.align.pop();
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if matches!(self.block, Block::Heading(_)) {
                    self.heading_end();
                }
                self.align.pop();
            }
            "summary" => self.summary_end(),
            "details" => {
                self.flush_paragraph(true);
                self.summary = None;
                self.details.pop();
            }
            "ul" | "ol" => {
                self.flush_item();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.b.y += self.gap();
                }
            }
            "li" => self.flush_item(),
            "blockquote" => self.quote_end(),
            "tr" => self.push("\n", self.style),
            "td" | "th" => self.push("  ", self.style),
            _ => {}
        }
    }

    /// A `<summary>` read: its line with the fold's arrow (clicking it opens or closes
    /// its `<details>`); a closed one's content is left out.
    pub(super) fn summary_end(&mut self) {
        let Some((index, open)) = self.summary.take() else {
            self.flush_paragraph(false);
            return;
        };
        let arrow = if open { "▾ " } else { "▸ " };
        let style = Style { bold: true, ..Style::default() };
        self.spans.insert(0, Span::text(arrow, Style { bold: false, ..style }, None));
        for span in &mut self.spans[1..] {
            span.style.bold = true;
        }
        let top = self.b.y;
        let x0 = self.indent();
        let color = self.text_color();
        self.flow_body(x0, color);
        let right = self.b.right;
        self.b.hits.push((Rect::new(x0, top, right - x0, self.b.y - top), Hit::Fold(index)));
        self.b.y += self.b.px(4.0);
        if !open {
            self.skip = Some(Skip::Details { nesting: 0 });
        }
    }
}
