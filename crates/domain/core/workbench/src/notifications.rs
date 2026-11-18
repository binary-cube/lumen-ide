//! Notifications (toasts): short messages floating over the workbench that go away by
//! themselves — a success, an information, a warning, an error, or work in progress that
//! turns into a success or an error when it ends. What a module tells the user goes
//! through the [`Notifier`] port (`host.notify`, `ProjectContext::notifier`); it never
//! draws a toast itself.
//!
//! [`Notifications`] holds a project's stack and its pure rules: identical messages merged
//! (`×2`) instead of filling the stack, at most [`MAX_VISIBLE`] shown (the rest wait, in
//! order), how long each kind stays ([`Notification::duration`]), time standing still while
//! the mouse is over the stack, the enter / exit transitions, and the last [`HISTORY`]
//! shown. Time is given to it ([`Notifications::advance`]): nothing here reads a clock but
//! the moment a message is kept in the history.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use command_model::CommandId;

/// A notification's identity (a UUID v4, `kernel::id::new_uuid`: never repeats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NotificationId(pub u128);

impl NotificationId {
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }
}

impl Default for NotificationId {
    fn default() -> Self {
        Self::new()
    }
}

/// What a notification says: its icon, its color and how long it stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationKind {
    Success,
    Info,
    Warning,
    Error,
    /// Work going on (a spinner): it stays until it's updated to how it ended.
    Progress,
}

impl NotificationKind {
    pub const ALL: [Self; 5] =
        [Self::Success, Self::Info, Self::Warning, Self::Error, Self::Progress];
}

/// A button of a notification: its label and the command it runs (its keys work as
/// anywhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAction {
    pub label: String,
    pub command: CommandId,
}

/// How long a notification stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Timeout {
    /// Its kind's: [`SHORT`] for a success or an information, [`LONG`] for a warning;
    /// errors, work in progress and notifications with actions stay until closed.
    #[default]
    Default,
    /// Until it's closed.
    Never,
    After(Duration),
}

/// How long a success or an information stays.
pub const SHORT: Duration = Duration::from_secs(4);
/// How long a warning stays.
pub const LONG: Duration = Duration::from_secs(8);
/// At most this many shown at once; the others wait.
pub const MAX_VISIBLE: usize = 4;
/// At most this many actions on one.
pub const MAX_ACTIONS: usize = 2;
/// How many the history keeps.
pub const HISTORY: usize = 50;
/// The enter and exit transitions.
pub const ENTER: Duration = Duration::from_millis(220);
pub const EXIT: Duration = Duration::from_millis(180);

/// A message for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: NotificationId,
    pub kind: NotificationKind,
    pub title: String,
    /// A line under the title (why it failed, where it went).
    pub detail: Option<String>,
    /// At most [`MAX_ACTIONS`].
    pub actions: Vec<NotificationAction>,
    pub timeout: Timeout,
    /// What makes two notifications the same one (a later one replaces the earlier, merged
    /// with it); `None`: the same kind, title and detail.
    pub key: Option<String>,
}

impl Notification {
    pub fn new(kind: NotificationKind, title: impl Into<String>) -> Self {
        Self {
            id: NotificationId::new(),
            kind,
            title: title.into(),
            detail: None,
            actions: Vec::new(),
            timeout: Timeout::Default,
            key: None,
        }
    }

    pub fn success(title: impl Into<String>) -> Self {
        Self::new(NotificationKind::Success, title)
    }

    pub fn info(title: impl Into<String>) -> Self {
        Self::new(NotificationKind::Info, title)
    }

    pub fn warning(title: impl Into<String>) -> Self {
        Self::new(NotificationKind::Warning, title)
    }

    pub fn error(title: impl Into<String>) -> Self {
        Self::new(NotificationKind::Error, title)
    }

    /// Work going on; [`Notifier::update`] it (same id) when it ends.
    pub fn progress(title: impl Into<String>) -> Self {
        Self::new(NotificationKind::Progress, title)
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }

