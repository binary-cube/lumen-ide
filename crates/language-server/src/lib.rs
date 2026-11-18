//! The built-in engines as a language server, for other editors (and to try our LSP client
//! against): the same crates the app runs in process, behind LSP on two streams. One thread
//! answers; the project is indexed on the indexer's own.

mod answers;
mod diagnostics;
mod engines;
mod frameworks;
mod rename;
mod session;

use std::io::{BufReader, Read, Write};
use std::sync::mpsc;

use lsp_protocol::jsonrpc::read_message;
use session::{Input, Session};

/// Answers the client on `input` / `output` until it leaves: `Ok(true)` if it asked to shut
/// down first (LSP's clean end).
pub fn serve(input: impl Read + Send + 'static, output: impl Write) -> std::io::Result<bool> {
    let (tx, rx) = mpsc::channel();
    let wake = tx.clone();
    std::thread::Builder::new().name("lsp-input".into()).spawn(move || {
        let mut reader = BufReader::new(input);
        while let Ok(Some(message)) = read_message(&mut reader) {
            if tx.send(Input::Message(message)).is_err() {
                return;
            }
        }
        let _ = tx.send(Input::Closed);
    })?;
    Session::new(engines::Engines::load(), output, wake).run(&rx)
}
