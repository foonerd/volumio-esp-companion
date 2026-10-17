//! Fetching over HTTP, as the application needs it.
//!
//! The Volumio client needs one verb: GET a URL within a timeout, get the
//! body back. A device supplies it from whatever stack it has (a raw TCP
//! client with rustls on Linux, ESP-IDF's client with mbedTLS on an
//! ESP32). Redirects, TLS and connection reuse are the implementation's
//! business; the application sees bytes or an error.

use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

/// Why a fetch failed, coarsely: enough to decide whether to retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The URL could not be parsed or has an unsupported scheme.
    BadUrl,
    /// The host did not resolve or the connection could not be opened.
    Connect,
    /// The request or response did not complete within the timeout.
    Timeout,
    /// The server answered with a non-success status.
    Status(u16),
    /// The transfer failed or the response was malformed.
    Io(String),
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadUrl => f.write_str("bad url"),
            Self::Connect => f.write_str("connect failed"),
            Self::Timeout => f.write_str("timed out"),
            Self::Status(s) => write!(f, "http status {s}"),
            Self::Io(why) => write!(f, "transfer failed: {why}"),
        }
    }
}

/// A blocking HTTP GET.
pub trait HttpClient {
    /// Fetch `url`, failing after `timeout`. Returns the body on a 2xx.
    fn get(&mut self, url: &str, timeout: Duration) -> Result<Vec<u8>, HttpError>;

    /// Fetch `url` and decode the body as UTF-8.
    fn get_text(&mut self, url: &str, timeout: Duration) -> Result<String, HttpError> {
        let body = self.get(url, timeout)?;
        String::from_utf8(body).map_err(|_| HttpError::Io(String::from("not utf-8")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Canned(Vec<u8>);

    impl HttpClient for Canned {
        fn get(&mut self, _url: &str, _timeout: Duration) -> Result<Vec<u8>, HttpError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn text_decodes_utf8_and_rejects_the_rest() {
        let t = Duration::from_secs(1);
        assert_eq!(Canned(b"ready".to_vec()).get_text("u", t).unwrap(), "ready");
        assert!(matches!(
            Canned(vec![0xff, 0xfe]).get_text("u", t),
            Err(HttpError::Io(_))
        ));
    }

    #[test]
    fn errors_display() {
        assert_eq!(HttpError::Status(404).to_string(), "http status 404");
        assert_eq!(HttpError::Timeout.to_string(), "timed out");
    }
}