    /// Adds a button (beyond [`MAX_ACTIONS`], ignored).
    pub fn with_action(mut self, label: impl Into<String>, command: CommandId) -> Self {
        if self.actions.len() < MAX_ACTIONS {
            self.actions.push(NotificationAction { label: label.into(), command });
        }
        self
    }

    pub fn with_timeout(mut self, timeout: Timeout) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// The same notification under another identity (an update of `id`).
    pub fn with_id(mut self, id: NotificationId) -> Self {
        self.id = id;
        self
    }

    /// How long it stays shown (`None`: until it's closed).
    pub fn duration(&self) -> Option<Duration> {
        match self.timeout {
            Timeout::Never => None,
            Timeout::After(d) => Some(d),
            Timeout::Default if !self.actions.is_empty() => None,
            Timeout::Default => match self.kind {
                NotificationKind::Success | NotificationKind::Info => Some(SHORT),
                NotificationKind::Warning => Some(LONG),
                NotificationKind::Error | NotificationKind::Progress => None,
            },
        }
    }

    /// Whether `other` says the same (merged with it).
    fn same_as(&self, other: &Notification) -> bool {
        match (&self.key, &other.key) {
            (Some(a), Some(b)) => a == b,
            (None, None) => {
                self.kind == other.kind && self.title == other.title && self.detail == other.detail
            }
            _ => false,
        }
    }
}

/// The port modules tell the user through. Usable from any thread: what it's given is
/// shown on the UI thread at its next frame.
pub trait Notifier: Send + Sync {
    /// Shows `notification` (merged with an identical one shown).
    fn show(&self, notification: Notification);
    /// Replaces the one with the same id (work in progress that ended: its kind, its texts,
    /// its time start again); shown if it's gone.
    fn update(&self, notification: Notification);
    /// Closes notification `id`.
    fn dismiss(&self, id: NotificationId);
}

/// A notifier that tells no one (no window: a test, a module used alone).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoNotifier;

impl Notifier for NoNotifier {
    fn show(&self, _: Notification) {}
    fn update(&self, _: Notification) {}
    fn dismiss(&self, _: NotificationId) {}
}

/// A notification on the stack, with its state.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub notification: Notification,
    /// How many identical ones it stands for (`×2`).
    pub count: u32,
    /// How long it's been shown (its enter transition; a spinner's turn).
    pub age: Duration,
    /// How long its time has run (standing still under the mouse).
    pub elapsed: Duration,
    /// How long since it was closed (its exit transition), once it is.
    pub leaving: Option<Duration>,
    /// Shown (else waiting for room).
    shown: bool,
}

impl Toast {
    fn new(notification: Notification) -> Self {
        Self {
            notification,
            count: 1,
            age: Duration::ZERO,
            elapsed: Duration::ZERO,
            leaving: None,
            shown: false,
        }
    }

    pub fn id(&self) -> NotificationId {
        self.notification.id
    }

    /// The part of its time left (1 → 0); `None`: it stays until closed.
    pub fn remaining(&self) -> Option<f32> {
        let total = self.notification.duration()?;
        let left = total.saturating_sub(self.elapsed).as_secs_f32();
        Some((left / total.as_secs_f32().max(1e-3)).clamp(0.0, 1.0))
    }

    /// How far it entered (0..=1, eased out).
    pub fn entered(&self) -> f32 {
        ease(self.age.as_secs_f32() / ENTER.as_secs_f32())
    }

    /// How present it is: entered, less how far it left (0..=1). Its place in the stack and
    /// its opacity follow it.
    pub fn presence(&self) -> f32 {
        let gone = self.leaving.map_or(0.0, |l| ease(l.as_secs_f32() / EXIT.as_secs_f32()));
        (self.entered() * (1.0 - gone)).clamp(0.0, 1.0)
    }

    pub fn is_leaving(&self) -> bool {
        self.leaving.is_some()
    }

    fn transitioning(&self) -> bool {
        self.age < ENTER || self.leaving.is_some()
    }
}

/// Ease-out cubic of `t` (clamped to 0..=1).
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// A notification as the history keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub id: NotificationId,
    pub kind: NotificationKind,
    pub title: String,
    pub detail: Option<String>,
    pub count: u32,
    /// When it was last shown.
    pub at: Instant,
}

