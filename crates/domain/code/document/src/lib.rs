//! Document bounded context: text content and editing (buffer, selections, transactions, history).

pub mod application;
pub mod domain;
pub mod events;

pub use application::closing::{CloseDocuments, CloseStep, OpenDocuments};
pub use application::commands;
pub use application::find::{FindState, WholeSearch};
pub use application::storage::{
    DocumentStorage, PreparedSave, open_document, prepare_save, probe_text, save_document,
};
pub use domain::buffer::{TextBuffer, TextSnapshot};
pub use domain::document::{Document, Indent, SaveCleanup};
pub use domain::edit::{Assoc, ChangeMap, Edit, OffsetMap, TextChange, Transaction};
pub use domain::encoding::{
    Decoded, Encoding, LineEnding, MAX_TEXT_LEN, TextCheck, check_text, decode, decode_as,
    represents_chunks,
};
pub use domain::history::EditKind;
pub use domain::movement::{Motion, visual_width};
pub use domain::search::{LineMatch, Matcher, SearchError, SearchQuery};
pub use domain::selection::{Selection, Selections};
