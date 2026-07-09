//! The trigger IPC wire protocol, shared by the daemon and its clients.
//!
//! Security (ENGINEERING.md 8): the protocol is a closed set of trigger verbs
//! plus a `status` query. There is deliberately no frame that carries text, so
//! no other process can ever push text through us into the focused app. A
//! request that is not one of the known verbs is rejected, never interpreted.
//!
//! Requests and responses are single newline-terminated lines. One request,
//! one response, per connection.

use std::path::PathBuf;

use verbatim_core::runner::Trigger;

/// The socket file name inside the runtime directory.
#[cfg(not(target_os = "windows"))]
pub const SOCKET_FILE: &str = "verbatim.sock";

/// The environment override for the socket path (tests, non-default installs).
pub const SOCKET_ENV: &str = "VERBATIM_SOCK";

/// The largest request line the daemon will read before giving up. Every valid
/// request is a short verb (`toggle\n` is the longest at 7 bytes); this cap
/// keeps a same-uid client that never sends a newline from growing the read
/// buffer without bound (threat model F1). Generous vs. the real max so future
/// verbs fit without a bump.
pub const MAX_REQUEST_BYTES: u64 = 64;

/// The one and only set of verbs a client may send. `Cancel` is intentionally
/// absent: discarding is a local ESC action, not a remotely triggerable one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Start,
    Stop,
    Toggle,
}

impl Verb {
    pub fn to_trigger(self) -> Trigger {
        match self {
            Verb::Start => Trigger::Start,
            Verb::Stop => Trigger::Stop,
            Verb::Toggle => Trigger::Toggle,
        }
    }

    fn token(self) -> &'static str {
        match self {
            Verb::Start => "start",
            Verb::Stop => "stop",
            Verb::Toggle => "toggle",
        }
    }
}

/// A parsed, validated client request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Trigger(Verb),
    Status,
}

impl Request {
    /// Parse one request line. Anything outside the closed verb set is an
    /// error carrying the offending token, never a text payload to act on.
    pub fn parse(line: &str) -> Result<Request, String> {
        match line.trim() {
            "start" => Ok(Request::Trigger(Verb::Start)),
            "stop" => Ok(Request::Trigger(Verb::Stop)),
            "toggle" => Ok(Request::Trigger(Verb::Toggle)),
            "status" => Ok(Request::Status),
            other => Err(other.to_owned()),
        }
    }

    pub fn encode(self) -> String {
        match self {
            Request::Trigger(verb) => format!("{}\n", verb.token()),
            Request::Status => "status\n".to_owned(),
        }
    }
}

/// A daemon reply. State is carried as an opaque token the client relays; the
/// client never needs to reconstruct the core state enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// A trigger was accepted; the session state after applying it.
    Accepted(String),
    /// A `status` answer.
    Status(String),
    /// The request was rejected or could not be served.
    Error(String),
}

impl Response {
    pub fn encode(&self) -> String {
        match self {
            Response::Accepted(state) => format!("accepted {state}\n"),
            Response::Status(state) => format!("status {state}\n"),
            Response::Error(message) => format!("error {message}\n"),
        }
    }

    pub fn parse(line: &str) -> Response {
        let line = line.trim();
        match line.split_once(' ') {
            Some(("accepted", rest)) => Response::Accepted(rest.to_owned()),
            Some(("status", rest)) => Response::Status(rest.to_owned()),
            Some(("error", rest)) => Response::Error(rest.to_owned()),
            _ => Response::Error(format!("malformed response: {line}")),
        }
    }
}

/// The endpoint path: `$VERBATIM_SOCK` if set, else the per-user runtime dir
/// (Unix socket) or a per-user named pipe (Windows).
///
/// The Unix default lives under the platform application-support directory
/// rather than a world-writable location; the daemon further restricts the
/// socket to owner-only. `$VERBATIM_SOCK` keeps tests off the real user
/// directory.
pub fn socket_path() -> PathBuf {
    if let Ok(path) = std::env::var(SOCKET_ENV) {
        return PathBuf::from(path);
    }
    default_endpoint()
}

