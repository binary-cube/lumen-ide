//! The notifications (toasts) of the window's project: kept in its data
//! (`ProjectData::notifications`, gone with it), told by the modules through the host
//! (`host.notify`) or, from any thread, through the project's [`ProjectNotifier`]
//! (`ProjectContext::notifier`); drawn by `ui_components::toast` as a floating overlay —
//! over everything, modals too (a modal's confirmation shows), taking only the clicks and
//! the hover on its own cards, never the keyboard.
//!
//! Time passes only while frames are drawn (a hidden or minimized window draws none: its
//! notifications wait) and stands still while the mouse is over the stack. While nothing is
//! shown nothing is done; while a ring counts down, a frame is asked for only when its next
//! dot would change.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use workbench_model::{Notification, NotificationId, NotificationKind, Notifier};

use super::overlay_stack::{OverlayKind, OverlaySlot};
use super::*;

/// The longest step time takes between two frames: more (the window was hidden, the
/// machine slept) isn't counted.
const MAX_STEP: Duration = Duration::from_millis(100);

/// What a notifier was asked, waiting for the UI thread.
#[derive(Debug)]
enum Told {
    Show(Notification),
    Update(Notification),
    Dismiss(NotificationId),
}

/// The project's notifier for its modules' threads: what it's told waits here until the
/// window's next frame (it wakes the window).
#[derive(Default)]
pub(super) struct ProjectNotifier {
    told: Mutex<Vec<Told>>,
    /// Something waits in `told` (looked at every frame without locking).
    pending: std::sync::atomic::AtomicBool,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl std::fmt::Debug for ProjectNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectNotifier").finish_non_exhaustive()
    }
}

