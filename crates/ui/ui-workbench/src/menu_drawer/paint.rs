//! Painting the drawer: its panels (the menus, a menu's items, a submenu), the search's results,
//! the footer, each row.

use super::*;

impl MenuDrawer {
    pub fn paint(
        &mut self,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.sync_query();
        // The category the mouse rested on, its time come.
        if let Some(m) = self.intent.due(Instant::now())
            && self.under == Some(Under::Menu(m))
        {
            self.nav.show_menu(m);
        }
        if let Some(i) = self.sub_intent.due(Instant::now())
            && self.under == Some(Under::Item(i))
        {
            self.nav.show_sub(i);
        }
        self.menu_rows.clear();
        self.item_rows.clear();
        self.sub_rows.clear();
        self.result_rows.clear();
        let shown = self.shown.value();
        if shown <= 0.0 && !self.open {
            self.panel = Rect::default();
            self.items_panel = Rect::default();
            self.sub_panel = Rect::default();
            return;
        }
        let open_menu = self.nav.open_menu();
        self.widen.retarget(if open_menu.is_some() && self.open { 1.0 } else { 0.0 });
        if open_menu != self.painted {
            if open_menu.is_some() && self.painted.is_some() {
                self.swap.start(Duration::from_millis(110));
            }
            self.painted = open_menu;
            self.items_scroll.reset();
        }
        if open_menu.is_some() {
            self.last_shown = open_menu;
        }
        let open_sub = open_menu.zip(self.nav.open_sub());
        self.widen_sub.retarget(if open_sub.is_some() && self.open { 1.0 } else { 0.0 });
        if open_sub != self.sub_painted {
            if open_sub.is_some() && self.sub_painted.is_some() {
                self.sub_swap.start(Duration::from_millis(110));
            }
            self.sub_painted = open_sub;
            self.sub_scroll.reset();
        }
        if open_sub.is_some() {
            self.sub_last_shown = open_sub;
        }

        let ctx = Ctx::new(palette, self.scale);
        let px = |v: f32| ctx.px(v);
        let a = self.area;
        // The backdrop: the window behind set back (the theme's style; more in a dark
        // theme), fading with the slide — the window's own buttons (Windows) too, under it
        // like the rest (they still take their clicks).
        let back = scene.push_layer(Some(a));
        ds::backdrop_faded(back, &ctx, a, BackdropRole::Drawer, shown, None);

        // The panels: the categories' and (widening) the submenu's, as one square surface
        // flush with the window's left, top and bottom edges, sliding in; no shadow, a
        // border line on its right edge.
        let menus_w = px(MENUS_W).min(a.size.width - px(24.0)).max(0.0);
        let items_full = px(ITEMS_W).min(a.size.width - menus_w - px(24.0)).max(0.0);
        let widen = self.widen.value();
        let items_w = (items_full * widen).round();
        // A row's own submenu beside the submenu; where three don't fit, over it.
        self.narrow = a.size.width < menus_w + items_full + px(SUB_W) + px(24.0);
        let sub_full = if self.narrow { items_full } else { px(SUB_W) };
        let sub_w = (sub_full * self.widen_sub.value()).round().min(sub_full);
        let width = menus_w + items_w + if self.narrow { 0.0 } else { sub_w };
        let travel = width + px(48.0);
        let x0 = (a.min_x() - travel * (1.0 - shown)).round();
        let panel = Rect::new(x0, a.min_y(), width, a.size.height);
        self.panel = Rect::new(
            x0.max(a.min_x()),
            a.min_y(),
            (panel.max_x() - x0.max(a.min_x())).max(0.0),
            a.size.height,
        );
        let layer = scene.push_layer(Some(a));
        // The panel's glass (the theme's `[floating]`, the user's over it): its color slightly
        // translucent over a blurred copy of what's under it. The backdrop's own glass under
        // it already blurs that (as much or more): nothing more to make. Otherwise its own
        // blur, once it has slid in (made once there, kept while nothing under it changes;
        // while it slides, opaque). Simple, or nothing blurred under it: opaque, as always.
        let glass = palette.floating;
        let scale = self.scale;
        let backdrop_blurs = palette
            .backdrop
            .blur_radius(scale)
            .is_some_and(|b| glass.blur_radius(scale).is_some_and(|g| b >= g));
        let settled = shown >= 1.0 && !self.shown.animating();
        let own_blur = glass.blur_radius(scale).filter(|_| !backdrop_blurs && settled);
        let frosted = glass.is_glass() && (backdrop_blurs || own_blur.is_some());
        if let Some(radius) = own_blur {
            layer.backdrop_blur(panel, radius, 1.0);
        }
        let fill =
            if frosted { glass.fill(palette.popup_background) } else { palette.popup_background };
        layer.quad(Quad::filled(panel, fill));
        let line = ctx.line();
        if frosted {
            // A faint lighter inner edge: the glass's rim.
            layer.quad(Quad::filled(
                Rect::new(panel.max_x() - 2.0 * line, panel.min_y(), line, panel.size.height),
                palette.text.with_alpha(0.06),
            ));
        }
        crate::ds::divider_vertical(
            layer,
            &ctx,
            panel.max_x() - line,
            panel.min_y(),
            panel.max_y(),
        );
        // Under the header row (the bar's height: its menu button, macOS' window buttons).
        let body_top = panel.min_y() + self.bar_h;
        let first = Rect::new(
            panel.min_x(),
            body_top,
            menus_w,
            (panel.max_y() - px(FOOTER_H) - body_top).max(0.0),
        );
        let second = Rect::new(first.max_x(), body_top, items_w, first.size.height);
        self.items_panel = if items_w > 0.0 { second } else { Rect::default() };
        let third_x = if self.narrow { second.min_x() } else { second.max_x() };
        let third_w = if self.narrow { sub_w.min(items_w) } else { sub_w };
        let third = Rect::new(third_x, body_top, third_w, first.size.height);
        self.sub_panel = if third.size.width > 0.0 { third } else { Rect::default() };

        // The footer, across both panels.
        let hints = self.footer_hints();
        let hints: Vec<KeyHint<'_>> =
            hints.iter().map(|(keys, action)| (keys.as_slice(), action.as_str())).collect();
        let room = width - px(PAD) * 2.0;
        let style = palette.key_hints(self.scale);
        let fit = ui_kit::fitting_key_hints(text, &hints, &style, room);
        let footer = KeyFooter { panel, height: px(FOOTER_H), pad: px(PAD), radius: 0.0 };
        paint_key_footer(scene, text, &ctx, footer, room, fit);

        self.paint_menus(first, icons, text, scene, &ctx);
        let mut anchor = None;
        if items_w > 0.0 {
            let layer = scene.push_layer(Some(second));
            ds::divider_vertical_faded(layer, &ctx, second.min_x(), second.min_y(), second.max_y());
            if let Some(m) = self.last_shown {
                anchor = self.paint_items(
                    m,
                    second,
                    items_full,
                    &mut ui_components::ScenePainter::new(scene, text, icons, ctx),
                );
            }
        } else {
            self.items_room = Rect::default();
        }
        self.sub_back = Rect::default();
        match self.sub_last_shown {
            Some((m, i)) if third.size.width > 0.0 => {
                self.paint_sub(
                    (m, i),
                    third,
                    sub_full,
                    anchor,
                    &mut ui_components::ScenePainter::new(scene, text, icons, ctx),
                );
            }
            _ => self.sub_room = Rect::default(),
        }
        self.paint_button(icons, text, scene, &ctx);
        self.reveal = false;
    }

