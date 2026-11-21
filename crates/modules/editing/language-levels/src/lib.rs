//! The language version picker, as this module's popover: the app opens it above the status
//! bar's version item (with the versions, the project's and the file's), and gets the choice
//! back as a request ([`AppRequest::SetLevel`]); Esc or a click outside closes it.

use std::path::Path;
use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::Point;
use ui_components::{
    AppRequest, KeyReach, LEVEL_PICKER_EXTENSION, LevelChoices, LevelClick, LevelPick, LevelPicker,
    Painter, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("editor.language-levels"),
    name: "Language versions",
    description: "The version picker in the status bar: for the project or only for the file",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "layers", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct LanguageLevelsModule;

impl Module for LanguageLevelsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(PickerProvider));
        Ok(())
    }
}

#[derive(Debug)]
struct PickerProvider;

impl ViewerProvider for PickerProvider {
    fn id(&self) -> &'static str {
        "levels.picker"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == LEVEL_PICKER_EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(PickerView::default()))
    }
}

#[derive(Debug, Default)]
struct PickerView {
    picker: LevelPicker,
    choices: Option<LevelChoices>,
    action: Option<ViewerAction>,
    /// Chosen or given up: the app closes it.
    done: bool,
    /// A press on the filter holds the pointer: a drag selects in it until it's let go.
    selecting: bool,
}

impl PickerView {
    /// Chosen: the app applies it (and closes the popover, done after its request).
    fn pick(&mut self, pick: LevelPick) {
        self.finish(ViewerAction::Request(AppRequest::SetLevel(pick)));
    }

    /// Given up (Esc, a click outside): the app closes the popover.
    fn close(&mut self) {
        self.finish(ViewerAction::Close);
    }

    fn finish(&mut self, action: ViewerAction) {
        self.picker.close();
        self.action = Some(action);
        self.done = true;
    }
}

impl Viewer for PickerView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(choices) = data.downcast_ref::<LevelChoices>() else { return };
        self.picker.open(choices);
        self.choices = Some(choices.clone());
    }

    /// Over the whole window (it places itself above its item).
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let Some(choices) = &self.choices else { return };
        self.picker.set_viewport(ctx.bounds, ctx.scale);
        let painter =
            Painter { text: ctx.text, scene: ctx.scene, palette: ctx.palette, icons: ctx.icons };
        self.picker.paint(choices, painter);
    }

    /// On the filter: its caret put (two a word, three all of it), × emptying it, a drag
    /// from there selecting; anywhere else as a click.
    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.selecting = self.picker.press_field(p, clicks);
        if self.selecting {
            return None;
        }
        self.click(p)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.selecting && self.picker.drag(p)
    }

    fn release(&mut self) -> bool {
        self.selecting = false;
        false
    }

    /// A pill chooses; outside, it closes (and does nothing else).
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        match self.picker.click(p) {
            LevelClick::Pick(pick) => self.pick(pick),
            LevelClick::Inside => {}
            LevelClick::Outside => self.close(),
        }
        self.action.take()
    }

    fn hover(&mut self, p: Point) -> bool {
        self.picker.hover(p)
    }

    /// A hand on a pill, a scope or ×; the text's over the filter.
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.picker.cursor_at(p)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.picker.type_text(text);
        true
    }

    fn paste(&mut self, text: &str) -> bool {
        self.text_input(text)
    }

    /// What's selected in the filter (⌘C).
    fn selected_text(&self) -> Option<String> {
        self.picker.selected_text()
    }

    /// The filter's edit commands (⌘A, ⌘X, ⌘Z, ⇧⌘Z).
    fn command(&mut self, id: &str) -> bool {
        self.picker.command(id)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    /// ← → ↑ ↓ move, Tab switches the scope, ↵ chooses; Esc clears the filter first, then
    /// closes; the filter's editing keys (⌫, Delete, Home, End, by word and extending) edit
    /// it.
    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let has_file = self.choices.as_ref().is_some_and(|c| c.file_name.is_some());
        match key {
            ViewerKey::Escape if !self.picker.query.text().is_empty() => {
                self.picker.query.set_text("");
            }
            ViewerKey::Escape => self.close(),
            ViewerKey::Left => self.picker.step(-1, false),
            ViewerKey::Right => self.picker.step(1, false),
            ViewerKey::Up => self.picker.step(-1, true),
            ViewerKey::Down => self.picker.step(1, true),
            ViewerKey::Tab => self.picker.toggle_scope(has_file),
            ViewerKey::Backspace | ViewerKey::Delete | ViewerKey::Home | ViewerKey::End => {
                return self.picker.field_key(key, reach);
            }
            ViewerKey::Enter => {
                if let Some(pick) = self.picker.confirm() {
                    self.pick(pick);
                }
            }
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn is_animating(&self) -> bool {
        self.picker.is_animating()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
