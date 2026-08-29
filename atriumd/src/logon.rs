//! Logon conversations with the authority, on behalf of a browser.
//!
//! atriumd is a PGSS Logon client exactly as `login` is (authd/login), with
//! the terminal replaced by a browser at the far end of atrium-server. It
//! authors `LogonStart` itself — the browser proposes nothing — relays the
//! authority's prompts out and the answers back, and keeps the terminal
//! outcome: on `AccessGranted` the token stays here, in the session table,
//! and never travels further.
//!
//! One conversation is one connection to `/run/logon.sock` (PGSS Logon §5).
//! The conversation is driven one step at a time from the server's requests:
//! start → (prompt → answer)* → granted | denied.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use atrium_proto::{Answer as ProtoAnswer, Message, Prompt, Reply};
use libauthd::transport::{recv_message_with_fd, send_message};
use libauthd::wire::{
    self, AccessDenied, Answer, CredentialResponse, CredentialType, Denial, IdentifierType, LogonStart, LogonType,
    MSG_ACCESS_DENIED, MSG_ACCESS_GRANTED, MSG_CREDENTIAL_REQUEST, decode_access_denied, decode_access_granted,
    decode_credential_request, decode_header, encode_credential_response, encode_logon_start,
};
use libauthd::{LOGON_SOCKET_PATH, Secret};
use peios::token::Token;

use crate::log;
use crate::spawn;

/// How long to wait on the authority for any one reply.
const AUTHORITY_TIMEOUT: Duration = Duration::from_secs(30);

/// A logged-on principal with a session host running as them.
///
/// The token is held for the session's life: it is what a future
/// re-spawn, an elevation, or peinit's jobs API (which replaces the spawn
/// here) needs, and closing it is what ends the logon session once the
/// host is gone.
#[allow(dead_code)]
pub struct Session {
    pub token: Token,
    pub username: String,
    pub profile: wire::Profile,
    pub pid: libc::pid_t,
    pub pidfd: OwnedFd,
}

struct Conversation {
    socket: UnixStream,
    username: String,
}

#[derive(Default)]
pub struct Logon {
    conversations: HashMap<u64, Conversation>,
    pub sessions: HashMap<u64, Session>,
    /// Set alongside a `Granted` reply: the descriptor that rides with it.
    pub pending_fd: Option<OwnedFd>,
    /// `--unrestricted`: spawn session hosts without installing a token.
    pub unrestricted: bool,
}

/// Which denials leave the page usable for another try (the same rule as
/// `login`'s `retryable`).
fn retryable(d: Denial) -> bool {
    matches!(d, Denial::AuthenticationFailed)
}

impl Logon {
    pub fn new(unrestricted: bool) -> Self {
        Logon { unrestricted, ..Default::default() }
    }

    pub fn start(&mut self, conv: u64, username: String, remote: String) -> Reply {
        if self.conversations.contains_key(&conv) {
            return Reply::Error { conv, reason: "conversation already open".into() };
        }
        let socket = match UnixStream::connect(LOGON_SOCKET_PATH) {
            Ok(s) => s,
            Err(e) => {
                log::error(format_args!("logon authority unreachable: {e}"));
                return Reply::Error { conv, reason: "the logon authority is unreachable".into() };
            }
        };
        let _ = socket.set_read_timeout(Some(AUTHORITY_TIMEOUT));
        let _ = socket.set_write_timeout(Some(AUTHORITY_TIMEOUT));

        let start = LogonStart {
            // Network for now. The type is a group SID on the token, so this
            // is an access-control decision deferred, not a label — see the
            // Atrium project's open questions.
            logon_type: LogonType::Network,
            identifier_type: IdentifierType::Username,
            identifier: username.as_bytes().to_vec(),
            tty: None,
            remote_host: Some(remote),
            supported_credential_types: vec![CredentialType::Password],
        };
        let encoded = match encode_logon_start(&start) {
            Ok(e) => e,
            Err(e) => return Reply::Error { conv, reason: format!("could not encode logon: {e:?}") },
        };
        if let Err(e) = send_message(&socket, &encoded) {
            return Reply::Error { conv, reason: format!("could not send logon: {e}") };
        }
        self.conversations.insert(conv, Conversation { socket, username });
        self.step(conv)
    }

    pub fn answer(&mut self, conv: u64, answers: Vec<ProtoAnswer>) -> Reply {
        let Some(c) = self.conversations.get(&conv) else {
            return Reply::Error { conv, reason: "no such conversation".into() };
        };
        let response = CredentialResponse {
            answers: answers
                .into_iter()
                .map(|a| Answer { credential_ref: a.credential_ref, data: Secret::from_slice(a.data.as_bytes()) })
                .collect(),
        };
        let encoded = match encode_credential_response(&response) {
            Ok(e) => e,
            Err(e) => {
                self.conversations.remove(&conv);
                return Reply::Error { conv, reason: format!("could not encode answers: {e:?}") };
            }
        };
        if let Err(e) = send_message(&c.socket, encoded.expose()) {
            self.conversations.remove(&conv);
            return Reply::Error { conv, reason: format!("could not send answers: {e}") };
        }
        self.step(conv)
    }