#[cfg(not(target_os = "windows"))]
fn default_endpoint() -> PathBuf {
    runtime_dir().join(SOCKET_FILE)
}

/// Named pipes live in a flat namespace, so the per-user scoping that a
/// home-directory path gives Unix comes from the user name in the pipe name.
#[cfg(target_os = "windows")]
fn default_endpoint() -> PathBuf {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_owned());
    PathBuf::from(format!(r"\\.\pipe\verbatim-{user}"))
}

#[cfg(target_os = "macos")]
fn runtime_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home)
            .join("Library/Application Support")
            .join("Verbatim"),
        None => std::env::temp_dir().join("Verbatim"),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("verbatim");
    }
    std::env::temp_dir().join("verbatim")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_round_trips_through_the_wire() {
        for request in [
            Request::Trigger(Verb::Start),
            Request::Trigger(Verb::Stop),
            Request::Trigger(Verb::Toggle),
            Request::Status,
        ] {
            let encoded = request.encode();
            assert!(encoded.ends_with('\n'));
            assert_eq!(Request::parse(&encoded), Ok(request));
        }
    }

    #[test]
    fn text_payloads_are_rejected_not_interpreted() {
        // The security guarantee: nothing that is not a known verb is ever
        // acted on, so no process can push text through the socket.
        for hostile in ["inject: rm -rf ~", "type hello", "start now", "STATUS", ""] {
            assert!(
                Request::parse(hostile).is_err(),
                "must reject non-verb payload: {hostile:?}"
            );
        }
    }

    #[test]
    fn parser_never_panics_and_only_the_closed_set_parses() {
        // Fuzz corpus for the wire protocol (threat model F4). The parser must
        // (a) never panic on any input and (b) accept nothing outside the four
        // exact verbs, so no crafted payload is ever interpreted as text to
        // inject. Binary/non-UTF-8 bytes cannot reach here at all - the daemon
        // reads the request with `read_line`, which errors on invalid UTF-8
        // before parse is called - so this corpus covers the UTF-8 space.
        //
        // ponytail: table-driven corpus, not cargo-fuzz. The grammar is four
        // fixed tokens; an exhaustive-by-construction table is stronger signal
        // than random bytes here. Promote to a cargo-fuzz target only if the
        // protocol ever grows arguments or structure.
        let hostile = [
            // Injection-shaped payloads: must never be treated as text.
            "inject: rm -rf ~",
            "type hello world",
            "start; rm -rf /",
            "toggle\nstart",
            "status\r\nstart",
            // Verb look-alikes and affixes.
            "START",
            "Start",
            "starts",
            "start now",
            "startstop",
            "sto",
            "stopp",
            // Empty / whitespace / control noise.
            "",
            " ",
            "\n",
            "\r\n",
            "\0",
            "start\0",
            "\u{202e}start",
            // Overlong / adversarial length (still under the read cap here, but
            // parse must not care about size).
            &"a".repeat(10_000),
            &format!("start{}", "x".repeat(10_000)),
        ];

        for input in hostile {
            // The only accepted inputs are the four exact verbs; none of these
            // are bare verbs, so parse must reject every one. A panic here means
            // the parser widened, not that the corpus is wrong.
            if let Ok(accepted) = Request::parse(input) {
                panic!("hostile payload was accepted: {input:?} -> {accepted:?}");
            }
        }

        // And the whitelist still works, including with the trailing newline the
        // wire always carries and surrounding whitespace `trim` must absorb.
        for (line, want) in [
            ("start\n", Request::Trigger(Verb::Start)),
            ("stop\n", Request::Trigger(Verb::Stop)),
            ("toggle\n", Request::Trigger(Verb::Toggle)),
            ("status\n", Request::Status),
            ("  toggle  \n", Request::Trigger(Verb::Toggle)),
        ] {
            assert_eq!(Request::parse(line), Ok(want));
        }
    }

    #[test]
    fn responses_round_trip() {
        for response in [
            Response::Accepted("Recording".to_owned()),
            Response::Status("Idle".to_owned()),
            Response::Error("nope".to_owned()),
        ] {
            assert_eq!(Response::parse(&response.encode()), response);
        }
    }
}