    /// The menu button where the bar has it, lit (the drawer is its menu): it stays put
    /// while the drawer slides in and out under it.
    pub(super) fn paint_button(
        &self,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) {
        let b = self.button;
        if b.size.width <= 0.0 {
            return;
        }
        let layer = scene.push_layer(Some(self.area));
        // The bar's own menu button, lit as far as the drawer is out.
        let lit = if self.open { 1.0 } else { self.shown.value() };
        let spec = ui_components::ds::ButtonSpec::new(ui_components::ds::ButtonKind::Ghost)
            .icon("menu")
            .rest_tone(ui_components::ds::Tone::Neutral)
            .hover_amount(lit);
        ui_components::ds::button(layer, text, icons, ctx, b, spec);
    }

    /// The keys that work now: on the categories, in a submenu, on the filter's results.
    pub(super) fn footer_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        let t = i18n::t;
        let mut hints = vec![(vec!["↑", "↓"], t("menu_drawer.hint_move"))];
        match self.nav.focus() {
            NavFocus::Menus => {
                hints.push((vec!["→"], t("menu_drawer.hint_open")));
                hints.push((vec!["esc"], t("menu_drawer.hint_close")));
            }
            NavFocus::Items => {
                // On a row with a submenu of its own: → opens it.
                match self.nav.selected_item() {
                    Some(i) if self.nav.item_is_submenu(i) => {
                        hints.push((vec!["→"], t("menu_drawer.hint_open")));
                    }
                    _ if self.nav.selected_disabled() => {}
                    _ => hints.push((vec!["↵"], t("menu_drawer.hint_run"))),
                }
                hints.push((vec!["←"], t("menu_drawer.hint_back")));
                hints.push((vec!["esc"], t("menu_drawer.hint_close")));
            }
            NavFocus::Sub => {
                if !self.nav.selected_disabled() {
                    hints.push((vec!["↵"], t("menu_drawer.hint_run")));
                }
                hints.push((vec!["←"], t("menu_drawer.hint_back")));
                hints.push((vec!["esc"], t("menu_drawer.hint_close")));
            }
            NavFocus::Results => {
                if !self.nav.selected_disabled() {
                    hints.push((vec!["↵"], t("menu_drawer.hint_run")));
                }
                hints.push((vec!["esc"], t("menu_drawer.hint_clear")));
            }
        }
        hints
    }

    /// The categories' panel: the filter on top, the categories (or the results) under it.
    pub(super) fn paint_menus(
        &mut self,
        first: Rect,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) {
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let pad = px(PAD);
        // The filter.
        let field = Rect::new(
            first.min_x() + pad,
            first.min_y() + pad,
            first.size.width - pad * 2.0,
            px(FIELD_H),
        );
        self.field = field;
        let layer = scene.push_layer(Some(first));
        // The drawer's one field: its caret while the drawer is open.
        let placeholder = i18n::t("menu_drawer.filter");
        self.query.paint(layer, text, icons, ctx, (field, self.open), &placeholder);

        // The list's room, under the field.
        let room = Rect::new(
            first.min_x(),
            field.max_y() + px(8.0),
            first.size.width,
            (first.max_y() - field.max_y() - px(8.0) - px(6.0)).max(0.0),
        );
        self.menus_room = room;
        self.menus_scroll.set_viewport(room.size.height);
        let font = ctx.ui_font(0.0);
        let row_x = first.min_x() + px(8.0);
        // The rows' places, from the list's top (content coordinates), then scrolled.
        let filtering = self.nav.filtering();
        let count = if filtering { self.nav.results().len() } else { self.nav.menus().len() };
        let row_h = px(MENU_ROW);
        let content = row_h * count as f32 + px(4.0);
        // Lit: the result chosen, or the category whose submenu is shown.
        let selected = if filtering { self.nav.selected_result() } else { self.nav.open_menu() };
        // The scroll measured before the rows are placed (their heights are all the same).
        self.menus_scroll.measure(0.0, content);
        // Scrolling, the rows leave the scrollbar its lane.
        let lane = if self.menus_scroll.overflows() { px(10.0) } else { 0.0 };
        let row_w = first.size.width - px(16.0) - lane;
        if self.reveal
            && let Some(i) = selected
        {
            let top = self.menus_scroll.origin(room.min_y()) + px(2.0) + row_h * i as f32;
            self.menus_scroll.keep_visible(Rect::new(row_x, top, row_w, row_h), room);
        }
        let origin = self.menus_scroll.origin(room.min_y()) + px(2.0);
        let layer = scene.push_layer(Some(room));
        if filtering && count == 0 {
            let empty = text.shape_line(&i18n::t("menu_drawer.nothing"), &ctx.ui_font(-0.5));
            draw_text(
                layer,
                &empty,
                (first.min_x() + (first.size.width - empty.width) / 2.0).round(),
                ds::optical_baseline(room.min_y() + px(10.0), row_h, &ctx.ui_font(-0.5)),
                p.text_muted,
            );
        }
        for i in 0..count {
            let top = origin + row_h * i as f32;
            if top + row_h < room.min_y() || top > room.max_y() {
                continue;
            }
            let row = Rect::new(row_x, top, row_w, row_h);
            let lit = selected == Some(i);
            let under = match self.under {
                Some(Under::Menu(m)) => !filtering && m == i,
                Some(Under::Result(r)) => filtering && r == i,
                _ => false,
            };
            // The category shown while the keyboard or the mouse is in its submenu: "open"
            // (a neutral fill, an accent mark), the row lit there being the submenu's.
            let open = lit && !filtering && self.nav.focus() == NavFocus::Items;
            if open {
                paint_open_mark(layer, ctx, row);
            } else {
                paint_row_background(layer, ctx, row, lit, under);
            }
            if filtering {
                let found = self.nav.results()[i];
                self.paint_result(
                    &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
                    row,
                    found,
                    lit,
                );
                self.result_rows.push((row, i));
            } else {
                let menu = &self.nav.menus()[i];
                paint_icon(layer, icons, ctx, menu.icon, row, (px(ICON), px(ICON_AT)), lit);
                let label = text.shape_line(&menu.title, &font);
                draw_text(
                    layer,
                    &label,
                    row.min_x() + px(LABEL_AT),
                    ds::optical_baseline(top, row_h, &font),
                    p.text,
                );
                if let Some(id) = icons.id("chevron-right") {
                    let size = px(13.0);
                    let at = Rect::new(
                        (row.max_x() - px(10.0) - size).round(),
                        (top + (row_h - size) / 2.0).round(),
                        size,
                        size,
                    );
                    let color = if open {
                        p.accent
                    } else if lit {
                        p.text
                    } else {
                        p.text_muted
                    };
                    draw_icon(layer, icons, id, at, color);
                }
                self.menu_rows.push((row, i));
            }
        }
        self.menus_bar.paint(
            scene,
            p,
            self.scale,
            room,
            self.menus_scroll.content(),
            self.menus_scroll.offset(),
        );
    }

    /// A result: its icon (or its menu's), its path muted (`Code › Folding › `, cut in its
    /// middle when it doesn't fit), then its label; its shortcut on the right.
    pub(super) fn paint_result(
        &mut self,
        pen: &mut ui_components::LayerPainter<'_>,
        row: Rect,
        found: crate::menu_nav::Found,
        lit: bool,
    ) {
        let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let Some(entry @ MenuEntry::Item { shortcut, icon, .. }) = self.nav.entry(found) else {
            return;
        };
        let Some((path, label)) = self.nav.path(found) else { return };
        let menu = &self.nav.menus()[found.menu];
        // Greyed out when it doesn't apply now; lit, why in its keys' place.
        let disabled = self.nav.disabled(entry);
        let faded = ctx.with_opacity(ctx.opacity * DISABLED);
        let ctx = if disabled.is_some() { &faded } else { ctx };
        let why = disabled.flatten().filter(|_| lit).map(str::to_owned);
        let shortcut = why.or_else(|| shortcut.clone());
        let lit = lit && disabled.is_none();
        paint_icon(layer, icons, ctx, icon.or(menu.icon), row, (px(ICON), px(ICON_AT)), lit);
        let font = ctx.ui_font(-0.5);
        let key_font = ctx.ui_font(-1.0);
        let baseline = ds::optical_baseline(row.min_y(), row.size.height, &font);
        let mut right = row.max_x() - px(10.0);
        if let Some(key) = shortcut {
            let shaped = text.shape_line(&key, &key_font);
            let x = (right - shaped.width).round();
            draw_text(
                layer,
                &shaped,
                x,
                ds::optical_baseline(row.min_y(), row.size.height, &key_font),
                p.text_muted,
            );
            right = x - px(12.0);
        }
        let x = row.min_x() + px(LABEL_AT);
        let room = (right - x).max(0.0);
        let path = format!("{path} › ");
        let path_w = text.shape_line(&path, &font).width;
        let label_w = text.shape_line(&label, &font).width;
        // The label keeps its room first; the path takes what's left (at least a part).
        let path_room =
            if path_w + label_w <= room { path_w } else { (room - label_w).max(room * 0.45) };
        let path = ds::ellipsize_middle(text, &path, &font, path_room);
        let shaped = text.shape_line(&path, &font);
        let path_w = shaped.width;
        draw_text(layer, &shaped, x, baseline, p.text_muted);
        let label = ds::ellipsize(text, &label, &font, (room - path_w).max(0.0));
        let shaped = text.shape_line(&label, &font);
        draw_text(layer, &shaped, x + path_w, baseline, p.text);
    }

    /// The submenu's panel: its title, its rows (icon, label, shortcut on the right — or a
    /// chevron, on a row with a submenu of its own —, separators), laid out at its full
    /// width (`full`) and uncovered as it widens. Where the row whose submenu is open was
    /// drawn (the third panel lines up with it).
    pub(super) fn paint_items(
        &mut self,
        m: usize,
        second: Rect,
        full: f32,
        pen: &mut ui_components::ScenePainter<'_>,
    ) -> Option<Rect> {
        let (scene, text, icons, ctx) = (&mut *pen.scene, &mut *pen.text, pen.icons, &pen.ds);
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let fade = self.swap.value();
        let ctx = &ctx.with_opacity(fade);
        let menu = self.nav.menus().get(m).cloned()?;
        let pad = px(PAD);
        let inner = Rect::new(second.min_x(), second.min_y(), full, second.size.height);
        // The title, level with the filter.
        let title_font = ctx.ui_font(0.5).with_weight(Font::SEMIBOLD);
        let head = Rect::new(
            inner.min_x() + pad,
            inner.min_y() + pad,
            inner.size.width - pad * 2.0,
            px(FIELD_H),
        );
        let layer = scene.push_layer(Some(second));
        let shaped = text.shape_line(&menu.title, &title_font);
        draw_text(
            layer,
            &shaped,
            head.min_x() + px(LABEL_AT - PAD + 8.0),
            ds::optical_baseline(head.min_y(), head.size.height, &title_font),
            ctx.fade(p.text),
        );
        let room = Rect::new(
            second.min_x(),
            head.max_y() + px(8.0),
            second.size.width,
            (second.max_y() - head.max_y() - px(8.0) - px(6.0)).max(0.0),
        );
        self.items_room = room;
        self.items_scroll.set_viewport(room.size.height);
        let row_x = inner.min_x() + px(8.0);
        let (tops, content) = row_tops(&menu.entries, ctx);
        self.items_scroll.measure(0.0, content);
        let lane = if self.items_scroll.overflows() { px(10.0) } else { 0.0 };
        let row_w = inner.size.width - px(16.0) - lane;
        let selected = self.nav.selected_item();
        if self.reveal
            && let Some(i) = selected
        {
            let top = self.items_scroll.origin(room.min_y()) + tops[i];
            self.items_scroll.keep_visible(Rect::new(row_x, top, row_w, px(ITEM_ROW)), room);
        }
        let origin = self.items_scroll.origin(room.min_y());
        let lift = ((1.0 - fade) * px(4.0)).round();
        let open = self.sub_last_shown.filter(|(sm, _)| *sm == m).map(|(_, i)| i);
        let in_sub = self.nav.focus() == NavFocus::Sub;
        let layer = scene.push_layer(Some(room));
        let mut anchor = None;
        for (i, entry) in menu.entries.iter().enumerate() {
            let top = origin + tops[i] + lift;
            let row = Rect::new(row_x, top, row_w, px(ITEM_ROW));
            if open == Some(i) {
                anchor = Some(row);
            }
            let look = RowLook {
                lit: selected == Some(i),
                under: self.under == Some(Under::Item(i)),
                // Its submenu shown while the keyboard (or the mouse) is in it.
                open: open == Some(i) && in_sub && self.nav.open_sub() == Some(i),
                disabled: self.nav.disabled(entry).map(|why| why.map(str::to_owned)),
            };
            if paint_entry(
                &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
                entry,
                row,
                room,
                look,
            ) {
                self.item_rows.push((row, i));
            }
        }
        let bar_room =
            Rect::new(room.min_x(), room.min_y(), second.size.width.min(full), room.size.height);
        self.items_bar.paint(
            scene,
            p,
            self.scale,
            bar_room,
            self.items_scroll.content(),
            self.items_scroll.offset(),
        );
        anchor
    }

    /// The third panel: a row's own submenu (`(category, row)`), at its full width `full`
    /// and uncovered as it widens. Beside the submenu, its title and rows line up with the
    /// row that opened it (`anchor`), moved up as far as they need to fit; over it (a
    /// narrow window), a header back on top: `‹ Code › Folding`.
    pub(super) fn paint_sub(
        &mut self,
        (m, i): (usize, usize),
        third: Rect,
        full: f32,
        anchor: Option<Rect>,
        pen: &mut ui_components::ScenePainter<'_>,
    ) {
        let (scene, text, icons, ctx) = (&mut *pen.scene, &mut *pen.text, pen.icons, &pen.ds);
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let Some(menu) = self.nav.menus().get(m) else { return };
        let title = menu.title.clone();
        let Some(MenuEntry::Submenu { label, entries, .. }) = menu.entries.get(i).cloned() else {
            self.sub_room = Rect::default();
            return;
        };
        let fade = self.sub_swap.value();
        let layer = scene.push_layer(Some(third));
        if self.narrow {
            // Over the submenu: its own surface.
            layer.quad(Quad::filled(third, p.popup_background));
        } else {
            ds::divider_vertical_faded(layer, ctx, third.min_x(), third.min_y(), third.max_y());
        }
        let ctx = &ctx.with_opacity(fade);
        let pad = px(PAD);
        let inner = Rect::new(third.min_x(), third.min_y(), full, third.size.height);
        let (tops, content) = row_tops(&entries, ctx);
        let head_h = px(FIELD_H);
        let title_font = ctx.ui_font(0.5).with_weight(Font::SEMIBOLD);
        // Where its rows start: under the header (narrow), else level with the row that
        // opened it, as far up as they need to fit (never over the panel's top).
        let highest = inner.min_y() + pad + head_h + px(8.0);
        let rows_top = match anchor {
            Some(row) if !self.narrow => {
                let lowest = third.max_y() - px(6.0) - content;
                (row.min_y() - px(2.0)).min(lowest).max(highest)
            }
            _ => highest,
        };
        let head = Rect::new(
            inner.min_x() + pad,
            rows_top - px(8.0) - head_h,
            inner.size.width - pad * 2.0,
            head_h,
        );
        let x = head.min_x() + px(LABEL_AT - PAD + 8.0);
        let baseline = ds::optical_baseline(head.min_y(), head.size.height, &title_font);
        if self.narrow {
            // ‹ Code › Folding: a click (or ←) back to the submenu.
            self.sub_back = Rect::new(
                inner.min_x() + px(8.0),
                head.min_y(),
                inner.size.width - px(16.0),
                head_h,
            );
            ds::RowHighlight::new(false, self.under == Some(Under::SubBack))
                .hover(ds::HoverFill::Faint(ROW_UNDER))
                .paint(layer, ctx, self.sub_back);
            if let Some(id) = icons.id("chevron-left") {
                let size = px(14.0);
                let at = Rect::new(
                    (head.min_x() + px(2.0)).round(),
                    (head.min_y() + (head_h - size) / 2.0).round(),
                    size,
                    size,
                );
                draw_icon(layer, icons, id, at, ctx.fade(p.text_muted));
            }
            let path = text.shape_line(&format!("{title} › "), &title_font);
            draw_text(layer, &path, x, baseline, ctx.fade(p.text_muted));
            let room = (head.max_x() - x - path.width).max(0.0);
            let label = ds::ellipsize(text, &label, &title_font, room);
            let shaped = text.shape_line(&label, &title_font);
            draw_text(layer, &shaped, x + path.width, baseline, ctx.fade(p.text));
        } else {
            let shaped = text.shape_line(&label, &title_font);
            draw_text(layer, &shaped, x, baseline, ctx.fade(p.text));
        }
        let room = Rect::new(
            third.min_x(),
            rows_top,
            third.size.width,
            (third.max_y() - rows_top - px(6.0)).max(0.0),
        );
        self.sub_room = room;
        self.sub_scroll.set_viewport(room.size.height);
        self.sub_scroll.measure(0.0, content);
        let row_x = inner.min_x() + px(8.0);
        let lane = if self.sub_scroll.overflows() { px(10.0) } else { 0.0 };
        let row_w = inner.size.width - px(16.0) - lane;
        let selected = self.nav.selected_sub_item();
        if self.reveal
            && let Some(j) = selected
        {
            let top = self.sub_scroll.origin(room.min_y()) + tops[j];
            self.sub_scroll.keep_visible(Rect::new(row_x, top, row_w, px(ITEM_ROW)), room);
        }
        let origin = self.sub_scroll.origin(room.min_y());
        let lift = ((1.0 - fade) * px(4.0)).round();
        let layer = scene.push_layer(Some(room));
        for (j, entry) in entries.iter().enumerate() {
            let row = Rect::new(row_x, origin + tops[j] + lift, row_w, px(ITEM_ROW));
            let look = RowLook {
                lit: selected == Some(j),
                under: self.under == Some(Under::SubItem(j)),
                open: false,
                disabled: self.nav.disabled(entry).map(|why| why.map(str::to_owned)),
            };
            if paint_entry(
                &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
                entry,
                row,
                room,
                look,
            ) {
                self.sub_rows.push((row, j));
            }
        }
        let bar_room =
            Rect::new(room.min_x(), room.min_y(), third.size.width.min(full), room.size.height);
        self.sub_bar.paint(
            scene,
            p,
            self.scale,
            bar_room,
            self.sub_scroll.content(),
            self.sub_scroll.offset(),
        );
    }
}

