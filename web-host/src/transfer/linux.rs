//! Linux: the frame's dmabuf descriptors are sent beside the message on the app's socket
//! (`SCM_RIGHTS`), which the host's server writes — not done yet (plan 99, 99e): until then
//! no frame is handed over and the page shows nothing.

use cef::AcceleratedPaintInfo;
use web_engine_protocol::{Hello, PageId, Texture};
use web_host_core::Undelivered;

pub struct Transfer;

impl Transfer {
    pub fn to(_hello: &Hello) -> Self {
        Self
    }

    pub fn hand_over(
        &self,
        _info: &AcceleratedPaintInfo,
        _page: PageId,
        _number: u64,
    ) -> Option<(Texture, Option<Undelivered>)> {
        None
    }
}