impl ProjectNotifier {
    /// Wakes the window with `wake` when told something.
    pub fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        *self.wake.lock().unwrap_or_else(PoisonError::into_inner) = Some(wake);
    }

    fn tell(&self, told: Told) {
        self.told.lock().unwrap_or_else(PoisonError::into_inner).push(told);
        self.pending.store(true, std::sync::atomic::Ordering::Release);
        let wake = self.wake.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    fn take(&self) -> Vec<Told> {
        if !self.pending.swap(false, std::sync::atomic::Ordering::Acquire) {
            return Vec::new();
        }
        std::mem::take(&mut *self.told.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Notifier for ProjectNotifier {
    fn show(&self, notification: Notification) {
        self.tell(Told::Show(notification));
    }

    fn update(&self, notification: Notification) {
        self.tell(Told::Update(notification));
    }

    fn dismiss(&self, id: NotificationId) {
        self.tell(Told::Dismiss(id));
    }
}

impl Workbench {
    /// Shows `notification` (the host's `notify`).
    pub(super) fn show_notification(&mut self, notification: Notification) {
        tracing::debug!(kind = ?notification.kind, title = %notification.title, "notification");
        self.data.notifications.show(notification, Instant::now());
        self.toast_clock.get_or_insert_with(Instant::now);
    }

    pub(super) fn update_notification(&mut self, notification: Notification) {
        self.data.notifications.update(notification, Instant::now());
        self.toast_clock.get_or_insert_with(Instant::now);
    }

    pub(super) fn dismiss_notification(&mut self, id: NotificationId) -> bool {
        self.data.notifications.dismiss(id)
    }

    /// "Close Notifications": every one shown or waiting. `true` if there was one.
    pub fn dismiss_notifications(&mut self) -> bool {
        self.data.notifications.dismiss_all()
    }

    /// What the project's modules told from their threads, shown. `true`: a redraw.
    pub(super) fn poll_notifications(&mut self) -> bool {
        let told = self.data.notifier.take();
        // What a module's background work told the whole app: this window shows it.
        let posted = workbench_model::app_wide::take_posted();
        let any = !told.is_empty() || !posted.is_empty();
        for n in posted {
            self.show_notification(n);
        }
        for told in told {
            match told {
                Told::Show(n) => self.show_notification(n),
                Told::Update(n) => self.update_notification(n),
                Told::Dismiss(id) => {
                    self.dismiss_notification(id);
                }
            }
        }
        any
    }

    /// Time passes for the notifications shown (since the last frame, at most
    /// [`MAX_STEP`]; standing still under the mouse).
    fn tick_toasts(&mut self) {
        if self.data.notifications.is_empty() {
            self.toast_clock = None;
            return;
        }
        let now = Instant::now();
        let last = self.toast_clock.replace(now).unwrap_or(now);
        let step = now.saturating_duration_since(last).min(MAX_STEP);
        self.data.notifications.advance(step, self.toasts.hovered());
    }

    /// Where the stack goes: under the tab bar or above the bottom bar, across the editor.
    fn toast_place(&self) -> ui_workbench::toast::StackPlace {
        let s = self.scale;
        let gap = (ui_workbench::ds::POPOVER_GAP * s).round();
        let top = self.card.min_y() + TabBar::height(s) + gap;
        let bottom = self.size.height as f32 - StatusBar::height(s) - gap;
        let area = Rect::new(self.card.min_x(), top, self.card.size.width, (bottom - top).max(0.0));
        let position = self.toast_position.unwrap_or_else(crate::toasts::position);
        ui_workbench::toast::StackPlace { area, position, scale: s }
    }

    /// What the views told the user (the active tab's, the modal's), shown.
    fn take_view_notifications(&mut self) {
        let mut told = Vec::new();
        if let Some(v) = self.tabs.get_mut(self.active).and_then(|t| t.viewer.as_mut()) {
            told.extend(v.viewer.take_notifications());
        }
        if let Some(modal) = &mut self.modal_view {
            told.extend(modal.viewer.take_notifications());
        }
        // The panels' (their views': the Services window's tabs).
        for side in &mut self.tools.sides {
            told.extend(side.panel.take_notifications());
        }
        for notification in told {
            self.show_notification(notification);
        }
    }

    /// What every view told the user (a background export ended in a tab not shown).
    pub(super) fn poll_view_notifications(&mut self) {
        let mut told = Vec::new();
        for tab in all_tabs_mut(&mut self.tabs, &mut self.editors.groups) {
            if let Some(v) = &mut tab.viewer {
                told.extend(v.viewer.take_notifications());
            }
        }
        for notification in told {
            self.show_notification(notification);
        }
    }

    /// Draws the stack (over everything); nothing at all while it's empty.
    pub(super) fn paint_toasts(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        self.take_view_notifications();
        self.poll_notifications();
        self.tick_toasts();
        let place = self.toast_place();
        let painter =
            ui_workbench::Painter { text, scene, palette: &self.palette, icons: &self.icons };
        self.toasts.paint(painter, &self.data.notifications, place);
    }

    /// When the next frame is needed for the stack (a ring's next step, a spinner's next
    /// step); `None`: nothing counts down.
    pub fn toast_deadline(&self) -> Option<Instant> {
        if !self.data.notifications.ticking() {
            return None;
        }
        let paused = self.toasts.hovered();
        let step = self
            .data
            .notifications
            .shown()
            .filter(|t| !t.is_leaving())
            .filter_map(|t| match t.notification.kind {
                // Its bar sweeping: smooth (about 60 a second, only while it shows).
                NotificationKind::Progress => Some(ui_workbench::toast::PROGRESS_FRAME),
                _ if paused => None,
                _ => t
                    .notification
                    .duration()
                    .map(|d| d / u32::from(ui_workbench::toast::RING_STEPS)),
            })
            .min()?;
        Some(Instant::now() + step.max(ui_workbench::toast::PROGRESS_FRAME))
    }

    /// The notifications' history (newest first) in a list; a row chosen shows it again.
    pub fn show_recent_notifications(&mut self) -> bool {
        let recent: Vec<workbench_model::Recent> =
            self.data.notifications.history().cloned().collect();
        if recent.is_empty() {
            self.show_notification(Notification::info(i18n::t("notifications.none")));
            return true;
        }
        let now = Instant::now();
        let items = recent
            .iter()
            .map(|r| {
                let title = if r.count > 1 {
                    format!("{} ×{}", r.title, r.count)
                } else {
                    r.title.clone()
                };
                let ago = ago(now.saturating_duration_since(r.at));
                let detail = match &r.detail {
                    Some(detail) => format!("{detail} · {ago}"),
                    None => ago,
                };
                workbench_model::ChoiceItem {
                    icon: ui_workbench::toast::kind_icon(r.kind),
                    label: title,
                    detail,
                }
            })
            .collect();
        let list = workbench_model::ChoiceList {
            title: i18n::t("notifications.recent_title"),
            subject: i18n::tr!("notifications.recent_count", count = recent.len()),
            icon: "bell",
            action: i18n::t("notifications.show_again"),
            items,
        };
        workbench_model::WorkbenchHost::choose(
            self,
            list,
            Box::new(move |i, host| {
                if let Some(r) = recent.get(i) {
                    let mut again = Notification::new(r.kind, r.title.clone());
                    if let Some(detail) = &r.detail {
                        again = again.with_detail(detail.clone());
                    }
                    // Work in progress seen again is only what it said.
                    if r.kind == NotificationKind::Progress {
                        again.kind = NotificationKind::Info;
                    }
                    host.notify(again);
                }
            }),
        )
    }

    /// The newest notification's title (the tests' look at what the user was told).
    #[cfg(test)]
    pub(super) fn last_notification(&self) -> Option<String> {
        self.data.notifications.history().next().map(|r| r.title.clone())
    }

    /// The notifications told so far shown, their transitions over and their time stopped a
    /// third of the way (snapshots: the same picture every run).
    #[cfg(feature = "snapshot")]
    pub fn settle_notifications(&mut self) {
        self.take_view_notifications();
        self.poll_view_notifications();
        self.poll_notifications();
        let notifications = &mut self.data.notifications;
        notifications.freeze(true);
        let timed: Vec<_> = notifications
            .shown()
            .filter_map(|t| Some((t.id(), t.notification.duration()?)))
            .collect();
        for (id, duration) in timed {
            notifications.set_elapsed(id, duration / 3);
        }
    }

    /// Shows this window's notifications at `position` whatever the setting (the tests).
    #[cfg(test)]
    pub(super) fn place_toasts_at(&mut self, position: workbench_model::ToastPosition) {
        self.toast_position = Some(position);
    }

    /// Whether `p` is on a notification's card.
    pub fn over_toast(&self, p: Point) -> bool {
        self.floating_at(p).is_some()
    }

    /// Esc with the mouse over the stack: they close. `true` if there were some.
    pub(super) fn escape_toasts(&mut self) -> bool {
        self.toasts.hovered() && self.dismiss_notifications()
    }
}

/// How long ago, in words.
fn ago(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => i18n::t("notifications.just_now"),
        m if m < 60 => i18n::tr!("notifications.minutes_ago", count = m),
        m => i18n::tr!("notifications.hours_ago", count = m / 60),
    }
}

/// The stack as an overlay: on top of every other, but floating — never the one with the
/// keyboard, never blocking what's under it; only a click or the mouse on its own cards
/// reaches it.
pub(super) struct ToastsSlot;

impl OverlaySlot for ToastsSlot {
    /// A notification's ring or spinner: a frame when it next changes (no faster than about
    /// 30 a second), none while nothing counts down.
    fn frame_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.toast_deadline()
    }

    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        !wb.data.notifications.is_empty()
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    fn floats(&self, _wb: &Workbench) -> bool {
        true
    }

    fn holds(&self, wb: &Workbench, p: Point) -> bool {
        wb.toasts.contains(p)
    }

    fn area(&self, wb: &Workbench) -> Option<kernel::Rect> {
        wb.toasts.bounds()
    }

    /// × closes its card, an action runs its command (and closes it); elsewhere on a card
    /// nothing happens.
    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        use ui_workbench::toast::ToastHit;
        match wb.toasts.hit(p) {
            Some(ToastHit::Close(id)) => {
                wb.dismiss_notification(id);
                MouseResult::Redraw
            }
            Some(ToastHit::Action(id, command)) => {
                wb.dismiss_notification(id);
                MouseResult::Command(command.0)
            }
            Some(ToastHit::Card(_)) | None => MouseResult::None,
        }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        let changed = wb.toasts.hover(p);
        let icon = if wb.toasts.over_button() { CursorIcon::Pointer } else { CursorIcon::Default };
        (changed, icon)
    }

    /// Nothing scrolls under it.
    fn scroll(&self, _wb: &mut Workbench, _dx: f32, _dy: f32) -> Option<bool> {
        Some(false)
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.paint_toasts(text, scene);
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.data.notifications.transitioning()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/toasts.rs"]
mod tests;
