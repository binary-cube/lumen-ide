//! The editor drawn: the request on the left (its method, URL and Send, its tabs and what
//! the chosen one holds), its latest response on the right; each part's area kept for the
//! clicks ([`Hit`]).

use i18n::t;
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ds::{
    self, ButtonKind, ButtonSpec, Check, Ctx, DropdownSpec, Tone, button_size,
};
use ui_components::tab_strip::{self, UnderlineTab};
use ui_components::{MultilineSpec, Viewer, ViewerPaint};
use ui_kit::{TextInputStyle, draw_text};

use super::form::{AuthKind, BodyKind, Row, Table};
use super::{Area, Focus, Hit, RequestEditor, Tab};
use crate::response_view::Part;

const TOP: f32 = 48.0;
/// The form's tabs: the room each side of a label (logical pixels) while they fit, closer
/// together when they don't, then smaller; the gap between them while they fit.
const TAB_PAD: f32 = 10.0;
const TAB_PAD_CLOSE: f32 = 6.0;
const TAB_PAD_SMALL: f32 = 5.0;
const TAB_GAP: f32 = 4.0;
/// Its tabs' row: as high as the response's status row beside it (one line across).
const TABS: f32 = crate::response_view::STATUS;
const ROW: f32 = 34.0;
const PAD: f32 = 14.0;
/// The request's share of the width, at first.
const DEFAULT_SHARE: f32 = 0.56;
/// The room kept for the URL field's × on its right (logical pixels).
const CLEAR_ROOM: f32 = 30.0;

/// A field without a frame (a table's cell, between the table's lines): typed in, its
/// caret shows it.
fn cell_style(ds: &Ctx<'_>, font: Font) -> TextInputStyle {
    let mut style = ds::field_style(ds, font, false);
    style.background = Rgba::TRANSPARENT;
    style.border = Rgba::TRANSPARENT;
    style.border_focused = Rgba::TRANSPARENT;
    style
}