/// How a submenu's row is lit: the keyboard's, the mouse's, its own submenu open; greyed
/// out when it doesn't apply now (with why, told while it's lit or under the mouse).
#[derive(Debug, Clone)]
struct RowLook {
    lit: bool,
    under: bool,
    open: bool,
    disabled: Option<Option<String>>,
}

/// Each row's top from the list's top (separators are shorter), and the list's height.
fn row_tops(entries: &[MenuEntry], ctx: &Ctx<'_>) -> (Vec<f32>, f32) {
    let (row_h, sep_h) = (ctx.px(ITEM_ROW), ctx.px(SEPARATOR));
    let mut tops = Vec::with_capacity(entries.len());
    let mut y = ctx.px(2.0);
    for entry in entries {
        tops.push(y);
        y += if matches!(entry, MenuEntry::Separator) { sep_h } else { row_h };
    }
    (tops, y + ctx.px(2.0))
}

/// A submenu's row in `row` (a separator in its upper part), if it's in sight in `room`:
/// its background, icon, label and shortcut — or a chevron, a row with a submenu of its
/// own. `true` if it's a row the mouse can be over (drawn, not a separator).
fn paint_entry(
    pen: &mut ui_components::LayerPainter<'_>,
    entry: &MenuEntry,
    row: Rect,
    room: Rect,
    look: RowLook,
) -> bool {
    let (layer, text, icons, ctx) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let px = |v: f32| ctx.px(v);
    let p = ctx.palette;
    let (top, row_h) = (row.min_y(), row.size.height);
    let h = if matches!(entry, MenuEntry::Separator) { px(SEPARATOR) } else { row_h };
    if top + h < room.min_y() || top > room.max_y() {
        return false;
    }
    let (label, shortcut, icon, submenu) = match entry {
        MenuEntry::Separator => {
            let y = (top + px(SEPARATOR) / 2.0).round();
            ds::divider(layer, ctx, row.min_x() + px(10.0), row.max_x() - px(10.0), y);
            return false;
        }
        MenuEntry::Item { label, shortcut, icon, .. } => (label, shortcut.as_deref(), *icon, false),
        MenuEntry::Submenu { label, icon, .. } => (label, None, *icon, true),
    };
    if look.open {
        paint_open_mark(layer, ctx, row);
    } else {
        paint_row_background(layer, ctx, row, look.lit, look.under);
    }
    // Greyed out: its icon, label and keys faded, never in the accent; lit or under the
    // mouse, why it doesn't apply in its keys' place.
    let faded = ctx.with_opacity(ctx.opacity * DISABLED);
    let disabled = look.disabled.is_some();
    let ctx = if disabled { &faded } else { ctx };
    let why = look.disabled.flatten().filter(|_| look.lit || look.under);
    let lit = look.lit && !look.open && !disabled;
    paint_icon(layer, icons, ctx, icon, row, (px(ICON), px(ICON_AT)), lit);
    let font = ctx.ui_font(-0.5);
    let key_font = ctx.ui_font(-1.0);
    let mut right = row.max_x() - px(10.0);
    if submenu {
        if let Some(id) = icons.id("chevron-right") {
            let size = px(13.0);
            let at =
                Rect::new((right - size).round(), (top + (row_h - size) / 2.0).round(), size, size);
            let color = if look.open {
                p.accent
            } else if look.lit {
                p.text
            } else {
                p.text_muted
            };
            draw_icon(layer, icons, id, at, ctx.fade(color));
            right = at.min_x() - px(10.0);
        }
    } else if let Some(key) = why.as_deref().or(shortcut) {
        let room = (row.size.width * 0.55).max(0.0);
        let key = ds::ellipsize(text, key, &key_font, room);
        let shaped = text.shape_line(&key, &key_font);
        let x = (right - shaped.width).round();
        draw_text(
            layer,
            &shaped,
            x,
            ds::optical_baseline(top, row_h, &key_font),
            ctx.fade(p.text_muted),
        );
        right = x - px(14.0);
    }
    let x = row.min_x() + px(LABEL_AT);
    let label = ds::ellipsize(text, label, &font, (right - x).max(0.0));
    let shaped = text.shape_line(&label, &font);
    draw_text(layer, &shaped, x, ds::optical_baseline(top, row_h, &font), ctx.fade(p.text));
    true
}

