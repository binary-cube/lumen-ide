//! The consent window, before every recording (`.claude/rules/privacy.md`): what's recorded
//! and what never is, said plainly; how long it may run (our dropdown: the field and the
//! shared picker under it); its footer with the keys on the left and the buttons on the
//! right (`.claude/rules/modals.md`). "Start recording" starts it and closes the window;
//! Esc or Cancel leave without anything recorded.

use std::sync::Arc;
use std::time::Duration;

use i18n::t;
use kernel::{Point, Rect};
use ui_components::control::{Control, Controls};
use ui_components::ds::{self, Ctx};
use ui_components::picker::{Picker, PickerEvent, PickerItem};
use ui_components::{
    Block, BlockKind, KeyReach, ReadText, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerPaint,
};
use workbench_model::{Notification, ResourceData, ResourceSource};

use crate::recording::{DEFAULT_LENGTH, LENGTHS};

/// The content's padding and the length row's height (logical pixels).
const PAD: f32 = 24.0;
const ROW_H: f32 = 36.0;
const FIELD_W: f32 = 200.0;
/// The content's height (logical pixels): its text, the length row, the footer.
const HEIGHT: f32 = 420.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Length,
    Cancel,
    Start,
}

/// The consent window's view.
pub struct ConsentView {
    text: ReadText,
    /// The length chosen (its place in [`LENGTHS`]), its field's list.
    length: usize,
    picker: Picker,
    field: Rect,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    /// Its parts' state of use (see [`ConsentView::with_controls`]).
    controls: Controls,
    resources: Option<Arc<dyn ResourceSource>>,
    action: Option<ViewerAction>,
    notifications: Vec<Notification>,
}

impl std::fmt::Debug for ConsentView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsentView").field("length", &self.length).finish_non_exhaustive()
    }
}

impl Default for ConsentView {
    fn default() -> Self {
        Self::new()
    }
}

/// What the window says: why, what's recorded, what never is, where it goes.
fn blocks() -> Vec<Block> {
    let point = |key: &str| Block::new(BlockKind::Point { depth: 0 }, t(key));
    vec![
        Block::new(BlockKind::Paragraph, t("profiler.consent_intro")),
        Block::new(BlockKind::Gap, ""),
        Block::new(BlockKind::Heading, t("profiler.consent_kept")),
        point("profiler.consent_kept_app"),
        point("profiler.consent_kept_system"),
        Block::new(BlockKind::Gap, ""),
        Block::new(BlockKind::Heading, t("profiler.consent_never")),
        point("profiler.consent_never_data"),
        point("profiler.consent_never_sent"),
    ]
}

impl ConsentView {
    pub fn new() -> Self {
        Self {
            text: ReadText::new(blocks()),
            length: DEFAULT_LENGTH,
            picker: Picker::default(),
            field: Rect::default(),
            hits: Vec::new(),
            hover: None,
            controls: Controls::default(),
            resources: None,
            action: None,
            notifications: Vec::new(),
        }
    }

    /// Its parts (the length's list, over everything while open), with their state of use.
    /// The list opens from its field and is given the focus then: its keys, Tab among them.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut parts: [&mut dyn Control; 1] = [&mut self.picker];
        f(&mut self.controls, &mut parts)
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    fn length_label(i: usize) -> String {
        LENGTHS.get(i).map_or_else(String::new, |(_, key)| t(key))
    }

    /// The recording started (the window closes), or why not (said).
    fn start(&mut self) {
        let seconds = LENGTHS.get(self.length).map_or(300, |(s, _)| *s);
        match crate::recording::start(Duration::from_secs(seconds), self.resources.clone()) {
            Ok(()) => self.action = Some(ViewerAction::Close),
            Err(why) => {
                tracing::warn!(error = %why, "the profile recording didn't start");
                self.notifications.push(Notification::warning(i18n::tr!(
                    "profiler.not_started",
                    why = why.as_str()
                )));
            }
        }
    }

    /// What a control does.
    fn act(&mut self, hit: Hit) {
        match hit {
            Hit::Length => {
                let items = (0..LENGTHS.len())
                    .map(|i| PickerItem { label: Self::length_label(i), ..PickerItem::default() })
                    .collect();
                self.picker.open(items, Some(self.length));
            }
            Hit::Cancel => self.action = Some(ViewerAction::Close),
            Hit::Start => self.start(),
        }
    }

    fn picked(&mut self, event: Option<PickerEvent>) {
        if let Some(PickerEvent::Chosen(i)) = event {
            self.length = i;
        }
    }
}