impl RequestEditor {
    pub(super) fn paint_editor(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint {
            bounds: b, scale, text, scene, palette: p, icons, waker, focused, ..
        } = ctx;
        let (current, part_shown) = (self.tab(), self.part());
        let running = self.running().is_some();
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let mut hits: Vec<(Rect, Hit)> = Vec::new();
        let mut layer = scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background));
        let Some(form) = self.form.as_mut() else {
            let message = self.error.clone().unwrap_or_else(|| t("http.loading"));
            ds::empty_state(layer, text, &ds, b, &message, None);
            self.hits.clear();
            return;
        };
        let font = ds.ui_font(0.0);
        let code = ds.code_font(-0.5);
        // The request on the left, its response on the right, the line between them dragged.
        let share = if self.share > 0.0 { self.share } else { DEFAULT_SHARE };
        let least = px(320.0).min(b.size.width / 2.0);
        let left_w = (b.size.width * share).clamp(least, (b.size.width - least).max(least));
        self.split_area = b;
        let left = Rect::new(b.min_x(), b.min_y(), left_w, b.size.height);
        let right =
            Rect::new(left.max_x(), b.min_y(), (b.max_x() - left.max_x()).max(0.0), b.size.height);
        // The top row: the method, the URL, Send.
        let top = Rect::new(left.min_x(), left.min_y(), left.size.width, px(TOP));
        let mid = top.min_y() + top.size.height / 2.0;
        let h = px(32.0);
        let method_r = Rect::new(top.min_x() + px(PAD), mid - h / 2.0, px(104.0), h);
        // The method: the shared dropdown, its value in the method's color.
        let look = ds::DropdownLook {
            value_color: Some(crate::format::method_color(&form.method, p)),
            ..ds::DropdownLook::default()
        };
        let spec = DropdownSpec {
            value: &form.method,
            open: self.picker.is_open() && self.picking == Some(super::Pick::Method),
            hovered: self.hover == Some(Hit::Method),
            enabled: true,
            note: None,
            placeholder: None,
        };
        ds::dropdown_styled(layer, text, icons, &ds, method_r, (spec, look));
        hits.push((method_r, Hit::Method));
        // Send, or Cancel while its run goes.
        let (send_label, cancel_label) = (t("http.send"), t("http.cancel"));
        let send_w = ds::primary_width(text, &ds, &[send_label.as_str(), cancel_label.as_str()]);
        let send_r = Rect::new(top.max_x() - px(PAD) - send_w, mid - h / 2.0, send_w, h);
        let (kind, label, hit) = if running {
            (ButtonKind::Danger, &cancel_label, Hit::Cancel)
        } else {
            (ButtonKind::Primary, &send_label, Hit::Send)
        };
        let spec = ButtonSpec::new(kind).label(label).hovered(self.hover == Some(hit));
        ds::button(layer, text, icons, &ds, send_r, spec);
        hits.push((send_r, hit));
        let url_r = Rect::new(
            method_r.max_x() + px(8.0),
            mid - h / 2.0,
            (send_r.min_x() - method_r.max_x() - px(16.0)).max(0.0),
            h,
        );
        let url_focused = focused && self.focus == Some(Focus::Url);
        // The text ends before the × (never under it): the shared field, × in its room.
        let clearing = !form.url.text().is_empty();
        let style = TextInputStyle {
            trail: if clearing { px(CLEAR_ROOM) } else { 0.0 },
            ..ds::field_style(&ds, code.clone(), false)
        };
        form.url.paint(layer, text, url_r, url_focused, &t("http.url_placeholder"), &style);
        hits.push((url_r, Hit::Field(Focus::Url)));
        if clearing {
            let hovered = self.hover == Some(Hit::Clear(Focus::Url));
            let r = ds::clear_button(layer, icons, &ds, url_r.max_x() - px(16.0), mid, hovered);
            hits.push((r, Hit::Clear(Focus::Url)));
        }
        // The tabs.
        let tabs = Rect::new(left.min_x(), top.max_y(), left.size.width, px(TABS));
        ds::divider(layer, &ds, tabs.min_x(), tabs.max_x(), tabs.min_y());
        ds::divider(layer, &ds, tabs.min_x(), tabs.max_x(), tabs.max_y());
        let labels: Vec<String> = Tab::ALL
            .iter()
            .map(|tab| {
                let label = t(tab.label_key());
                let count = match tab {
                    Tab::Headers => form
                        .headers
                        .iter()
                        .filter(|r| r.enabled && !r.name.text().is_empty())
                        .count(),
                    Tab::Params => form
                        .params
                        .iter()
                        .filter(|r| r.enabled && !r.name.text().is_empty())
                        .count(),
                    _ => 0,
                };
                if count > 0 { format!("{label}  {count}") } else { label }
            })
            .collect();
        // Narrow: the tabs closer together, then smaller; never past the editor's half.
        let room = tabs.size.width - 2.0 * px(PAD);
        let width_of = |text: &mut text_layout::TextSystem, font: &Font, pad: f32, gap: f32| {
            labels.iter().map(|l| text.shape_line(l, font).width + pad + gap).sum::<f32>()
        };
        // Each tab's room on each side of its label (logical pixels) and the gap after it.
        let (tab_font, tab_pad, tab_gap) =
            if width_of(text, &font, 2.0 * px(TAB_PAD), px(TAB_GAP)) <= room {
                (font.clone(), TAB_PAD, px(TAB_GAP))
            } else if width_of(text, &font, 2.0 * px(TAB_PAD_CLOSE), 0.0) <= room {
                (font.clone(), TAB_PAD_CLOSE, 0.0)
            } else {
                (ds.ui_font(-1.5), TAB_PAD_SMALL, 0.0)
            };
        let tabs_layer = &mut *layer;
        let mut x = tabs.min_x() + px(PAD);
        for (tab, label) in Tab::ALL.into_iter().zip(&labels) {
            let shown = UnderlineTab::new(label).on(tab == current).pad(tab_pad);
            // What doesn't fit even so isn't drawn over the response beside it.
            if x + shown.width(text, &ds, &tab_font) > tabs.max_x() {
                break;
            }
            let at = (x, tabs.min_y(), tabs.size.height);
            let r = tab_strip::underline_tab(tabs_layer, text, &ds, &tab_font, at, &shown);
            if let Some(seen) = r.intersect(&tabs) {
                hits.push((seen, Hit::Tab(tab)));
            }
            x = r.max_x() + tab_gap;
        }
        // What the tab holds.
        let content = Rect::new(
            left.min_x() + px(PAD),
            tabs.max_y() + px(10.0),
            left.size.width - 2.0 * px(PAD),
            (left.max_y() - tabs.max_y() - px(14.0)).max(0.0),
        );
        let focus = if focused { self.focus } else { None };
        let mut anchor = Rect::default();
        match current {
            Tab::Params => {
                // The URL as it goes, and copying it.
                let preview =
                    Rect::new(content.min_x(), content.min_y(), content.size.width, px(58.0));
                layer.quad(
                    Quad::filled(preview, p.text.with_alpha(0.04))
                        .rounded(px(6.0))
                        .bordered(ds.line(), p.card_border),
                );
                let small = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
                let caption = text.shape_line(&t("http.url_preview"), &small);
                draw_text(
                    layer,
                    &caption,
                    preview.min_x() + px(12.0),
                    preview.min_y() + px(18.0),
                    p.text_muted,
                );
                let copy_r = Rect::new(
                    preview.max_x() - px(40.0),
                    preview.min_y() + px(14.0),
                    px(30.0),
                    px(30.0),
                );
                let shown = ds::ellipsize(
                    text,
                    form.url.text(),
                    &code,
                    copy_r.min_x() - preview.min_x() - px(24.0),
                );
                let shaped = text.shape_line(&shown, &code);
                draw_text(
                    layer,
                    &shaped,
                    preview.min_x() + px(12.0),
                    preview.min_y() + px(42.0),
                    p.text,
                );
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .icon("copy")
                    .rest_tone(Tone::Neutral)
                    .hovered(self.hover == Some(Hit::CopyUrl));
                ds::button(layer, text, icons, &ds, copy_r, spec);
                hits.push((copy_r, Hit::CopyUrl));
                let caption_y = preview.max_y() + px(12.0);
                let caption = text.shape_line(&t("http.query_params"), &small);
                draw_text(layer, &caption, content.min_x(), caption_y + px(14.0), p.text_muted);
                let actions_y = caption_y + px(24.0);
                if self.bulk.is_none() {
                    let row = Rect::new(
                        content.min_x(),
                        actions_y,
                        content.size.width,
                        px(ds::TOOLBAR_BUTTON_H),
                    );
                    actions_row(
                        (layer, text, icons),
                        &ds,
                        (row, Table::Params, self.hover),
                        &mut hits,
                    );
                }
                let bulk_label =
                    t(if self.bulk.is_some() { "http.table_edit" } else { "http.bulk_edit" });
                let bulk_w = ds::toolbar_button_width(text, &ds, &bulk_label);
                let bulk_r = Rect::new(
                    content.max_x() - bulk_w,
                    actions_y,
                    bulk_w,
                    px(ds::TOOLBAR_BUTTON_H),
                );
                let spec = ButtonSpec::new(ButtonKind::Toolbar)
                    .label(&bulk_label)
                    .icon("file-text")
                    .hovered(self.hover == Some(Hit::Bulk));
                ds::button(layer, text, icons, &ds, bulk_r, spec);
                hits.push((bulk_r, Hit::Bulk));
                let table_top = actions_y + px(ds::TOOLBAR_BUTTON_H) + px(8.0);
                let area = Rect::new(
                    content.min_x(),
                    table_top,
                    content.size.width,
                    (content.max_y() - table_top).max(0.0),
                );
                if let Some(bulk) = self.bulk.as_mut() {
                    let spec = MultilineSpec {
                        placeholder: "name=value",
                        focused: focus == Some(Focus::Area(Area::Bulk)),
                        subject_limit: None,
                    };
                    bulk.paint(scene, text, &ds, area, spec);
                    layer = scene.push_layer(Some(b));
                } else {
                    paint_rows(
                        (layer, text, icons),
                        &ds,
                        (area, Table::Params, &mut form.params),
                        (focus, self.hover),
                        &mut hits,
                    );
                }
            }
            Tab::Headers => {
                let actions = actions_row(
                    (layer, text, icons),
                    &ds,
                    (content, Table::Headers, self.hover),
                    &mut hits,
                );
                let mut y = actions.max_y() + px(6.0);
                // The app's own, dimmed.
                for (name, value) in form.automatic_headers() {
                    let n = text.shape_line(&name, &font);
                    let v = text.shape_line(&value, &font);
                    let base = ds::optical_baseline(y, px(ROW), &font);
                    draw_text(
                        layer,
                        &n,
                        content.min_x() + px(10.0),
                        base,
                        p.text_muted.with_alpha(0.7),
                    );
                    draw_text(
                        layer,
                        &v,
                        content.min_x() + content.size.width * 0.4 + px(10.0),
                        base,
                        p.text_muted.with_alpha(0.7),
                    );
                    y += px(ROW);
                }
                let area = Rect::new(
                    content.min_x(),
                    y,
                    content.size.width,
                    (content.max_y() - y).max(0.0),
                );
                paint_rows(
                    (layer, text, icons),
                    &ds,
                    (area, Table::Headers, &mut form.headers),
                    (focus, self.hover),
                    &mut hits,
                );
            }
            Tab::Body => {
                let kind_r = Rect::new(content.min_x(), content.min_y(), px(220.0), px(32.0));
                ds::dropdown(
                    layer,
                    text,
                    icons,
                    &ds,
                    kind_r,
                    DropdownSpec {
                        value: &t(form.body_kind.label_key()),
                        open: self.picker.is_open(),
                        hovered: self.hover == Some(Hit::BodyKind),
                        enabled: true,
                        note: None,
                        placeholder: None,
                    },
                );
                hits.push((kind_r, Hit::BodyKind));
                anchor = kind_r;
                let area = Rect::new(
                    content.min_x(),
                    kind_r.max_y() + px(10.0),
                    content.size.width,
                    (content.max_y() - kind_r.max_y() - px(10.0)).max(0.0),
                );
                match form.body_kind {
                    BodyKind::None => ds::empty_state(
                        layer,
                        text,
                        &ds,
                        area,
                        &t("http.no_body"),
                        Some(&t("http.no_body_detail")),
                    ),
                    BodyKind::UrlEncoded => paint_rows(
                        (layer, text, icons),
                        &ds,
                        (area, Table::Form, &mut form.form),
                        (focus, self.hover),
                        &mut hits,
                    ),
                    BodyKind::FormData => paint_rows(
                        (layer, text, icons),
                        &ds,
                        (area, Table::Multipart, &mut form.parts),
                        (focus, self.hover),
                        &mut hits,
                    ),
                    BodyKind::File => {
                        let r = Rect::new(area.min_x(), area.min_y(), area.size.width, px(32.0));
                        form.file.paint(
                            layer,
                            text,
                            r,
                            focus == Some(Focus::File),
                            &t("http.file_placeholder"),
                            &ds::field_style(&ds, code.clone(), false),
                        );
                        hits.push((r, Hit::Field(Focus::File)));
                    }
                    BodyKind::GraphQl => {
                        let half = (area.size.height * 0.62).round();
                        let q =
                            Rect::new(area.min_x(), area.min_y(), area.size.width, half - px(6.0));
                        let v = Rect::new(
                            area.min_x(),
                            q.max_y() + px(12.0),
                            area.size.width,
                            (area.max_y() - q.max_y() - px(12.0)).max(0.0),
                        );
                        hits.push((q, Hit::Field(Focus::Area(Area::Body))));
                        hits.push((v, Hit::Field(Focus::Area(Area::Variables))));
                        form.body.paint(
                            scene,
                            text,
                            &ds,
                            q,
                            MultilineSpec {
                                placeholder: "query { … }",
                                focused: focus == Some(Focus::Area(Area::Body)),
                                subject_limit: None,
                            },
                        );
                        form.variables.paint(
                            scene,
                            text,
                            &ds,
                            v,
                            MultilineSpec {
                                placeholder: "{ \"variables\": … }",
                                focused: focus == Some(Focus::Area(Area::Variables)),
                                subject_limit: None,
                            },
                        );
                        layer = scene.push_layer(Some(b));
                    }
                    _ => {
                        hits.push((area, Hit::Field(Focus::Area(Area::Body))));
                        form.body.paint(
                            scene,
                            text,
                            &ds,
                            area,
                            MultilineSpec {
                                placeholder: &t("http.body_placeholder"),
                                focused: focus == Some(Focus::Area(Area::Body)),
                                subject_limit: None,
                            },
                        );
                        layer = scene.push_layer(Some(b));
                    }
                }
            }
            Tab::Auth => {
                let kind_r = Rect::new(content.min_x(), content.min_y(), px(220.0), px(32.0));
                ds::dropdown(
                    layer,
                    text,
                    icons,
                    &ds,
                    kind_r,
                    DropdownSpec {
                        value: &t(form.auth_kind.label_key()),
                        open: self.picker.is_open(),
                        hovered: self.hover == Some(Hit::AuthKind),
                        enabled: true,
                        note: None,
                        placeholder: None,
                    },
                );
                hits.push((kind_r, Hit::AuthKind));
                anchor = kind_r;
                let mut y = kind_r.max_y() + px(16.0);
                if form.auth_kind == AuthKind::None {
                    let area = Rect::new(
                        content.min_x(),
                        y,
                        content.size.width,
                        (content.max_y() - y).max(0.0),
                    );
                    ds::empty_state(layer, text, &ds, area, &t("http.no_auth"), None);
                }
                for (i, label) in form.auth_kind.fields().iter().enumerate() {
                    let shaped = text.shape_line(&t(label), &font);
                    draw_text(
                        layer,
                        &shaped,
                        content.min_x(),
                        ds::optical_baseline(y, px(32.0), &font),
                        p.text_muted,
                    );
                    let r = Rect::new(
                        content.min_x() + px(160.0),
                        y,
                        (content.size.width - px(160.0)).max(0.0),
                        px(32.0),
                    );
                    if let Some(input) = form.auth.get(i) {
                        input.paint(
                            layer,
                            text,
                            r,
                            focus == Some(Focus::Auth(i)),
                            "",
                            &ds::field_style(&ds, code.clone(), false),
                        );
                    }
                    hits.push((r, Hit::Field(Focus::Auth(i))));
                    y += px(42.0);
                }
                if form.auth_kind == AuthKind::ApiKey {
                    let label = t(if form.key_in_query {
                        "http.auth_in_query"
                    } else {
                        "http.auth_in_header"
                    });
                    let w = ds::toolbar_button_width(text, &ds, &label);
                    let r = Rect::new(content.min_x() + px(160.0), y, w, px(ds::TOOLBAR_BUTTON_H));
                    let spec = ButtonSpec::new(ButtonKind::Toolbar)
                        .label(&label)
                        .icon("arrow-right")
                        .hovered(self.hover == Some(Hit::KeyPlace));
                    ds::button(layer, text, icons, &ds, r, spec);
                    hits.push((r, Hit::KeyPlace));
                }
                if form.auth_kind == AuthKind::OAuth2 {
                    let note = text.shape_line(&t("http.auth_oauth2_note"), &ds.ui_font(-1.0));
                    draw_text(layer, &note, content.min_x(), y + px(14.0), p.text_muted);
                }
            }
            Tab::Scripts => {
                let small = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
                let half = ((content.size.height - px(48.0)) / 2.0).max(0.0);
                let c1 = text.shape_line(&t("http.script_before"), &small);
                draw_text(layer, &c1, content.min_x(), content.min_y() + px(14.0), p.text_muted);
                let pre = Rect::new(
                    content.min_x(),
                    content.min_y() + px(22.0),
                    content.size.width,
                    half,
                );
                let c2 = text.shape_line(&t("http.script_after"), &small);
                draw_text(layer, &c2, content.min_x(), pre.max_y() + px(20.0), p.text_muted);
                let post = Rect::new(
                    content.min_x(),
                    pre.max_y() + px(28.0),
                    content.size.width,
                    (content.max_y() - pre.max_y() - px(28.0)).max(0.0),
                );
                hits.push((pre, Hit::Field(Focus::Area(Area::Pre))));
                hits.push((post, Hit::Field(Focus::Area(Area::Post))));
                form.pre.paint(
                    scene,
                    text,
                    &ds,
                    pre,
                    MultilineSpec {
                        placeholder: "request.variables.set(\"id\", 1);",
                        focused: focus == Some(Focus::Area(Area::Pre)),
                        subject_limit: None,
                    },
                );
                form.post.paint(scene, text, &ds, post, MultilineSpec { placeholder: "client.test(\"ok\", () => client.assert(response.status === 200));", focused: focus == Some(Focus::Area(Area::Post)), subject_limit: None });
                layer = scene.push_layer(Some(b));
            }
            Tab::Settings => {
                let mut y = content.min_y();
                for (setting, key, on) in [
                    (super::Setting::Redirects, "http.setting_redirects", form.follow_redirects),
                    (super::Setting::Cookies, "http.setting_cookies", form.cookie_jar),
                    (super::Setting::History, "http.setting_history", form.history),
                    (super::Setting::Encoding, "http.setting_encoding", form.encoding),
                ] {
                    let mid = y + px(16.0);
                    let check = ds::checkbox(
                        layer,
                        text,
                        &ds,
                        content.min_x(),
                        mid,
                        if on { Check::On } else { Check::Off },
                        true,
                    );
                    let shaped = text.shape_line(&t(key), &font);
                    draw_text(
                        layer,
                        &shaped,
                        check.max_x() + px(10.0),
                        ds::optical_baseline(y, px(32.0), &font),
                        p.text,
                    );
                    hits.push((
                        Rect::new(
                            content.min_x(),
                            y,
                            check.max_x() - content.min_x() + px(10.0) + shaped.width,
                            px(32.0),
                        ),
                        Hit::Toggle(setting),
                    ));
                    y += px(38.0);
                }
                y += px(8.0);
                for (focus, key) in [
                    (Focus::Timeout, "http.setting_timeout"),
                    (Focus::ConnectTimeout, "http.setting_connect_timeout"),
                ] {
                    let shaped = text.shape_line(&t(key), &font);
                    draw_text(
                        layer,
                        &shaped,
                        content.min_x(),
                        ds::optical_baseline(y, px(32.0), &font),
                        p.text_muted,
                    );
                    let r = Rect::new(content.min_x() + px(220.0), y, px(160.0), px(32.0));
                    let input =
                        if focus == Focus::Timeout { &form.timeout } else { &form.connect_timeout };
                    input.paint(
                        layer,
                        text,
                        r,
                        self.focus == Some(focus) && focused,
                        &t("http.setting_default"),
                        &ds::field_style(&ds, code.clone(), false),
                    );
                    hits.push((r, Hit::Field(focus)));
                    y += px(42.0);
                }
                let note = text.shape_line(&t("http.setting_units"), &ds.ui_font(-1.5));
                draw_text(layer, &note, content.min_x() + px(220.0), y + px(6.0), p.text_muted);
            }
            Tab::Docs => {
                let labels = [t("http.docs_write"), t("http.docs_preview")];
                let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let w = ds::segmented_width(text, &ds, &label_refs);
                let segments = ds::segmented(
                    layer,
                    text,
                    &ds,
                    ds::SegmentedSpec {
                        right: content.min_x() + w,
                        mid: content.min_y() + px(16.0),
                        labels: &label_refs,
                        selected: usize::from(self.docs_preview),
                        hovered: None,
                    },
                );
                for (i, r) in segments.into_iter().enumerate() {
                    hits.push((r, Hit::DocsMode(i == 1)));
                }
                let area = Rect::new(
                    content.min_x(),
                    content.min_y() + px(40.0),
                    content.size.width,
                    (content.size.height - px(40.0)).max(0.0),
                );
                if self.docs_preview {
                    let mut y = area.min_y() + px(18.0);
                    for line in form.docs.text().lines() {
                        let (line, f) = match line.strip_prefix("# ") {
                            Some(heading) => (heading, ds.ui_font(3.0).with_weight(Font::SEMIBOLD)),
                            None => (line, font.clone()),
                        };
                        let shaped = text.shape_line(line, &f);
                        draw_text(layer, &shaped, area.min_x(), y, p.text);
                        y += (f.size * 1.6).round();
                    }
                } else {
                    hits.push((area, Hit::Field(Focus::Area(Area::Docs))));
                    form.docs.paint(
                        scene,
                        text,
                        &ds,
                        area,
                        MultilineSpec {
                            placeholder: &t("http.docs_placeholder"),
                            focused: focus == Some(Focus::Area(Area::Docs)),
                            subject_limit: None,
                        },
                    );
                    layer = scene.push_layer(Some(b));
                }
            }
        }
        if let Some(why) = &self.error {
            let shaped = text.shape_line(why, &ds.ui_font(-1.0));
            draw_text(layer, &shaped, left.min_x() + px(PAD), left.max_y() - px(10.0), p.danger);
        }
        // The response.
        ds::divider_vertical(layer, &ds, right.min_x(), right.min_y(), right.max_y());
        let head = Rect::new(right.min_x(), right.min_y(), right.size.width, px(TOP));
        // Its tests when it has scripts, its page when it's HTML.
        let parts: Vec<Part> = self.shown_parts();
        let mut x = head.min_x() + px(PAD);
        for part in &parts {
            let title = part.title();
            let shown = UnderlineTab::new(&title).on(*part == part_shown);
            let at = (x, head.min_y(), head.size.height);
            let r = tab_strip::underline_tab(layer, text, &ds, &font, at, &shown);
            hits.push((r, Hit::Part(*part)));
            x = r.max_x();
        }
        ds::divider(layer, &ds, head.min_x(), head.max_x(), head.max_y());
        let response_r = Rect::new(
            right.min_x() + ds.line(),
            head.max_y(),
            (right.size.width - ds.line()).max(0.0),
            (right.max_y() - head.max_y()).max(0.0),
        );
        // Its run going: a loader beside the parts, Cancel on the left.
        if running {
            let label = t("http.running");
            ds::loader(
                layer,
                text,
                &ds,
                head.max_x() - px(PAD) - px(90.0),
                head.min_y() + head.size.height / 2.0,
                &ds::Loader {
                    text: Some(&label),
                    spinner: true,
                    size: 14.0,
                    ..ds::Loader::default()
                },
            );
        }
        match self.response.as_mut() {
            Some((_, _, view)) => view.paint(ViewerPaint {
                bounds: response_r,
                scale,
                text,
                scene,
                palette: p,
                icons,
                focused: false,
                waker: waker.clone(),
            }),
            None => {
                let layer = scene.push_layer(Some(response_r));
                ds::empty_state(
                    layer,
                    text,
                    &ds,
                    response_r,
                    &t("http.no_response"),
                    Some(&t("http.no_response_detail")),
                );
            }
        }
        // The line between the request and its response, dragged (as the other splitters).
        let seam = px(6.0);
        let splitter = self.splitter.get_or_insert_with(|| {
            ui_components::Splitter::new(
                ui_components::SplitterStyle::from_palette(p),
                ui_components::Orientation::Vertical,
            )
        });
        splitter.set_style(ui_components::SplitterStyle::from_palette(p));
        splitter.set_bounds(
            Rect::new(right.min_x() - seam / 2.0, b.min_y(), seam, b.size.height),
            scale,
        );
        splitter.paint(scene);
        // The list opened, over everything.
        if self.picker.is_open() {
            let anchor = match self.picking {
                Some(super::Pick::Method) => method_r,
                // The field typed in.
                Some(super::Pick::Suggest(focus, _)) => {
                    hits.iter().find(|(_, h)| *h == Hit::Field(focus)).map_or(anchor, |(r, _)| *r)
                }
                _ => anchor,
            };
            self.picker.paint(scene, text, icons, &ds, (anchor, b), &t("http.search"));
        }
        self.anchor = anchor;
        self.hits = hits;
    }
}