    pub fn abort(&mut self, conv: u64) -> Reply {
        // Dropping the socket ends the conversation on the authority's side.
        self.conversations.remove(&conv);
        Reply::Ok
    }

    pub fn logout(&mut self, session: u64) -> Reply {
        // The host is signalled; the entry (and the token) go when the
        // main loop reaps it, so the logon session outlives the process by
        // exactly as long as it takes to die.
        match self.sessions.get(&session) {
            Some(s) => {
                log::info(format_args!("session {session} ({}) logged out; terminating pid {}", s.username, s.pid));
                spawn::terminate(s.pid);
            }
            None => log::warn(format_args!("logout of unknown session {session}")),
        }
        Reply::Ok
    }

    /// A session host has exited: forget it and release its token.
    pub fn ended(&mut self, session: u64, how: &str) {
        if let Some(s) = self.sessions.remove(&session) {
            log::info(format_args!("session {session} ({}) ended: {how}", s.username));
        }
    }

    /// Read the authority's next message for `conv` and turn it into a reply.
    /// Terminal outcomes close the conversation.
    fn step(&mut self, conv: u64) -> Reply {
        let c = self.conversations.get(&conv).expect("conversation exists");
        let (message, descriptor) = match recv_message_with_fd(&wire::FRAMING, &c.socket) {
            Ok(m) => m,
            Err(e) => {
                self.conversations.remove(&conv);
                return Reply::Error { conv, reason: format!("lost the authority: {e}") };
            }
        };
        let kind = match decode_header(message.expose()) {
            Ok((kind, _)) => kind,
            Err(e) => {
                self.conversations.remove(&conv);
                return Reply::Error { conv, reason: format!("malformed reply from the authority: {e:?}") };
            }
        };
        match kind {
            MSG_CREDENTIAL_REQUEST => match decode_credential_request(message.expose()) {
                Ok(req) => Reply::Prompt {
                    conv,
                    messages: req
                        .messages
                        .iter()
                        .map(|m| Message { severity: format!("{:?}", m.severity).to_lowercase(), text: m.text.clone() })
                        .collect(),
                    prompts: req
                        .prompts
                        .iter()
                        .map(|p| Prompt {
                            credential_ref: p.credential_ref,
                            credential_type: match p.credential_type {
                                CredentialType::Password => "password".into(),
                            },
                            name: p.credential_name.clone(),
                        })
                        .collect(),
                },
                Err(e) => {
                    self.conversations.remove(&conv);
                    Reply::Error { conv, reason: format!("malformed credential request: {e:?}") }
                }
            },
            MSG_ACCESS_GRANTED => {
                let c = self.conversations.remove(&conv).expect("conversation exists");
                let granted = match decode_access_granted(message.expose()) {
                    Ok(g) => g,
                    Err(e) => return Reply::Error { conv, reason: format!("malformed grant: {e:?}") },
                };
                // A grant with no descriptor is a failed logon (PGSS Logon §9).
                let Some(descriptor): Option<OwnedFd> = descriptor else {
                    log::error(format_args!("authority granted {} but sent no token", c.username));
                    return Reply::Denied { conv, retryable: false, reason: "the authority sent no token".into() };
                };
                let session = granted.session_id;
                let token = Token::from(descriptor);
                log::info(format_args!("session {session} established for {}", c.username));
                let spawned = match spawn::spawn_session(
                    session,
                    if self.unrestricted { None } else { Some(&token) },
                    &c.username,
                    &granted.profile,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error(format_args!("could not start a session host for {}: {e}", c.username));
                        // The token drops here, which ends the logon session.
                        return Reply::Error { conv, reason: "could not start a session".into() };
                    }
                };
                let display_name = if granted.profile.display_name.is_empty() {
                    c.username.clone()
                } else {
                    granted.profile.display_name.clone()
                };
                self.sessions.insert(
                    session,
                    Session { token, username: c.username.clone(), profile: granted.profile, pid: spawned.pid, pidfd: spawned.pidfd },
                );
                self.pending_fd = Some(spawned.server_end);
                Reply::Granted { conv, session, username: c.username, display_name }
            }
            MSG_ACCESS_DENIED => {
                let c = self.conversations.remove(&conv).expect("conversation exists");
                match decode_access_denied(message.expose()) {
                    Ok(AccessDenied { denial, reason }) => {
                        log::info(format_args!("logon of {} denied: {denial:?}", c.username));
                        // The browser sees one uniform reason for credential
                        // failures; the specific denial goes to the log only.
                        let shown = if retryable(denial) {
                            "Incorrect username or password".to_string()
                        } else if reason.is_empty() {
                            format!("{denial:?}")
                        } else {
                            reason
                        };
                        Reply::Denied { conv, retryable: retryable(denial), reason: shown }
                    }
                    Err(e) => Reply::Error { conv, reason: format!("malformed denial: {e:?}") },
                }
            }
            other => {
                self.conversations.remove(&conv);
                Reply::Error { conv, reason: format!("unexpected message from the authority: {other:#06x}") }
            }
        }
    }
}
