//! A page's render handler: its size and scale, its frames (GPU textures handed over, or
//! pixels written into its file; while its app is behind, only now and then), its popup and
//! where its text being composed is.

use std::time::Instant;

use cef::rc::Rc as _;
use cef::*;
use web_engine_protocol::{Event, Texture};

use super::{Painting, Shared, popup_rect};
use crate::pixels::Painted;
use crate::transfer::Handed;

wrap_render_handler! {
    pub struct RenderBuilder {
        page: Shared,
        access: AccessibilityHandler,
    }

    impl RenderHandler {
        fn accessibility_handler(&self) -> Option<AccessibilityHandler> {
            Some(self.access.clone())
        }

        /// Its size in CSS pixels.
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            let Some(rect) = rect else { return };
            // Within a page's bounds: its scale never below the least one.
            let size = self.page.0.size();
            rect.width = ((size.width as f32 / size.scale).round() as i32).max(1);
            rect.height = ((size.height as f32 / size.scale).round() as i32).max(1);
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            let Some(info) = screen_info else { return 0 };
            info.device_scale_factor = self.page.0.size().scale;
            1
        }

        /// A new frame as pixels (a page asked for them): written into its file (only the
        /// rows that changed, once it holds one of its size), told to the app, held until it's
        /// read (while the app is behind, only now and then: the file then forgets what it
        /// holds, the next written whole).
        // CEF's callback hands a raw buffer: its shape is CEF's, not ours to mark `unsafe`.
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            let page = &self.page.0;
            let popup = type_ == PaintElementType::POPUP;
            if page.closed.get() || buffer.is_null() {
                return;
            }
            let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else { return };
            let Some(stride) = width.checked_mul(4) else { return };
            let Some(length) = (stride as usize).checked_mul(height as usize) else { return };
            crate::pump::Pump::touch();
            if !page.hands_over(popup, Instant::now()) {
                // Not written: the rows CEF tells changed next are from this frame on.
                page.forget_pixels(popup);
                return;
            }
            // SAFETY: CEF's BGRA buffer of `width` × `height` pixels (`length` bytes), valid
            // and unchanged for the call; the slice doesn't outlive it.
            #[allow(unsafe_code)]
            let pixels = unsafe { std::slice::from_raw_parts(buffer, length) };
            let dirty = dirty_rects.unwrap_or_default().iter().map(|rect| (rect.y, rect.height));
            if !page.write_pixels(popup, Painted { pixels, width, height }, dirty) {
                page.missed(popup);
                return;
            }
            let frame = page.next_frame();
            let texture = Texture::Pixels { stride };
            let painting = Painting { popup, frame, width, height, texture, dirty: dirty_rects };
            page.tell_frame(painting, None);
        }

        /// A new frame: its texture handed to the app, held until it's copied (while the app
        /// is behind, only now and then: [`super::frames::BEHIND_RETRY`]).
        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            let page = &self.page.0;
            tracing::trace!(page = page.key.page.0, "a frame painted");
            let popup = type_ == PaintElementType::POPUP;
            if page.closed.get() {
                return;
            }
            let Some(info) = info else { return };
            crate::pump::Pump::touch();
            if !page.hands_over(popup, Instant::now()) {
                return;
            }
            let frame = page.next_frame();
            let handed = page.transfer.hand_over(info, page.key.page, frame);
            let Some(Handed { texture, width, height, undelivered }) = handed else {
                tracing::debug!(frame, "a frame not handed over");
                page.missed(popup);
                return;
            };
            let painting = Painting { popup, frame, width, height, texture, dirty: dirty_rects };
            page.tell_frame(painting, undelivered);
        }

        /// Where its text being composed is: the last character's box (the input method's
        /// window goes beside it).
        fn on_ime_composition_range_changed(
            &self,
            _browser: Option<&mut Browser>,
            _selected_range: Option<&Range>,
            character_bounds: Option<&[Rect]>,
        ) {
            let Some(last) = character_bounds.and_then(<[Rect]>::last) else { return };
            let page = &self.page.0;
            page.send(Event::Caret { page: page.key.page, rect: popup_rect(last) });
        }

        /// Its popup (an open `<select>`'s list) shown or gone: told to the app, which draws
        /// it over the page. Gone, the next one shown is told whole.
        fn on_popup_show(&self, _browser: Option<&mut Browser>, show: ::std::os::raw::c_int) {
            if show == 0 {
                let page = &self.page.0;
                page.popup_gone();
                page.send(Event::Popup { page: page.key.page, rect: None });
            }
        }

        /// Where its popup is (CSS pixels, in the page).
        fn on_popup_size(&self, _browser: Option<&mut Browser>, rect: Option<&Rect>) {
            let Some(rect) = rect else { return };
            let page = &self.page.0;
            page.send(Event::Popup { page: page.key.page, rect: Some(popup_rect(rect)) });
        }
    }
}