/// A project's notifications: the stack (shown and waiting) and the history.
#[derive(Debug, Default)]
pub struct Notifications {
    /// In the order they came (the shown ones first).
    toasts: Vec<Toast>,
    /// Newest first.
    history: VecDeque<Recent>,
    /// New at every change of what's shown (not by time), unique across stacks: what draws
    /// them keeps its layout until it changes.
    generation: u64,
    /// Time doesn't pass (snapshots: the same picture every run).
    frozen: bool,
}

impl Notifications {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows `notification`: merged with an identical one still there (its count grows,
    /// its time starts again, it takes the newer one's id), else at the end (shown if
    /// there's room). Returns the id it's shown under.
    pub fn show(&mut self, notification: Notification, now: Instant) -> NotificationId {
        let id = notification.id;
        self.bump();
        if let Some(toast) = self
            .toasts
            .iter_mut()
            .find(|t| !t.is_leaving() && t.notification.same_as(&notification))
        {
            let old = toast.id();
            toast.count += 1;
            toast.elapsed = Duration::ZERO;
            let count = toast.count;
            toast.notification = notification.clone();
            self.remember(old, &notification, count, now);
            return id;
        }
        self.remember(id, &notification, 1, now);
        self.toasts.push(Toast::new(notification));
        self.promote();
        id
    }

    /// Replaces notification `notification.id` (its kind, texts and actions; its time
    /// starts again, its count and place stay); shown anew if it's gone or leaving.
    pub fn update(&mut self, notification: Notification, now: Instant) {
        let id = notification.id;
        let Some(toast) = self.toasts.iter_mut().find(|t| t.id() == id && !t.is_leaving()) else {
            self.show(notification, now);
            return;
        };
        toast.elapsed = Duration::ZERO;
        let count = toast.count;
        toast.notification = notification.clone();
        self.bump();
        self.remember(id, &notification, count, now);
    }

    /// Closes notification `id` (it leaves with its transition). `false`: not there.
    pub fn dismiss(&mut self, id: NotificationId) -> bool {
        let Some(toast) = self.toasts.iter_mut().find(|t| t.id() == id && !t.is_leaving()) else {
            return false;
        };
        if toast.shown {
            toast.leaving = Some(Duration::ZERO);
        } else {
            self.toasts.retain(|t| t.id() != id);
        }
        self.bump();
        true
    }

    /// Closes them all (those waiting too). `false`: none was there.
    pub fn dismiss_all(&mut self) -> bool {
        let any = self.toasts.iter().any(|t| !t.is_leaving());
        self.toasts.retain(|t| t.shown);
        for toast in &mut self.toasts {
            toast.leaving.get_or_insert(Duration::ZERO);
        }
        if any {
            self.bump();
        }
        any
    }

    /// `dt` passed (`paused`: the mouse is over the stack, their time stands still; the
    /// transitions go on). Those whose time ran out leave, those gone are dropped, those
    /// waiting take their room. `true` if what's shown changed.
    pub fn advance(&mut self, dt: Duration, paused: bool) -> bool {
        if self.frozen || self.toasts.is_empty() {
            return false;
        }
        let mut changed = false;
        for toast in self.toasts.iter_mut().filter(|t| t.shown) {
            toast.age = toast.age.saturating_add(dt);
            match &mut toast.leaving {
                Some(leaving) => *leaving = leaving.saturating_add(dt),
                None if !paused => {
                    toast.elapsed = toast.elapsed.saturating_add(dt);
                    if toast.notification.duration().is_some_and(|d| toast.elapsed >= d) {
                        toast.leaving = Some(Duration::ZERO);
                        changed = true;
                    }
                }
                None => {}
            }
        }
        let before = self.toasts.len();
        self.toasts.retain(|t| t.leaving.is_none_or(|l| l < EXIT));
        changed |= self.toasts.len() != before;
        changed |= self.promote();
        if changed {
            self.bump();
        }
        changed
    }

    /// The first [`MAX_VISIBLE`] not leaving are shown. `true` if one more is.
    fn promote(&mut self) -> bool {
        let mut room = MAX_VISIBLE;
        let mut promoted = false;
        for toast in self.toasts.iter_mut().filter(|t| !t.is_leaving()) {
            if room == 0 {
                break;
            }
            room -= 1;
            if !toast.shown {
                toast.shown = true;
                promoted = true;
            }
        }
        promoted
    }

    /// Keeps `notification` (shown under `old`'s entry before, if merged) at the top of
    /// the history.
    fn remember(
        &mut self,
        old: NotificationId,
        notification: &Notification,
        count: u32,
        at: Instant,
    ) {
        let id = notification.id;
        self.history.retain(|r| r.id != old && r.id != id);
        self.history.push_front(Recent {
            id,
            kind: notification.kind,
            title: notification.title.clone(),
            detail: notification.detail.clone(),
            count,
            at,
        });
        self.history.truncate(HISTORY);
    }

    /// The ones shown (those leaving too), in the order they came.
    pub fn shown(&self) -> impl Iterator<Item = &Toast> {
        self.toasts.iter().filter(|t| t.shown)
    }

    /// How many wait for room.
    pub fn waiting(&self) -> usize {
        self.toasts.iter().filter(|t| !t.shown).count()
    }

    pub fn get(&self, id: NotificationId) -> Option<&Toast> {
        self.toasts.iter().find(|t| t.id() == id)
    }

    /// Nothing shown nor waiting.
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// An enter or exit transition is going on (frames are needed).
    pub fn transitioning(&self) -> bool {
        !self.frozen && self.shown().any(Toast::transitioning)
    }

    /// Time passes for one shown (its ring turns; a spinner too): frames at a slow pace.
    pub fn ticking(&self) -> bool {
        !self.frozen
            && self.shown().any(|t| {
                !t.is_leaving()
                    && (t.notification.duration().is_some()
                        || t.notification.kind == NotificationKind::Progress)
            })
    }

    /// The last ones shown, newest first.
    pub fn history(&self) -> impl Iterator<Item = &Recent> {
        self.history.iter()
    }

    /// What's shown changed: a new generation, never one another stack had (a project's
    /// stack replaced by another's is never taken for it).
    fn bump(&mut self) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.generation = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Stops time (or lets it pass again): the transitions are over at once.
    pub fn freeze(&mut self, frozen: bool) {
        self.frozen = frozen;
        if frozen {
            self.toasts.retain(|t| t.leaving.is_none());
            for toast in self.toasts.iter_mut().filter(|t| t.shown) {
                toast.age = toast.age.max(ENTER);
            }
            self.bump();
        }
    }

    /// Notification `id`'s time as if `elapsed` had run (snapshots: its ring part way).
    pub fn set_elapsed(&mut self, id: NotificationId, elapsed: Duration) {
        if let Some(toast) = self.toasts.iter_mut().find(|t| t.id() == id) {
            toast.elapsed = elapsed;
        }
    }
}

/// Where the stack is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ToastPosition {
    BottomRight,
    BottomCenter,
    #[default]
    TopCenter,
    TopRight,
}

impl ToastPosition {
    /// In the order the settings offer them.
    pub const ALL: [Self; 4] =
        [Self::BottomRight, Self::BottomCenter, Self::TopCenter, Self::TopRight];

    /// As saved in the settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::BottomRight => "bottom-right",
            Self::BottomCenter => "bottom-center",
            Self::TopCenter => "top-center",
            Self::TopRight => "top-right",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// Under the tab bar (the newest on top); else above the bottom bar (the newest at the
    /// bottom).
    pub fn top(self) -> bool {
        matches!(self, Self::TopCenter | Self::TopRight)
    }

    pub fn centered(self) -> bool {
        matches!(self, Self::TopCenter | Self::BottomCenter)
    }
}

impl settings_model::SettingType for ToastPosition {
    fn from_value(value: &settings_model::SettingValue) -> Option<Self> {
        value.as_str().and_then(Self::parse)
    }

    fn to_value(&self) -> settings_model::SettingValue {
        settings_model::SettingValue::Text(self.id().to_owned())
    }
}

#[cfg(test)]
#[path = "../tests/unit/notifications.rs"]
mod tests;