/// A table's actions over it (Add, Delete all), where they are.
fn actions_row(
    (layer, text, icons): (&mut Layer, &mut TextSystem, &icons::Icons),
    ds: &Ctx<'_>,
    (content, table, hover): (Rect, Table, Option<Hit>),
    hits: &mut Vec<(Rect, Hit)>,
) -> Rect {
    let px = |v: f32| ds.px(v);
    let h = px(ds::TOOLBAR_BUTTON_H);
    let mut x = content.min_x();
    for (label, icon, hit) in [
        (t("http.add_row"), "plus", Hit::Add(table)),
        (t("http.delete_all"), "trash", Hit::DeleteAll(table)),
    ] {
        let w = ds::toolbar_button_width(text, ds, &label);
        let r = Rect::new(x, content.min_y(), w, h);
        let spec = ButtonSpec::new(ButtonKind::Toolbar)
            .label(&label)
            .icon(icon)
            .hovered(hover == Some(hit));
        ds::button(layer, text, icons, ds, r, spec);
        hits.push((r, hit));
        x = r.max_x() + px(8.0);
    }
    Rect::new(content.min_x(), content.min_y(), content.size.width, h)
}

/// How far past a row's checkbox its remove button's center is (logical pixels): 12 apart,
/// then half its 16-pixel icon.
const TRASH_CENTER: f32 = 20.0;

