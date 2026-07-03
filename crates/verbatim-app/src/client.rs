//! Trigger clients: `verbatim trigger <verb>` and `verbatim status` connect to
//! the daemon socket, send one request, print the reply. This is how native
//! shortcut bindings drive dictation and how scripts integrate
//! (ARCHITECTURE.md 6). Unix-only for this slice, matching the daemon.

use std::path::Path;
use std::process::ExitCode;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::ipc::{Request, Response};

/// Connect, send `request`, print the daemon's reply. A missing daemon or a
/// rejected request is a non-zero exit; a served reply prints to stdout.
pub async fn run(path: &Path, request: Request) -> ExitCode {
    let stream = match UnixStream::connect(path).await {
        Ok(stream) => stream,
        Err(_) => {
            eprintln!(
                "verbatim: no running instance at {} (start one with `verbatim daemon`)",
                path.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let (reader, mut writer) = stream.into_split();
    if let Err(err) = writer.write_all(request.encode().as_bytes()).await {
        eprintln!("verbatim: could not send request: {err}");
        return ExitCode::FAILURE;
    }
    if let Err(err) = writer.flush().await {
        eprintln!("verbatim: could not send request: {err}");
        return ExitCode::FAILURE;
    }

    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    if let Err(err) = reader.read_line(&mut line).await {
        eprintln!("verbatim: no reply: {err}");
        return ExitCode::FAILURE;
    }

    match Response::parse(&line) {
        Response::Accepted(state) | Response::Status(state) => {
            println!("{state}");
            ExitCode::SUCCESS
        }
        Response::Error(message) => {
            eprintln!("verbatim: {message}");
            ExitCode::FAILURE
        }
    }
}
