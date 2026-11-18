//! Images, rules, footnotes and the table of contents.

use super::*;

impl Walk<'_> {
    pub(super) fn image_end(&mut self) {
        let Some((src, alt)) = self.image.take() else { return };
        self.image(&src, &alt, None, None);
    }

    /// Image `src` set in the text (`alt` while it's read, or if it can't be), at most
    /// `width` × `height` (logical pixels) when the HTML says so.
    pub(super) fn image(&mut self, src: &str, alt: &str, width: Option<f32>, height: Option<f32>) {
        let (probe, path, stamp) = self.caches.images.size(self.dir, src);
        let (w, h) = match probe {
            crate::layout::Probe::Size(w, h) => (w, h),
            // Why a web image isn't there, beside its name.
            crate::layout::Probe::Failed(why) => {
                let name = if alt.is_empty() { src } else { alt };
                let text = format!(
                    "🖼 {name} — {}",
                    i18n::tr!("markdown.image_failed", why = why.as_str())
                );
                let style = Style { italic: true, strike: false, ..self.style };
                self.spans.push(Span::text(text, style, Some(src.to_owned())));
                return;
            }
            _ => {
                let text = format!("🖼 {}", if alt.is_empty() { src } else { alt });
                let style = Style { italic: true, ..self.style };
                self.spans.push(Span::text(text, style, self.link.clone()));
                return;
            }
        };
        let s = self.b.s;
        let (mut w, mut h) = (w as f32, h as f32);
        match (width, height) {
            (Some(width), _) if w > 0.0 => {
                h *= width / w;
                w = width;
            }
            (None, Some(height)) if h > 0.0 => {
                w *= height / h;
                h = height;
            }
            _ => {}
        }
        let (w, h) = (w * s, h * s);
        let max = self.b.right - self.indent();
        let k = (max / w).min(1.0);
        let width = (w * k).round();
        let decode = crate::layout::decode_width(width);
        let image = InlineImage { path, stamp, decode, width, height: (h * k).round() };
        self.spans.push(Span {
            text: String::new(),
            style: self.style,
            link: self.link.clone(),
            image: Some(image),
            math: None,
        });
    }

    pub(super) fn rule(&mut self) {
        self.flush_paragraph(false);
        self.b.y += self.b.px(8.0);
        let (x0, right) = (self.indent(), self.b.right);
        self.b.rule(x0, right);
        self.b.y += self.gap() + self.b.px(8.0);
    }

    pub(super) fn footnote_reference(&mut self, label: &str) {
        let n = self.outline.footnotes.get(label).copied().unwrap_or(0);
        let y = self.b.y;
        self.b.anchors.push((format!("fnref-{label}"), y));
        let style = Style { sup: true, link: true, ..self.style };
        self.spans.push(Span::text(n.to_string(), style, Some(format!("#fn-{label}"))));
    }

    /// The footnotes' definitions, numbered as they were referenced, under a rule at the
    /// end; each ends with a link back to where it was referenced.
    pub(super) fn footnotes(&mut self) {
        if self.definitions.is_empty() {
            return;
        }
        let mut definitions = std::mem::take(&mut self.definitions);
        definitions.sort_by_key(|(label, _)| self.outline.footnotes.get(label).copied());
        self.b.y += self.b.px(8.0);
        let (x0, right) = (self.b.left, self.b.right);
        self.b.rule(x0, right);
        self.b.y += self.gap();
        self.small = true;
        for (label, mut events) in definitions {
            let n = self.outline.footnotes.get(&label).copied().unwrap_or(0);
            self.b.anchors.push((format!("fn-{label}"), self.b.y));
            // The way back, at the end of its last paragraph.
            let back = vec![
                (
                    Event::Start(Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url: format!("#fnref-{label}").into(),
                        title: "".into(),
                        id: "".into(),
                    }),
                    0..0,
                ),
                (Event::Text(" ↩".into()), 0..0),
                (Event::End(TagEnd::Link), 0..0),
            ];
            let last = events.iter().rposition(|(e, _)| matches!(e, Event::End(TagEnd::Paragraph)));
            match last {
                Some(i) => {
                    events.splice(i..i, back);
                }
                None => events.extend(back),
            }
            self.lists.push(ListState { next: Some(n as u64) });
            self.item_start();
            for (event, range) in events {
                self.event(event, range);
            }
            self.flush_item();
            self.lists.pop();
        }
        self.small = false;
    }

    /// The paragraph holds only a table of contents' marker (`[TOC]`, `[[_TOC_]]`).
    pub(super) fn is_toc(&self) -> bool {
        let text: String = self.spans.iter().map(|s| s.text.as_str()).collect();
        let text = text.trim();
        let marker = ["[TOC]", "[toc]", "[[_TOC_]]", "[[toc]]"].contains(&text);
        let wikilink = matches!(text, "_TOC_" | "toc")
            && self
                .spans
                .iter()
                .all(|s| s.link.as_deref().is_some_and(|l| l.contains("_TOC_") || l == "toc.md"));
        marker || wikilink
    }

    /// The document's headings as links, indented by level.
    pub(super) fn toc(&mut self) {
        let headings: Vec<(u8, String, String)> = self
            .outline
            .headings
            .iter()
            .map(|h| (h.level, h.text.clone(), h.anchor.clone()))
            .collect();
        let Some(top) = headings.iter().map(|h| h.0).min() else { return };
        let x0 = self.indent();
        for (level, text, anchor) in headings {
            let style = Style { link: true, ..Style::default() };
            let spans = [Span::text(text, style, Some(format!("#{anchor}")))];
            let indent = f32::from(level - top) * self.b.px(20.0);
            let (size, right) = (self.b.body, self.b.right);
            self.b.flow(
                &spans,
                Run {
                    x0: x0 + indent,
                    x1: right,
                    size,
                    weight: Font::NORMAL,
                    color: self.b.palette.text,
                    line: 1.5,
                    align: Align::Left,
                },
            );
        }
        self.b.y += self.gap();
    }
}
