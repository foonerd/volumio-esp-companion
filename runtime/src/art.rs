//! Album art: which cover to ask for, and when to ask again.
//!
//! Carried over from rpi-waveshare28 `runtime/src/art.rs` at `61ecf6d`:
//! the fetch gate, without the thread. Fetching and decoding a cover is
//! slow enough to stall the ticker and delay a touch, so the Pi did it on
//! a worker and the loop carried on. Here the application hands the device
//! an [`ArtRequest`] and takes the result back when it comes, and whether
//! the device fetches inline, on a second thread or not at all is its own
//! business. The gate's rules do not move:
//!
//! - a playing cover is not refetched every tick (that was a TLS handshake
//!   storm);
//! - a new `albumart` string is sent at once;
//! - a failed fetch retries on a backoff that opens then caps, because the
//!   first miss is often a webradio cache that has not been written yet
//!   and a permanent 404 must not be a handshake every poll;
//! - only one request is in flight: skipping through a playlist should
//!   render where the user stopped.
//!
//! The pending key is the `albumart` string from `getState`, not the
//! encoded URL. The device should drain to the newest request it holds
//! before fetching, as the Pi's worker did.

use alloc::string::String;
use core::time::Duration;

use crate::url;

/// A cover the device is asked to fetch and decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtRequest {
    /// The URL to GET, percent-encoded and ready for the HTTP client.
    pub url: String,
    /// The square the picture is to be scaled to fit, in pixels.
    pub size: u32,
}

/// Dedupes and retries fetches for the current `albumart` string.
#[derive(Debug, Clone)]
pub struct ArtGate {
    pending: Option<String>,
    in_flight: bool,
    failed: bool,
    failures: u32,
    next_retry: Option<Duration>,
}

impl Default for ArtGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ArtGate {
    /// A gate with nothing pending.
    pub fn new() -> Self {
        Self {
            pending: None,
            in_flight: false,
            failed: false,
            failures: 0,
            next_retry: None,
        }
    }

    /// Ask for `path` at `now`; a request comes back when one is due.
    ///
    /// `base` is the player's origin for relative paths; `size` the art
    /// box. The same `path` asked again while in flight, after success, or
    /// inside a backoff yields nothing.
    pub fn request(
        &mut self,
        base: &str,
        path: &str,
        size: u32,
        now: Duration,
    ) -> Option<ArtRequest> {
        self.should_send(path, now).then(|| ArtRequest {
            url: url::encode(&url::join(base, path)),
            size,
        })
    }

    /// Record the outcome of the request last handed out.
    pub fn completed(&mut self, ok: bool, now: Duration) {
        self.in_flight = false;
        if ok {
            self.failed = false;
            self.failures = 0;
            self.next_retry = None;
        } else {
            self.failed = true;
            self.failures = self.failures.saturating_add(1);
            self.next_retry = Some(now + retry_delay(self.failures));
        }
    }

    /// True after the pending cover has missed at least once and has not
    /// arrived since. A new path clears it. The face says "Retrieving
    /// artwork" on it.
    pub fn retrieving(&self) -> bool {
        self.failed
    }

    /// How many times the pending cover has failed in a row.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// The `albumart` string the gate is tracking.
    pub fn pending(&self) -> Option<&str> {
        self.pending.as_deref()
    }

    fn should_send(&mut self, path: &str, now: Duration) -> bool {
        if self.pending.as_deref() == Some(path) {
            if self.failed && !self.in_flight && self.next_retry.is_some_and(|t| now >= t) {
                self.in_flight = true;
                return true;
            }
            return false;
        }
        self.pending = Some(String::from(path));
        self.in_flight = true;
        self.failed = false;
        self.failures = 0;
        self.next_retry = None;
        true
    }
}

/// Delay before the next attempt of a failed URL.
fn retry_delay(failures: u32) -> Duration {
    match failures {
        0 | 1 => Duration::from_secs(2),
        2 => Duration::from_secs(8),
        _ => Duration::from_secs(30),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://p:3000";

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn first_path_is_sent_once() {
        let mut gate = ArtGate::new();
        let req = gate.request(BASE, "/albumart?x=1", 180, s(0)).unwrap();
        assert_eq!(req.url, "http://p:3000/albumart?x=1");
        assert_eq!(req.size, 180);
        assert!(gate.request(BASE, "/albumart?x=1", 180, s(0)).is_none());
    }

    #[test]
    fn the_url_is_encoded_but_the_key_is_not() {
        let mut gate = ArtGate::new();
        let path = "https://cdn.example/radio-thumbnails/Classic FM.jpg";
        let req = gate.request(BASE, path, 180, s(0)).unwrap();
        assert_eq!(
            req.url,
            "https://cdn.example/radio-thumbnails/Classic%20FM.jpg"
        );
        assert_eq!(gate.pending(), Some(path));
    }

    #[test]
    fn success_does_not_refetch() {
        let mut gate = ArtGate::new();
        assert!(gate.request(BASE, "/a.jpg", 1, s(0)).is_some());
        gate.completed(true, s(0));
        assert!(gate.request(BASE, "/a.jpg", 1, s(60)).is_none());
    }

    #[test]
    fn failed_url_retries_after_backoff_not_before() {
        let mut gate = ArtGate::new();
        assert!(gate.request(BASE, "Classic FM.jpg", 1, s(0)).is_some());
        gate.completed(false, s(0));
        assert_eq!(gate.failures(), 1);
        assert!(gate
            .request(BASE, "Classic FM.jpg", 1, Duration::from_millis(1999))
            .is_none());
        assert!(gate.request(BASE, "Classic FM.jpg", 1, s(2)).is_some());
    }

    #[test]
    fn in_flight_blocks_a_retry() {
        let mut gate = ArtGate::new();
        assert!(gate.request(BASE, "/a", 1, s(0)).is_some());
        gate.completed(false, s(0));
        assert!(gate.request(BASE, "/a", 1, s(2)).is_some());
        assert!(gate.request(BASE, "/a", 1, s(30)).is_none());
    }

    #[test]
    fn retrieving_is_the_gap_after_a_miss_until_the_cover_arrives() {
        let mut gate = ArtGate::new();
        assert!(gate.request(BASE, "/a.jpg", 1, s(0)).is_some());
        assert!(!gate.retrieving(), "the first try is not a wait message");
        gate.completed(false, s(0));
        assert!(gate.retrieving());
        assert!(gate.request(BASE, "/a.jpg", 1, s(1)).is_none());
        assert!(gate.retrieving(), "backoff still owes the cover");
        gate.completed(true, s(2));
        assert!(!gate.retrieving());
    }

    #[test]
    fn new_path_sends_immediately_and_resets_failures() {
        let mut gate = ArtGate::new();
        assert!(gate.request(BASE, "/old", 1, s(0)).is_some());
        gate.completed(false, s(0));
        assert_eq!(gate.failures(), 1);
        assert!(gate.request(BASE, "/new", 1, s(0)).is_some());
        assert_eq!(gate.failures(), 0);
        assert!(!gate.retrieving());
    }

    #[test]
    fn backoff_opens_then_caps() {
        assert_eq!(retry_delay(1), s(2));
        assert_eq!(retry_delay(2), s(8));
        assert_eq!(retry_delay(3), s(30));
        assert_eq!(retry_delay(9), s(30));
    }
}
