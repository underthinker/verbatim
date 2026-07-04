//! The trigger IPC transport: a Unix domain socket on Unix, a named pipe on
//! Windows. The wire protocol on top (`ipc.rs`) is byte-identical on both -
//! newline-terminated verb lines, one request/response per connection.
//!
//! Security (ENGINEERING.md 8): the Unix socket is chmod 0600; the named pipe
//! relies on the closed verb set (the protocol carries no text frames), with
//! a tightened DACL tracked for the M4 security review of this surface.

use std::io;
use std::path::Path;

#[cfg(unix)]
mod imp {
    use super::*;

    use tokio::net::{UnixListener, UnixStream};

    pub type ClientStream = UnixStream;

    pub struct Listener {
        inner: UnixListener,
    }

    impl Listener {
        /// Bind the listening socket owner-only, clearing any stale socket
        /// file first.
        pub fn bind(path: &Path) -> io::Result<Self> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // A leftover socket from a previous run would make bind fail with
            // EADDRINUSE.
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
            let inner = UnixListener::bind(path)?;
            restrict_to_owner(path)?;
            Ok(Self { inner })
        }

        pub async fn accept(&mut self) -> io::Result<UnixStream> {
            let (stream, _addr) = self.inner.accept().await?;
            Ok(stream)
        }
    }

    fn restrict_to_owner(path: &Path) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }

    pub async fn connect(path: &Path) -> io::Result<ClientStream> {
        UnixStream::connect(path).await
    }

    /// Remove the socket file on shutdown so the next daemon binds cleanly.
    pub fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    use std::time::Duration;

    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    pub type ClientStream = NamedPipeClient;

    /// `ERROR_PIPE_BUSY`: all server instances are mid-handshake; retry.
    const PIPE_BUSY: i32 = 231;
    const BUSY_RETRIES: u32 = 20;
    const BUSY_RETRY_DELAY: Duration = Duration::from_millis(25);

    pub struct Listener {
        name: String,
        /// The pre-created instance the next client will reach. Named pipes
        /// have no accept queue; an instance must exist *before* a client
        /// calls, so one is always kept ahead.
        next: NamedPipeServer,
    }

    impl Listener {
        pub fn bind(path: &Path) -> io::Result<Self> {
            let name = path.to_string_lossy().into_owned();
            let next = ServerOptions::new()
                // Fail like EADDRINUSE if another daemon already serves here.
                .first_pipe_instance(true)
                .create(&name)?;
            Ok(Self { name, next })
        }

        pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
            self.next.connect().await?;
            let connected =
                std::mem::replace(&mut self.next, ServerOptions::new().create(&self.name)?);
            Ok(connected)
        }
    }

    pub async fn connect(path: &Path) -> io::Result<ClientStream> {
        let name = path.to_string_lossy().into_owned();
        let mut attempt = 0;
        loop {
            match ClientOptions::new().open(&name) {
                Ok(client) => return Ok(client),
                Err(err) if err.raw_os_error() == Some(PIPE_BUSY) && attempt < BUSY_RETRIES => {
                    attempt += 1;
                    tokio::time::sleep(BUSY_RETRY_DELAY).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Named pipes vanish with their last handle; nothing to clean up.
    pub fn cleanup(_path: &Path) {}
}

#[cfg(test)]
pub use imp::ClientStream;
pub use imp::{Listener, cleanup, connect};
