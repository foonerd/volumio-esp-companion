//! The Volumio REST client, over whatever HTTP the device has.
//!
//! Carried over from rpi-waveshare28 `runtime/src/state.rs` at `61ecf6d`:
//! `StateSource`, `CommandSink` and `poll_system_status`, folded into one
//! client generic over [`HttpClient`]. The endpoints are fixed paths under
//! the player's origin: `/status`, `/api/v1/getState`, `/api/v1/commands/`.
//!
//! Every call is blocking and bounded by the timeout; the loop tolerates a
//! failure by keeping what is on screen, so none of these is fatal.

use alloc::format;
use alloc::string::String;
use core::time::Duration;

use companion_core::{Command, PlayerState, SystemStatus};
use companion_hal::{HttpClient, HttpError};

/// Why a call to the player failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// The fetch failed.
    Http(HttpError),
    /// The body was not the JSON the state endpoint publishes.
    Json(String),
}

impl core::fmt::Display for ClientError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "{e}"),
            Self::Json(why) => write!(f, "bad state json: {why}"),
        }
    }
}

impl From<HttpError> for ClientError {
    fn from(e: HttpError) -> Self {
        Self::Http(e)
    }
}

/// A player's address and the time this client gives each call.
///
/// Measured on a Pi 3A+, `getState` answers in 5 to 11 ms; two seconds is
/// generous headroom for a loaded player without being long enough to
/// stack requests behind a stalled one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// `http://host:port`, no trailing slash.
    base: String,
    timeout: Duration,
}

impl Client {
    /// A client for the player at `base` (`http://host:3000`).
    pub fn new(base: &str, timeout: Duration) -> Self {
        Self {
            base: String::from(base.trim_end_matches('/')),
            timeout,
        }
    }

    /// The origin relative art paths are joined to.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The URL `/status` is read from.
    pub fn status_url(&self) -> String {
        format!("{}/status", self.base)
    }

    /// The URL the player state is read from.
    pub fn state_url(&self) -> String {
        format!("{}/api/v1/getState", self.base)
    }

    /// The URL a command is sent to.
    pub fn command_url(&self, cmd: Command) -> String {
        format!("{}/api/v1/commands/?{}", self.base, cmd.query())
    }

    /// Read `/status`. `None` if the player is unreachable or the body is
    /// not a status word.
    pub fn status<H: HttpClient>(&self, http: &mut H) -> Option<SystemStatus> {
        let body = http.get_text(&self.status_url(), self.timeout).ok()?;
        SystemStatus::parse(&body)
    }

    /// Fetch the current player state.
    pub fn state<H: HttpClient>(&self, http: &mut H) -> Result<PlayerState, ClientError> {
        let body = http.get_text(&self.state_url(), self.timeout)?;
        PlayerState::parse(&body).map_err(ClientError::Json)
    }

    /// Send a command. The response body is discarded; only success matters.
    pub fn send<H: HttpClient>(&self, http: &mut H, cmd: Command) -> Result<(), ClientError> {
        http.get(&self.command_url(cmd), self.timeout)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    struct Canned {
        body: Result<&'static str, HttpError>,
        asked: Vec<String>,
    }

    impl HttpClient for Canned {
        fn get(&mut self, url: &str, _timeout: Duration) -> Result<Vec<u8>, HttpError> {
            self.asked.push(String::from(url));
            self.body.clone().map(|b| b.as_bytes().to_vec())
        }
    }

    fn canned(body: Result<&'static str, HttpError>) -> Canned {
        Canned {
            body,
            asked: Vec::new(),
        }
    }

    #[test]
    fn urls_hang_off_the_origin() {
        let c = Client::new("http://volumio.local:3000/", Duration::from_secs(2));
        assert_eq!(c.base(), "http://volumio.local:3000");
        assert_eq!(c.status_url(), "http://volumio.local:3000/status");
        assert_eq!(c.state_url(), "http://volumio.local:3000/api/v1/getState");
        assert_eq!(
            c.command_url(Command::Volume(40)),
            "http://volumio.local:3000/api/v1/commands/?cmd=volume&volume=40"
        );
    }

    #[test]
    fn status_reads_the_word_and_tolerates_failure() {
        let c = Client::new("http://p:3000", Duration::from_secs(1));
        assert_eq!(
            c.status(&mut canned(Ok("ready"))),
            Some(SystemStatus::Ready)
        );
        assert_eq!(
            c.status(&mut canned(Ok("starting"))),
            Some(SystemStatus::Starting)
        );
        assert_eq!(c.status(&mut canned(Err(HttpError::Connect))), None);
        assert_eq!(c.status(&mut canned(Ok("<html>"))), None);
    }

    #[test]
    fn state_parses_and_reports_bad_json() {
        let c = Client::new("http://p:3000", Duration::from_secs(1));
        let s = c
            .state(&mut canned(Ok(
                r#"{"status":"play","title":"T","volume":40}"#,
            )))
            .unwrap();
        assert_eq!(s.title.as_deref(), Some("T"));
        assert_eq!(s.volume, Some(40));
        assert!(matches!(
            c.state(&mut canned(Ok("nope"))),
            Err(ClientError::Json(_))
        ));
        assert_eq!(
            c.state(&mut canned(Err(HttpError::Status(503)))),
            Err(ClientError::Http(HttpError::Status(503)))
        );
    }

    #[test]
    fn send_asks_the_command_url_once() {
        let c = Client::new("http://p:3000", Duration::from_secs(1));
        let mut h = canned(Ok(""));
        c.send(&mut h, Command::Next).unwrap();
        assert_eq!(h.asked, ["http://p:3000/api/v1/commands/?cmd=next"]);
    }
}
