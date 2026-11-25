//! The winit loop's events turned into the app's: the loop resumed and waiting (timed
//! redraws), the events sent from outside it, each window's own, and the keys and
//! modifiers in the platform API's terms.

use super::*;

impl ApplicationHandler<UserEvent> for Handler {
    /// The app ends: its sign leaves the notification area (Windows keeps a dead icon there
    /// until the mouse passes over it otherwise); the app writes what it still has to.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "windows")]
        system_sign_windows::set(None, std::iter::empty());
        self.app.exiting();
    }

    /// Fires the timed redraws that are due and sleeps until the next one (or until an
    /// event, when there is none).
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let dragging = self.tell_file_drags(event_loop);
        let now = Instant::now();
        let next = match self.timers.lock() {
            Ok(mut timers) => {
                for (id, _) in timers.iter().filter(|(_, at)| *at <= now) {
                    if let Some(w) = self.windows.iter().find(|w| w.window.id() == *id) {
                        w.window.request_redraw();
                    }
                }
                let windows = &self.windows;
                timers.retain(|(id, at)| *at > now && windows.iter().any(|w| w.window.id() == *id));
                timers.iter().map(|(_, at)| *at).min()
            }
            Err(_) => None,
        };
        // A drag over a window: its pointer asked again soon.
        let next = match (next, dragging.then(|| now + file_drag::POLL)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        self.install_menu();
        #[cfg(target_os = "macos")]
        menu::observe_opening(self.proxy.clone());
        #[cfg(target_os = "macos")]
        {
            let proxy = Mutex::new(self.proxy.clone());
            ime_caret::observe_source(move || {
                if let Ok(proxy) = proxy.lock() {
                    let _ = proxy.send_event(UserEvent::InputSourceChanged);
                }
            });
        }
        self.open_window(event_loop);
        // The Dock's icon once the window is on its way (it doesn't hold the window up).
        #[cfg(target_os = "macos")]
        app_icon::set_dock_icon(self.options.icon);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::PathsOpened(paths) => self.app.paths_opened(paths),
            // Commands from the native menu: the active window.
            // The sign's row bringing the app to the front: its last window, unminimized.
            UserEvent::Command(id) if id == platform_api::SHOW_APP => {
                if let Some(w) = self.windows.iter().find(|w| Some(w.window.id()) == self.focused) {
                    w.window.set_minimized(false);
                    w.window.focus_window();
                } else if let Some(w) = self.windows.last() {
                    w.window.set_minimized(false);
                    w.window.focus_window();
                }
                #[cfg(target_os = "macos")]
                if let Some(mtm) = objc2::MainThreadMarker::new() {
                    objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
                }
            }
            UserEvent::Command(id) => {
                if let Some(focused) =
                    self.focused.or_else(|| self.windows.first().map(|w| w.window.id()))
                {
                    self.dispatch(event_loop, focused, PlatformEvent::Command(id));
                }
            }
            // A background task finished: its window checks its results.
            UserEvent::Wake(Some((id, pending))) => {
                // Cleared before polling: anything finishing during the poll wakes it again.
                pending.store(false, Ordering::Release);
                if self.windows.iter().any(|w| w.window.id() == id) {
                    self.dispatch(event_loop, id, PlatformEvent::Wake);
                }
            }
            // Something every window shows: each one checks.
            UserEvent::Wake(None) => {
                WAKE_PENDING.store(false, Ordering::Release);
                let ids: Vec<WindowId> = self.windows.iter().map(|w| w.window.id()).collect();
                for id in ids {
                    self.dispatch(event_loop, id, PlatformEvent::Wake);
                }
            }
            UserEvent::OpenWindow => self.open_window(event_loop),
            UserEvent::SetMenus(menus) => {
                self.options.menus = menus;
                self.install_menu();
            }
            UserEvent::SetMenuStates(disabled) => {
                #[cfg(target_os = "macos")]
                {
                    if let Some(menu) = &self.native_menu {
                        menu.set_disabled(&disabled);
                    }
                    self.menu_disabled = disabled;
                }
                #[cfg(not(target_os = "macos"))]
                let _ = disabled;
            }
            UserEvent::InputSourceChanged => {
                if let Some(state) = self.focused.and_then(|id| self.state(id)) {
                    let told = ime_caret::lock(&state.ime).source_changed();
                    if let Some(area) = told {
                        ime_caret::tell(&state.window, area);
                    }
                }
            }
            UserEvent::MenuOpening => {
                if let Some(focused) = self.focused {
                    self.dispatch(event_loop, focused, PlatformEvent::MenuOpening);
                }
            }
            UserEvent::SetIconMenu(menu) => {
                #[cfg(target_os = "macos")]
                dock::set(&menu);
                #[cfg(target_os = "windows")]
                jump_list::set(&menu);
                let _ = menu;
            }
            UserEvent::SetSystemSign(sign) => {
                #[cfg(target_os = "macos")]
                system_sign::set(sign.as_ref());
                #[cfg(target_os = "windows")]
                system_sign_windows::set(sign.as_ref(), self.windows.iter().map(|w| &*w.window));
                let _ = sign;
            }
            UserEvent::Focus(id) => {
                if let Some(w) = self.windows.iter().find(|w| api_id(w.window.id()) == id) {
                    w.window.set_minimized(false);
                    w.window.focus_window();
                }
            }
            UserEvent::ScreenColor(id, color) => {
                self.dispatch(event_loop, id, PlatformEvent::ScreenColor(color));
            }
            // A timed redraw was requested: `about_to_wait` sets the deadline.
            UserEvent::Timer => {}
            UserEvent::Access(accesskit_winit::Event { window_id, window_event }) => {
                use accesskit_winit::WindowEvent as Asked;
                match window_event {
                    // A screen reader came: a frame makes the tree.
                    Asked::InitialTreeRequested => {
                        if let Some(state) = self.state(window_id) {
                            state.access.asks.asked();
                            state.window.request_redraw();
                        }
                    }
                    Asked::ActionRequested(asked) => {
                        if let Some(request) = access_bridge::request(&asked) {
                            self.dispatch(event_loop, window_id, PlatformEvent::Access(request));
                        }
                    }
                    Asked::AccessibilityDeactivated => {
                        if let Some(state) = self.state(window_id) {
                            state.access.asks.left();
                        }
                    }
                }
            }
            UserEvent::AccessTree(id, ask, update) => {
                if let Some(state) = self.state(id)
                    && state.access.asks.current(ask)
                {
                    state.access.adapter.update_if_active(|| update);
                }
            }
            UserEvent::ResetModifiers(id) => {
                let Some(state) = self.state(id) else { return };
                if state.modifiers != Modifiers::default() {
                    state.modifiers = Modifiers::default();
                    self.dispatch(
                        event_loop,
                        id,
                        PlatformEvent::ModifiersChanged(Modifiers::default()),
                    );
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.state(id).map(|w| Arc::clone(&w.window)) else { return };
        // The system's accessibility follows the window (its focus, its size) first.
        if let Some(state) = self.state(id) {
            state.access.adapter.process_event(&window, &event);
        }
        let translated = match event {
            WindowEvent::Resized(size) => {
                // AppKit repositions the window buttons on every resize.
                self.layout_titlebar(&window);
                PlatformEvent::Resized(PhysicalSize::new(size.width, size.height))
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.layout_titlebar(&window);
                PlatformEvent::ScaleFactorChanged(scale_factor)
            }
            WindowEvent::RedrawRequested => PlatformEvent::RedrawRequested,
            WindowEvent::CloseRequested => PlatformEvent::CloseRequested,
            WindowEvent::Focused(focused) => {
                self.layout_titlebar(&window);
                if focused {
                    self.focused = Some(id);
                }
                PlatformEvent::Focused(focused)
            }
            WindowEvent::Occluded(occluded) => PlatformEvent::Occluded(occluded),
            WindowEvent::ThemeChanged(theme) => {
                PlatformEvent::SystemAppearanceChanged(appearance(theme))
            }
            WindowEvent::ModifiersChanged(m) => {
                let changed = modifiers(m.state());
                let Some(state) = self.state(id) else { return };
                if changed == state.modifiers {
                    return;
                }
                state.modifiers = changed;
                PlatformEvent::ModifiersChanged(changed)
            }
            WindowEvent::KeyboardInput { event, .. } => PlatformEvent::Key(KeyEvent {
                key: key(&event.logical_key),
                pressed: event.state == ElementState::Pressed,
                repeat: event.repeat,
                modifiers: self.state(id).map(|s| s.modifiers).unwrap_or_default(),
                text: event.text.map(|t| t.to_string()),
                numpad: event.location == keyboard::KeyLocation::Numpad,
                physical: physical(event.physical_key),
            }),
            WindowEvent::Ime(ime) => {
                // A composition starting tells the system the caret's place (its candidates
                // shown there), one ending stops following it.
                let composing =
                    matches!(&ime, winit::event::Ime::Preedit(text, _) if !text.is_empty());
                if let Some(state) = self.state(id) {
                    let told = ime_caret::lock(&state.ime).composing(composing);
                    if let Some(area) = told {
                        ime_caret::tell(&state.window, area);
                    }
                }
                match ime {
                    winit::event::Ime::Preedit(text, cursor) => {
                        PlatformEvent::Ime(ImeEvent::Preedit { text, cursor })
                    }
                    winit::event::Ime::Commit(text) => PlatformEvent::Ime(ImeEvent::Commit(text)),
                    winit::event::Ime::Enabled | winit::event::Ime::Disabled => return,
                }
            }
            WindowEvent::PinchGesture { delta, .. } => PlatformEvent::Pinch(delta as f32),
            WindowEvent::MouseWheel { delta, .. } => PlatformEvent::Scroll(match delta {
                MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
                MouseScrollDelta::PixelDelta(p) => {
                    ScrollDelta::Pixels { x: p.x as f32, y: p.y as f32 }
                }
            }),
            WindowEvent::CursorMoved { position, .. } => {
                #[cfg(target_os = "macos")]
                titlebar::end_drag(self.windows.iter().map(|w| &w.window));
                let p = Point::new(position.x as f32, position.y as f32);
                if let Some(state) = self.state(id) {
                    state.cursor = p;
                }
                PlatformEvent::CursorMoved(p)
            }
            WindowEvent::CursorLeft { .. } => PlatformEvent::CursorLeft,
            WindowEvent::MouseInput { state, button, .. } => {
                #[cfg(target_os = "macos")]
                titlebar::end_drag(self.windows.iter().map(|w| &w.window));
                let button = match button {
                    winit::event::MouseButton::Left => MouseButton::Left,
                    winit::event::MouseButton::Right => MouseButton::Right,
                    winit::event::MouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };
                let (cursor, modifiers) =
                    self.state(id).map(|s| (s.cursor, s.modifiers)).unwrap_or_default();
                match state {
                    ElementState::Pressed => {
                        PlatformEvent::MouseDown { button, position: cursor, modifiers }
                    }
                    ElementState::Released => PlatformEvent::MouseUp { button, position: cursor },
                }
            }
            WindowEvent::HoveredFile(path) => {
                if let Some(state) = self.state(id) {
                    state.drag.hovered(path);
                }
                return;
            }
            WindowEvent::DroppedFile(path) => {
                if let Some(state) = self.state(id) {
                    state.drag.dropped(path);
                }
                return;
            }
            WindowEvent::HoveredFileCancelled => {
                if !self.state(id).is_some_and(|state| state.drag.cancelled()) {
                    return;
                }
                PlatformEvent::FilesHoverCancelled
            }
            _ => return,
        };
        self.dispatch(event_loop, id, translated);
    }
}

pub(super) fn appearance(theme: winit::window::Theme) -> SystemAppearance {
    match theme {
        winit::window::Theme::Light => SystemAppearance::Light,
        winit::window::Theme::Dark => SystemAppearance::Dark,
    }
}

fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        meta: state.super_key(),
    }
}

