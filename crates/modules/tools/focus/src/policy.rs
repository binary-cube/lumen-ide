//! What Focus mode asks of the window: while it's on, the region being worked in stands out
//! as its settings say (how gray, how dim, what it follows) and the theme's animated
//! effects stand still; off, nothing.

use workbench_model::{EmphasisRequest, RegionEmphasis};

use crate::settings::Config;

/// The request for `config`, while `on`.
pub fn request(on: bool, config: &Config) -> Option<EmphasisRequest> {
    on.then_some(EmphasisRequest {
        gray: config.gray,
        dim: config.dim,
        follow: config.follow,
        pause_effects: true,
    })
}

/// Its region emphasis: the state and the settings now.
#[derive(Debug)]
pub struct FocusEmphasis;

impl RegionEmphasis for FocusEmphasis {
    fn request(&self) -> Option<EmphasisRequest> {
        request(crate::state::is_on(), &crate::settings::config())
    }
}

#[cfg(test)]
#[path = "../tests/unit/policy.rs"]
mod tests;