/// A row's background as the list modals light theirs: the keyboard's (or the open
/// category's) in the accent, the mouse's faintly.
fn paint_row_background(layer: &mut Layer, ctx: &Ctx<'_>, row: Rect, lit: bool, under: bool) {
    ds::RowHighlight::new(lit, under)
        .selected_alpha(ROW_LIT)
        .hover(ds::HoverFill::Faint(ROW_UNDER))
        .paint(layer, ctx, row);
}

/// A row lit (the accent's alpha), under the mouse (the text's), and the open category's
/// (the text's).
const ROW_LIT: f32 = 0.12;
const ROW_UNDER: f32 = 0.05;
const ROW_OPEN: f32 = 0.07;

/// A row's icon (`size`, at `inset` from its left edge), in the accent when lit.
fn paint_icon(
    layer: &mut Layer,
    icons: &Icons,
    ctx: &Ctx<'_>,
    name: Option<&'static str>,
    row: Rect,
    (size, inset): (f32, f32),
    lit: bool,
) {
    let Some(id) = name.and_then(|n| icons.id(n)) else { return };
    let p = ctx.palette;
    let at = Rect::new(
        row.min_x() + inset,
        (row.min_y() + (row.size.height - size) / 2.0).round(),
        size,
        size,
    );
    draw_icon(layer, icons, id, at, ctx.fade(if lit { p.accent } else { p.text_muted }));
}

/// The open category's row: a neutral fill and an accent mark on its left.
fn paint_open_mark(layer: &mut Layer, ctx: &Ctx<'_>, row: Rect) {
    let p = ctx.palette;
    ds::RowHighlight::new(false, true).hover(ds::HoverFill::Faint(ROW_OPEN)).paint(layer, ctx, row);
    let (w, h) = (ctx.px(3.0), (row.size.height * 0.5).round());
    let mark = Rect::new(
        row.min_x() + ctx.px(2.0),
        (row.min_y() + (row.size.height - h) / 2.0).round(),
        w,
        h,
    );
    layer.quad(Quad::filled(mark, ctx.fade(p.accent)).rounded(w / 2.0));
}