fn key(key: &keyboard::Key) -> Key {
    use keyboard::NamedKey as W;
    match key {
        keyboard::Key::Character(c) => Key::Character(c.to_string()),
        keyboard::Key::Named(named) => Key::Named(match named {
            W::ArrowUp => NamedKey::ArrowUp,
            W::ArrowDown => NamedKey::ArrowDown,
            W::ArrowLeft => NamedKey::ArrowLeft,
            W::ArrowRight => NamedKey::ArrowRight,
            W::PageUp => NamedKey::PageUp,
            W::PageDown => NamedKey::PageDown,
            W::Home => NamedKey::Home,
            W::End => NamedKey::End,
            W::Enter => NamedKey::Enter,
            W::Escape => NamedKey::Escape,
            W::Tab => NamedKey::Tab,
            W::Backspace => NamedKey::Backspace,
            W::Delete => NamedKey::Delete,
            W::Insert => NamedKey::Insert,
            W::Space => NamedKey::Space,
            W::F1 => NamedKey::F(1),
            W::F2 => NamedKey::F(2),
            W::F3 => NamedKey::F(3),
            W::F4 => NamedKey::F(4),
            W::F5 => NamedKey::F(5),
            W::F6 => NamedKey::F(6),
            W::F7 => NamedKey::F(7),
            W::F8 => NamedKey::F(8),
            W::F9 => NamedKey::F(9),
            W::F10 => NamedKey::F(10),
            W::F11 => NamedKey::F(11),
            W::F12 => NamedKey::F(12),
            _ => return Key::Unidentified,
        }),
        _ => Key::Unidentified,
    }
}

/// The character a key's place gives on a US layout (see [`KeyEvent::physical`]).
fn physical(key: keyboard::PhysicalKey) -> Option<char> {
    use keyboard::KeyCode as K;
    let keyboard::PhysicalKey::Code(code) = key else { return None };
    const LETTERS: [K; 26] = [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ];
    const DIGITS: [K; 10] = [
        K::Digit0,
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
    ];
    if let Some(i) = LETTERS.iter().position(|k| *k == code) {
        return char::from_u32(u32::from('a') + i as u32);
    }
    if let Some(i) = DIGITS.iter().position(|k| *k == code) {
        return char::from_digit(i as u32, 10);
    }
    Some(match code {
        K::Minus => '-',
        K::Equal => '=',
        K::BracketLeft => '[',
        K::BracketRight => ']',
        K::Backslash => '\\',
        K::Semicolon => ';',
        K::Quote => '\'',
        K::Comma => ',',
        K::Period => '.',
        K::Slash => '/',
        K::Backquote => '`',
        _ => return None,
    })
}
