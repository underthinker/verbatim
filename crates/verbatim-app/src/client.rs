//! Trigger clients: `verbatim trigger <verb>` and `verbatim status` connect to
//! the daemon endpoint, send one request, print the reply. This is how native
//! shortcut bindings drive dictation and how scripts integrate
//! (ARCHITECTURE.md 6).

use std::path::Path;
use std::process::ExitCode;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::ipc::{Request, Response};
use crate::transport;

/// Connect, send `request`, print the daemon's reply. A missing daemon or a
/// rejected request is a non-zero exit; a served reply prints to stdout.
pub async fn run(path: &Path, request: Request) -> ExitCode {
    let stream = match transport::connect(path).await {
        Ok(stream) => stream,
        Err(_) => {
            eprintln!(
                "verbatim: no running instance at {} (start one with `verbatim daemon`)",
                path.display()
            );
            return ExitCode::FAILURE;
        }
    };

    let (reader, mut writer) = tokio::io::split(stream);
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
