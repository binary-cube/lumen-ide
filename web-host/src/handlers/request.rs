//! Where a page may go and what it may load (its own URLs only: anything else stopped, a
//! link told to the app), its windows asked for (never opened), its load's end and errors,
//! its renderer's end.

use std::sync::atomic::Ordering;

use cef::rc::Rc as _;
use cef::*;
use web_engine_protocol::Event;

use super::{OPEN_BROWSERS, Shared};
use crate::text_of;

wrap_life_span_handler! {
    pub struct LifeSpanBuilder {
        page: Shared,
    }

    impl LifeSpanHandler {
        /// A link for a new window: never opened here, told to the app.
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            if let Some(url) = target_url {
                self.page.0.link(url.to_string(), user_gesture == 1);
            }
            1
        }

        fn on_after_created(&self, _browser: Option<&mut Browser>) {
            OPEN_BROWSERS.fetch_add(1, Ordering::AcqRel);
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            OPEN_BROWSERS.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

wrap_request_handler! {
    pub struct RequestBuilder {
        page: Shared,
        // Every resource's handler, made once for the page.
        loads: ResourceRequestHandler,
    }

    impl RequestHandler {
        /// Only the page's own URLs (its app's scheme, its HTML): anything else stopped (a
        /// link followed in place is told to the app as one to open).
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let Some(request) = request else { return 1 };
            let url = text_of(&request.url());
            let page = &self.page.0;
            if page.may_go(&url) {
                return 0;
            }
            page.link(url, user_gesture == 1);
            1
        }

        /// Every resource the page loads goes through [`LoadsBuilder`] (the network refused).
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            Some(self.loads.clone())
        }

        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            _status: TerminationStatus,
            _error_code: ::std::os::raw::c_int,
            _error_string: Option<&CefString>,
        ) {
            let page = &self.page.0;
            page.send(Event::Crashed { page: page.key.page });
        }
    }
}

wrap_resource_request_handler! {
    pub struct LoadsBuilder {
        page: Shared,
    }

    impl ResourceRequestHandler {
        /// The page's own resources load; anything from the network is cancelled.
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(request) = request else { return ReturnValue::CANCEL };
            if web_pages::allows_load(&text_of(&request.url())) {
                ReturnValue::CONTINUE
            } else {
                ReturnValue::CANCEL
            }
        }
    }
}

wrap_load_handler! {
    pub struct LoadBuilder {
        page: Shared,
    }

    impl LoadHandler {
        /// A load that failed. One cancelled (a navigation stopped: a link told to the app,
        /// another load in its place) is the usual case, not a warning.
        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            error_code: Errorcode,
            _error_text: Option<&CefString>,
            _failed_url: Option<&CefString>,
        ) {
            let page = self.page.0.key.page.0;
            if *error_code.as_ref() == sys::cef_errorcode_t::ERR_ABORTED {
                tracing::debug!(page, "a page's load was cancelled");
            } else {
                tracing::warn!(page, code = ?error_code, "a page didn't load");
            }
        }

        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: ::std::os::raw::c_int,
        ) {
            let Some(frame) = frame.filter(|f| f.is_main() == 1) else { return };
            // The blank page a browser starts on isn't the page's content.
            if text_of(&frame.url()) != "about:blank" {
                let page = &self.page.0;
                tracing::debug!(page = page.key.page.0, "a page loaded");
                page.send(Event::Loaded { page: page.key.page });
            }
        }
    }
}