/// A table's rows: the name, the value, on / off, remove (a multipart field: file or text).
fn paint_rows(
    (layer, text, icons): (&mut Layer, &mut TextSystem, &icons::Icons),
    ds: &Ctx<'_>,
    (area, table, rows): (Rect, Table, &mut [Row]),
    (focus, hover): (Option<Focus>, Option<Hit>),
    hits: &mut Vec<(Rect, Hit)>,
) {
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let font = ds.code_font(-0.5);
    let style = cell_style(ds, font);
    let row_h = px(ROW);
    let tail = px(if table == Table::Multipart { 112.0 } else { 64.0 });
    let name_w = ((area.size.width - tail) * 0.42).round();
    for (i, row) in rows.iter().enumerate() {
        let y = area.min_y() + i as f32 * row_h;
        if y + row_h > area.max_y() {
            break;
        }
        let name_r = Rect::new(area.min_x(), y + px(1.0), name_w, row_h - px(2.0));
        let value_r = Rect::new(
            name_r.max_x() + px(6.0),
            y + px(1.0),
            (area.size.width - tail - name_w - px(6.0)).max(0.0),
            row_h - px(2.0),
        );
        let dim = !row.enabled;
        let mut s = style.clone();
        if dim {
            s.text = p.text_muted.with_alpha(0.6);
        }
        row.name.paint(
            layer,
            text,
            name_r,
            focus == Some(Focus::Cell(table, i, false)),
            &t("http.cell_name"),
            &s,
        );
        row.value.paint(
            layer,
            text,
            value_r,
            focus == Some(Focus::Cell(table, i, true)),
            &t("http.cell_value"),
            &s,
        );
        hits.push((name_r, Hit::Field(Focus::Cell(table, i, false))));
        hits.push((value_r, Hit::Field(Focus::Cell(table, i, true))));
        // The cell typed in: its ×.
        for (value, r, input) in [(false, name_r, &row.name), (true, value_r, &row.value)] {
            let cell = Focus::Cell(table, i, value);
            if focus == Some(cell) && !input.text().is_empty() {
                let x = ds::clear_button(
                    layer,
                    icons,
                    ds,
                    r.max_x() - px(14.0),
                    r.min_y() + r.size.height / 2.0,
                    hover == Some(Hit::Clear(cell)),
                );
                hits.push((x, Hit::Clear(cell)));
            }
        }
        let mid = y + row_h / 2.0;
        let mut x = value_r.max_x() + px(10.0);
        if table == Table::Multipart {
            let label = t(if row.file { "http.part_file" } else { "http.part_text" });
            let shaped = text.shape_line(&label, &ds.ui_font(-1.0));
            let r = Rect::new(x, y + px(6.0), shaped.width + px(14.0), row_h - px(12.0));
            let spec = ButtonSpec::new(ButtonKind::Soft)
                .label(&label)
                .rest_tone(Tone::Neutral)
                .hovered(hover == Some(Hit::FileToggle(i)));
            ds::button(layer, text, icons, ds, r, spec);
            hits.push((r, Hit::FileToggle(i)));
            x = r.max_x() + px(8.0);
        }
        let check = ds::checkbox(
            layer,
            text,
            ds,
            x,
            mid,
            if row.enabled { Check::On } else { Check::Off },
            true,
        );
        hits.push((check, Hit::Check(table, i)));
        let trash = ds::square_button(
            ds,
            (check.max_x() + px(TRASH_CENTER) - px(button_size::SMALL) / 2.0, mid),
            button_size::SMALL,
        );
        let lit = hover == Some(Hit::Remove(table, i));
        // Removing it: in the danger color under the mouse.
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon("trash")
            .rest_tone(Tone::Neutral)
            .hover_content_tone(Tone::Danger)
            .hovered(lit);
        ds::button(layer, text, icons, ds, trash, spec);
        hits.push((trash, Hit::Remove(table, i)));
        ds::divider(layer, ds, area.min_x(), area.max_x(), y + row_h);
    }
}
