//! The bar painted: its tools by the layout, the pills, the notes on the right (read-only,
//! limited results) and what the last run did.

use super::*;

/// A notice's chip's height, the room on each side, its fill's and its outline's alpha of
/// its color (logical pixels).
const NOTICE_H: f32 = 22.0;
const NOTICE_PAD: f32 = 8.0;
const NOTICE_FILL: f32 = 0.10;
const NOTICE_BORDER: f32 = 0.35;

/// A notice of the bar (limited, read-only, production): its icon and label in `color`, in
/// a pill outlined in it.
fn notice_badge<'a>(
    label: &'a str,
    icon: &'a str,
    color: Rgba,
    font: &'a Font,
    icons: &'a icons::Icons,
) -> ds::Badge<'a> {
    ds::Badge::new(label)
        .color(color)
        .fill_alpha(NOTICE_FILL)
        .border(color.with_alpha(NOTICE_BORDER))
        .size(ds::BadgeSize::Custom(NOTICE_H))
        .pad(NOTICE_PAD)
        .font(font)
        .icon(icons, icon)
}

impl ConsoleBar {
    pub(super) fn paint_bar(&mut self, ctx: ToolbarPaint<'_>) {
        let ToolbarPaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let mid = (b.min_y() + b.size.height / 2.0).round();
        // The line under it, between the bar and the text.
        ds::divider(layer, &ds, b.min_x(), b.max_x(), b.max_y() - ds.line());

        let font = ds.ui_font(-1.0);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let tx_label = self.tx_label();
        let mode_label = i18n::t(match self.spec.mode {
            RunMode::Playground => "console.mode_playground",
            RunMode::Script => "console.mode_script",
        });
        let place_label = self.place_label();
        // `Tx:` muted before the mode; an open transaction's dot (red: failed) before it all.
        let tx_prefix = i18n::tr!("console.tx_label", mode = "").trim().to_owned();
        let tx_value = tx_label.strip_prefix(tx_prefix.as_str()).unwrap_or(&tx_label).trim_start();
        let tx_dot = match self.snapshot.tx {
            TxState::Failed => Some(p.danger),
            _ if self.snapshot.tx.is_open() => Some(p.warning),
            _ => None,
        };
        let tx_look = ds::DropdownLook {
            prefix: Some(&tx_prefix),
            dot: tx_dot,
            compact: true,
            ..ds::DropdownLook::default()
        };
        let compact = ds::DropdownLook { compact: true, ..ds::DropdownLook::default() };
        let status = self.status();
        let status_w = status.as_ref().map(|(s, _)| text.shape_line(s, &font).width + px(22.0));
        let limited = self.snapshot.limited || self.capabilities().limited;
        let limited_label = i18n::t("console.limited");
        let read_only_label = i18n::t("console.read_only");
        let production_label = i18n::t("console.production_badge");
        let small = ds.ui_font(-1.5);
        let bold = small.clone().with_weight(Font::BOLD);
        let notice = |tool: Tool| match tool {
            Tool::Limited => notice_badge(&limited_label, "info", p.warning, &small, icons),
            Tool::Production => notice_badge(&production_label, "alert", p.danger, &bold, icons),
            _ => notice_badge(&read_only_label, "lock", p.text_muted, &small, icons),
        };
        let limited_w = limited.then(|| notice(Tool::Limited).width(text, &ds));
        let read_only = self.brief.as_ref().is_some_and(|s| s.read_only);
        let read_only_w = read_only.then(|| notice(Tool::ReadOnly).width(text, &ds));
        let production = self.production();
        let production_w = production.then(|| notice(Tool::Production).width(text, &ds));
        let source_label = self.spec.is_attached().then(|| self.source_label());
        let mut with_icon = |value: &str| {
            ds::dropdown_width_with_icon(text, &ds, (pill_spec(value, false, true), compact))
        };
        let place_w = with_icon(&place_label);
        let source_w = source_label.as_deref().map(&mut with_icon);
        let spec = BarSpec {
            min_x: b.min_x(),
            max_x: b.max_x(),
            metrics: Metrics::new(px),
            transaction: self.snapshot.in_transaction,
            tx_w: ds::dropdown_width(text, &ds, (pill_spec(tx_value, false, true), tx_look)),
            mode_w: ds::dropdown_width(text, &ds, (pill_spec(&mode_label, false, true), compact)),
            place_w,
            status_w,
            limited_w,
            read_only_w,
            production_w,
            source_w,
        };
        let key: LayoutKey = (
            spec.min_x.to_bits(),
            spec.max_x.to_bits(),
            spec.transaction,
            spec.tx_w.to_bits(),
            spec.mode_w.to_bits(),
            spec.place_w.to_bits(),
            spec.status_w.map(f32::to_bits),
            spec.limited_w.map(f32::to_bits),
            spec.read_only_w.map(f32::to_bits),
            spec.production_w.map(f32::to_bits),
            spec.source_w.map(f32::to_bits),
        );
        let laid = self.layout.get_or_update(key, || layout::layout(&spec)).clone();
        self.hits.clear();
        for x in &laid.dividers {
            ds::divider_vertical(layer, &ds, *x, mid - px(9.0), mid + px(9.0));
        }
        let green = p.vcs_added;
        for &(tool, x, w) in &laid.placed {
            let enabled = self.enabled(tool);
            let hovered = self.hover == Some(tool) && enabled;
            let r = Rect::new(x, (mid - px(14.0)).round(), w, px(28.0));
            match tool {
                Tool::Run
                | Tool::History
                | Tool::Params
                | Tool::Commit
                | Tool::Rollback
                | Tool::Stop
                | Tool::Settings
                | Tool::Services
                | Tool::More => {
                    let (icon, tone) = match tool {
                        Tool::Run | Tool::Commit => {
                            (if tool == Tool::Run { "play" } else { "check" }, Some(Tone::Success))
                        }
                        Tool::History => ("clock", None),
                        Tool::Params => ("parameter", None),
                        Tool::Rollback => ("undo", None),
                        Tool::Stop => ("stop", Some(Tone::Danger)),
                        Tool::Settings => ("gear", None),
                        Tool::Services => ("services", None),
                        _ => ("more-horizontal", None),
                    };
                    let mut spec = ButtonSpec::new(ButtonKind::Ghost)
                        .icon(icon)
                        .enabled(enabled)
                        .hovered(hovered);
                    if let Some(tone) = tone {
                        spec = spec.tone(tone);
                    }
                    ds::button(layer, text, icons, &ds, r, spec);
                }
                Tool::RunMenu => {
                    // Its chevron drawn small (a split button's half), its hit the bar's height.
                    let back = Rect::new(
                        r.min_x(),
                        (mid - px(button_size::SMALL) / 2.0).round(),
                        w,
                        px(button_size::SMALL),
                    );
                    let spec = ButtonSpec::new(ButtonKind::Ghost)
                        .icon("chevron-down")
                        .tone(Tone::Neutral)
                        .enabled(enabled)
                        .hovered(hovered);
                    ds::button(layer, text, icons, &ds, back, spec);
                }
                Tool::Tx | Tool::Mode => {
                    let r = Rect::new(x, (mid - px(PILL_H) / 2.0).round(), w, px(PILL_H));
                    let field = match tool {
                        Tool::Tx => (pill_spec(tx_value, hovered, enabled), tx_look),
                        _ => (pill_spec(&mode_label, hovered, enabled), compact),
                    };
                    ds::dropdown_styled(layer, text, icons, &ds, r, field);
                    self.hits.push((r, tool));
                    continue;
                }
                Tool::Place | Tool::Source => {
                    let value = match (tool, &source_label) {
                        (Tool::Source, Some(label)) => label.as_str(),
                        (Tool::Source, None) => continue,
                        _ => place_label.as_str(),
                    };
                    let r = Rect::new(x, (mid - px(PILL_H) / 2.0).round(), w, px(PILL_H));
                    // An attached file's source pill has the driver's icon.
                    let icon = match tool {
                        Tool::Place if self.spec.is_attached() => "database",
                        _ => self.brief.as_ref().map_or("database", |b| b.icon),
                    };
                    let field = (pill_spec(value, hovered, enabled), compact);
                    ds::dropdown_with_icon(layer, text, icons, &ds, r, field, icon);
                    self.hits.push((r, tool));
                    continue;
                }
                Tool::Status => {
                    let Some((label, error)) = &status else { continue };
                    let running = self.snapshot.running.is_some();
                    let icon_x = x;
                    let s = px(13.0);
                    if running {
                        ds::loader(
                            layer,
                            text,
                            &ds,
                            icon_x,
                            mid,
                            &ds::Loader { size: 13.0, ..Default::default() },
                        );
                    } else {
                        let (icon, color) = if *error {
                            ("circle-alert", p.danger)
                        } else if self.snapshot.last.as_ref().is_some_and(|l| l.cancelled) {
                            ("square", p.text_muted)
                        } else {
                            ("check", green)
                        };
                        if let Some(id) = icons.id(icon) {
                            let ir = Rect::new(icon_x.round(), (mid - s / 2.0).round(), s, s);
                            draw_icon(layer, icons, id, ir, color);
                        }
                    }
                    let room = laid.status_room - px(22.0);
                    let line = ds::ellipsize(text, label, &font, room.max(0.0));
                    let shaped = text.shape_line(&line, &font);
                    let base = ds::optical_baseline(r.min_y(), r.size.height, &font);
                    let color = if *error { p.danger } else { p.text_muted };
                    draw_text(layer, &shaped, (x + px(20.0)).round(), base, color);
                    self.hits.push((r, tool));
                    continue;
                }
                Tool::Limited | Tool::ReadOnly | Tool::Production => {
                    let r = notice(tool).paint(layer, text, &ds, x, mid);
                    self.hits.push((r, tool));
                    continue;
                }
            }
            if enabled || tool == Tool::Params {
                self.hits.push((r, tool));
            }
        }
        let _ = strong;
    }
}
