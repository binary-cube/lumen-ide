//! What a sleeping terminal says over its last screen: the sleeping's card (its moon, why,
//! "Wake Up ↵"), drawn in the terminal's words over its grid; while it comes back, the same
//! card says it's resuming (no button).

use super::*;

/// What the snapshots say it freed (a made-up figure, the same every run).
#[cfg(feature = "snapshot")]
const SNAPSHOT_FREED: u64 = 412 * 1024 * 1024;

impl TerminalView {
    /// The app's `--snapshot`: put to sleep by hand now, as if it held [`SNAPSHOT_FREED`]
    /// (its owner needed not: it resumes the way it started).
    #[cfg(feature = "snapshot")]
    pub(super) fn asleep_for_snapshot(&mut self) -> bool {
        let Some(launch) = self.launch.clone() else { return false };
        if let Some(slot) = &self.renamed {
            asked(slot).resume = Some(launch);
        }
        if let Some(part) = &self.sleep {
            part.slot().set_memory(Some(SNAPSHOT_FREED));
        }
        self.fall_asleep(ui_components::sleep::SleepWhy::ByHand);
        self.asleep
    }

    /// The card over its grid (see the module); `resuming`: it's coming back (no button).
    pub(super) fn paint_asleep(
        &mut self,
        layer: &mut Layer,
        paint: (&mut TextSystem, &icons::Icons),
        (palette, scale): (&Palette, f32),
        resuming: bool,
    ) {
        let area = self.grid_area;
        let Some(part) = &mut self.sleep else { return };
        let ctx = ui_components::ds::Ctx::new(palette, scale);
        part.paint_card(layer, paint, &ctx, area, resuming);
    }
}