impl Viewer for ConsentView {
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette, icons, .. } = ctx;
        let ds = Ctx::new(palette, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let foot_h = px(ds::MODAL_FOOTER_H);
        let foot = Rect::new(b.min_x(), b.max_y() - foot_h, b.size.width, foot_h);
        let inner = Rect::new(
            b.min_x() + px(PAD),
            b.min_y() + px(PAD) * 0.6,
            b.size.width - 2.0 * px(PAD),
            (foot.min_y() - b.min_y() - px(PAD) - px(ROW_H) - px(16.0)).max(0.0),
        );
        self.text.paint(scene, text, &ds, inner);
        // The length: its label, its field.
        let row = Rect::new(inner.min_x(), inner.max_y() + px(8.0), inner.size.width, px(ROW_H));
        let layer = scene.push_layer(Some(b));
        let font = ds.ui_font(0.0);
        let label = text.shape_line(&t("profiler.length"), &font);
        ui_kit::draw_text(
            layer,
            &label,
            row.min_x(),
            ds::optical_baseline(row.min_y(), row.size.height, &font),
            palette.text,
        );
        self.field =
            Rect::new(row.max_x() - px(FIELD_W), row.min_y(), px(FIELD_W), row.size.height);
        let value = Self::length_label(self.length);
        let spec = ds::DropdownSpec {
            value: &value,
            open: self.picker.is_open(),
            hovered: self.hover == Some(Hit::Length),
            enabled: true,
            note: None,
            placeholder: None,
        };
        ds::dropdown(layer, text, icons, &ds, self.field, spec);
        self.hits.push((self.field, Hit::Length));
        // The footer: the keys on the left, the buttons on the right.
        let layer = ds::modal_footer_bar(scene, &ds, foot);
        let mid = foot.min_y() + foot.size.height / 2.0;
        let start_label = t("profiler.start");
        let cancel_label = t("profiler.cancel");
        let buttons = [
            ds::FooterButton {
                label: &start_label,
                main: true,
                enabled: true,
                hovered: self.hover == Some(Hit::Start),
                ..ds::FooterButton::default()
            },
            ds::FooterButton {
                label: &cancel_label,
                enabled: true,
                hovered: self.hover == Some(Hit::Cancel),
                ..ds::FooterButton::default()
            },
        ];
        let right = foot.max_x() - px(PAD);
        let (areas, left) = ds::footer_buttons(layer, text, &ds, right, mid, &buttons);
        self.hits.extend(areas.into_iter().zip([Hit::Start, Hit::Cancel]));
        let hints = self.key_hints();
        let hints: Vec<ui_kit::KeyHint<'_>> =
            hints.iter().map(|(k, w)| (k.as_slice(), w.as_str())).collect();
        let keys = Rect::new(
            foot.min_x() + px(PAD),
            foot.min_y(),
            (left - foot.min_x() - px(PAD)).max(0.0),
            foot.size.height,
        );
        ui_kit::paint_key_hints(layer, text, &hints, keys, &palette.key_hints(scale));
        // The length's list, over everything.
        if self.picker.is_open() {
            self.picker.paint(scene, text, icons, &ds, (self.field, b), "");
        }
    }

    fn draws_footer(&self) -> bool {
        true
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], t("profiler.start")), (vec!["esc"], t("profiler.cancel"))]
    }

    fn modal_height(&self) -> Option<f32> {
        Some(HEIGHT)
    }

    fn modal_width(&self) -> Option<f32> {
        Some(560.0)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some(crate::ICON)
    }

    fn subtitle(&self) -> Option<String> {
        Some(t("profiler.consent_subtitle"))
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(ResourceData(resources)) = data.downcast_ref::<ResourceData>() {
            self.resources = Some(Arc::clone(resources));
        }
    }

    /// A click (as a tab or a modal window sends it: both come here).
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // A part takes it (an open list: a length chosen, or closed by a press outside).
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            let event = self.picker.take_event();
            self.picked(event);
            return None;
        }
        if let Some(hit) = self.hit(p) {
            self.act(hit);
        }
        // A list opened: it has the keys, and the mouse (there) is over it.
        if self.picker.is_open() {
            self.with_controls(|c, parts| {
                c.focus(parts, Some(0));
                c.hover(parts, Some(p));
            });
        }
        self.action.take()
    }

    fn hover(&mut self, p: Point) -> bool {
        // A part under the mouse (an open list) takes it over what's under it.
        let over = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        if self.controls.hovered().is_some() {
            return over;
        }
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // A part under the mouse (the length's list open): its own cursor, never the text's
        // under it.
        if self.controls.hovered().is_some() {
            return Some(self.controls.cursor_at(p).unwrap_or(ViewerCursor::Arrow));
        }
        Some(self.hit(p).map_or(ViewerCursor::Arrow, |_| ViewerCursor::Pointer))
    }

    /// The wheel to the part under the mouse (the open list).
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.with_controls(|c, parts| c.scroll_hovered(parts, dx, dy)).is_some()
    }

    /// A part holding the pointer (the open list's thumb) follows it.
    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, changed)| changed)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // The part with the focus (the open list: every key its own).
        if self.with_controls(|c, parts| c.key(parts, key, reach)).is_some() {
            let event = self.picker.take_event();
            self.picked(event);
            return true;
        }
        match key {
            ViewerKey::Enter => {
                self.start();
                true
            }
            ViewerKey::Escape => {
                self.action = Some(ViewerAction::Close);
                true
            }
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }
}

#[cfg(test)]
#[path = "../tests/unit/consent.rs"]
mod tests;
