//! The GSC language server.
//!
//! A thin LSP layer over the LSP-agnostic core: [`gsc_sema`] answers the editor queries,
//! [`gsc_check`] produces diagnostics and fixes, [`gsc_fmt`] formats. The transport is
//! [`lsp_server`] (rust-analyzer's synchronous scaffold): a main loop owns the state, queries run
//! on it, and the two slow jobs run on their own threads: loading the workspace (with
//! `$/progress`) and linting (debounced, results dropped when the text changed meanwhile).
//!
//! [`run_stdio`] is what `gruff lsp` calls; [`serve`] runs the same server over any byte streams
//! (the integration tests drive it with in-memory pipes).

// `lsp_types::Uri` caches its parse in a cell; hashing it is stable (by text) all the same.
#![allow(clippy::mutable_key_type)]

mod analysis;
pub mod convert;
mod diagnostics;
mod features;
mod handlers;
mod imports;
mod inlay;
pub mod project;
mod semantic;
mod server;
mod signature;

use std::io::{self, BufRead, Write};

use lsp_server::{Connection, Message};

pub use server::DEBOUNCE;

/// Run the server on standard input and output until the client says `exit`.
pub fn run_stdio() -> Result<(), String> {
    let (conn, io) = Connection::stdio();
    let result = server::run(conn);
    io.join().map_err(|e| format!("i/o: {e}"))?;
    result
}

/// Run the server over `reader` and `writer` (LSP base protocol framing), until `exit` or the
/// end of `reader`.
pub fn serve<R, W>(mut reader: R, mut writer: W) -> Result<(), String>
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
{
    let (in_tx, in_rx) = crossbeam_channel::unbounded::<Message>();
    let (out_tx, out_rx) = crossbeam_channel::unbounded::<Message>();
    let reader_thread = std::thread::spawn(move || -> io::Result<()> {
        while let Some(msg) = Message::read(&mut reader)? {
            let exit = matches!(&msg, Message::Notification(n) if n.method == "exit");
            if in_tx.send(msg).is_err() || exit {
                break;
            }
        }
        Ok(())
    });
    let writer_thread = std::thread::spawn(move || -> io::Result<()> {
        for msg in out_rx {
            msg.write(&mut writer)?;
            writer.flush()?;
        }
        Ok(())
    });
    let result = server::run(Connection {
        sender: out_tx,
        receiver: in_rx,
    });
    let _ = writer_thread.join();
    // The reader may still be blocked on input the client never closes; do not wait for it.
    drop(reader_thread);
    result
}
