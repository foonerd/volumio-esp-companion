//! The HAL's HTTP client over `ureq`.

use std::time::Duration;

use companion_hal::{HttpClient, HttpError};

/// A pooled HTTP client with rustls and Mozilla's roots compiled in.
pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    /// A client identifying itself as the simulator.
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent(concat!("companion-sim/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.new_agent(),
        }
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for Http {
    fn get(&mut self, url: &str, timeout: Duration) -> Result<Vec<u8>, HttpError> {
        let mut response = self
            .agent
            .get(url)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(map)?;
        response.body_mut().read_to_vec().map_err(map)
    }
}

fn map(e: ureq::Error) -> HttpError {
    match e {
        ureq::Error::StatusCode(code) => HttpError::Status(code),
        ureq::Error::Timeout(_) => HttpError::Timeout,
        ureq::Error::BadUri(_) => HttpError::BadUrl,
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => HttpError::Connect,
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused => {
            HttpError::Connect
        }
        other => HttpError::Io(other.to_string()),
    }
}
