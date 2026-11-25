//! What an app's page tells its host: `window.ipc.postMessage(text)`, as the system's web
//! engines give it. Defined in the page's renderer, only for our own pages (an HTML page
//! runs no script at all); sent to the host's main process as a process message, then on
//! to the app ([`receive`]).

use cef::rc::Rc as _;
use cef::*;

/// The process message's name.
const MESSAGE: &str = "page-message";

/// What a page is told when its message is too long (a script's exception, never the user's).
const TOO_LONG: &str = "ipc.postMessage: the message is too long";

/// The most bytes a UTF-16 unit takes in UTF-8.
const UTF8_PER_UNIT: usize = 3;

/// Whether a page's message fits what the app takes ([`web_engine_protocol::MAX_PAGE_MESSAGE`]
/// bytes of UTF-8): its UTF-16 length tells it without a copy where it surely fits or surely
/// doesn't, its UTF-8 length otherwise.
fn fits(text: &CefString) -> bool {
    let units = text.as_slice().map_or(0, <[u16]>::len);
    let most = web_engine_protocol::MAX_PAGE_MESSAGE;
    if units.saturating_mul(UTF8_PER_UNIT) <= most {
        return true;
    }
    units <= most && text.to_string().len() <= most
}

/// `window.ipc` defined in `context` when `frame` shows one of our pages.
pub fn define(frame: Option<&mut Frame>, context: Option<&mut V8Context>) {
    let (Some(frame), Some(context)) = (frame, context) else { return };
    if frame.is_main() != 1 || !web_pages::is_own_url(&crate::text_of(&frame.url())) {
        return;
    }
    let Some(window) = context.global() else { return };
    let Some(mut ipc) = v8_value_create_object(None, None) else { return };
    let mut handler = PostBuilder::new(Post);
    let name = CefString::from("postMessage");
    let mut post = v8_value_create_function(Some(&name), Some(&mut handler));
    let attributes = sys::cef_v8_propertyattribute_t(
        [
            sys::cef_v8_propertyattribute_t::V8_PROPERTY_ATTRIBUTE_READONLY,
            sys::cef_v8_propertyattribute_t::V8_PROPERTY_ATTRIBUTE_DONTDELETE,
        ]
        .into_iter()
        .fold(0, |all, attribute| all | attribute.0),
    );
    ipc.set_value_bykey(Some(&name), post.as_mut(), attributes.into());
    window.set_value_bykey(Some(&"ipc".into()), Some(&mut ipc), attributes.into());
}

/// In the host's main process: the page's text if `message` is one of its messages, from
/// one of our pages, not too long.
pub fn receive(frame: Option<&mut Frame>, message: Option<&mut ProcessMessage>) -> Option<String> {
    let message = message?;
    if crate::text_of(&message.name()) != MESSAGE {
        return None;
    }
    let url = crate::text_of(&frame?.url());
    if !web_pages::is_own_url(&url) {
        tracing::warn!("a message from outside our pages dropped");
        return None;
    }
    let text = crate::text_of(&message.argument_list()?.string(0));
    if text.len() > web_engine_protocol::MAX_PAGE_MESSAGE {
        tracing::warn!(size = text.len(), "a page's message too long: dropped");
        return None;
    }
    Some(text)
}

#[derive(Clone)]
struct Post;

wrap_v8_handler! {
    struct PostBuilder {
        post: Post,
    }

    impl V8Handler {
        /// `postMessage(text)`: the text sent to the main process (anything else is ignored;
        /// a text too long is refused here, the page told by an exception, before it crosses
        /// processes).
        fn execute(
            &self,
            _name: Option<&CefString>,
            _object: Option<&mut V8Value>,
            arguments: Option<&[Option<V8Value>]>,
            _retval: Option<&mut Option<V8Value>>,
            exception: Option<&mut CefString>,
        ) -> ::std::os::raw::c_int {
            let Some(text) = arguments
                .and_then(|a| a.first())
                .and_then(Option::as_ref)
                .filter(|v| v.is_string() == 1)
            else {
                return 1;
            };
            let text = CefString::from(&text.string_value());
            if !fits(&text) {
                if let Some(exception) = exception {
                    *exception = TOO_LONG.into();
                }
                return 1;
            }
            let Some(frame) = v8_context_get_current_context().and_then(|c| c.frame()) else {
                return 1;
            };
            let Some(mut message) = process_message_create(Some(&MESSAGE.into())) else {
                return 1;
            };
            if let Some(list) = message.argument_list() {
                list.set_string(0, Some(&text));
            }
            frame.send_process_message(ProcessId::BROWSER, Some(&mut message));
            1
        }
    }
}
