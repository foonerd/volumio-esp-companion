//! The HAL's HTTP client over ESP-IDF's `esp_http_client`, with mbedTLS
//! and the certificate bundle for `https`.

use std::time::Duration;

use companion_hal::{HttpClient, HttpError};
use esp_idf_svc::http::client::{Configuration, EspHttpConnection, FollowRedirectsPolicy};
use esp_idf_svc::http::Method;
use esp_idf_svc::sys::{
    EspError, ESP_ERR_HTTP_CONNECT, ESP_ERR_HTTP_CONNECTING, ESP_ERR_HTTP_EAGAIN,
    ESP_ERR_HTTP_INVALID_TRANSPORT, ESP_ERR_HTTP_READ_TIMEOUT, ESP_ERR_INVALID_ARG,
    ESP_ERR_TIMEOUT,
};

/// The most a body may be; a cover is a few hundred kilobytes, a state
/// document a few kilobytes. Guards PSRAM against a runaway server.
pub const MAX_BODY: usize = 4 << 20;

/// One connection per request, as the player's REST shape expects.
///
/// A new connection takes the call's timeout; a pool would share one.
/// `https` verifies against ESP-IDF's bundled roots, which is what a
/// webradio's artwork host needs and a Volumio on the LAN does not.
#[derive(Debug, Default)]
pub struct Http;

impl Http {
    /// A client.
    pub fn new() -> Self {
        Self
    }
}

impl HttpClient for Http {
    fn get(&mut self, url: &str, timeout: Duration) -> Result<Vec<u8>, HttpError> {
        let mut conn = EspHttpConnection::new(&Configuration {
            timeout: Some(timeout),
            follow_redirects_policy: FollowRedirectsPolicy::FollowGetHead,
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            buffer_size: Some(4096),
            ..Default::default()
        })
        .map_err(map)?;
        conn.initiate_request(Method::Get, url, &[("Accept", "*/*")])
            .map_err(map)?;
        conn.initiate_response().map_err(map)?;
        let status = conn.status();
        let mut body = Vec::new();
        let mut buf = [0u8; 2048];
        loop {
            let n = conn.read(&mut buf).map_err(map)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
            if body.len() > MAX_BODY {
                return Err(HttpError::Io(format!("body over {MAX_BODY} bytes")));
            }
        }
        if !(200..300).contains(&status) {
            return Err(HttpError::Status(status));
        }
        Ok(body)
    }
}

/// ESP-IDF's error, as coarsely as the application wants it.
fn map(e: EspError) -> HttpError {
    match e.code() {
        ESP_ERR_HTTP_CONNECT | ESP_ERR_HTTP_CONNECTING => HttpError::Connect,
        ESP_ERR_TIMEOUT | ESP_ERR_HTTP_EAGAIN | ESP_ERR_HTTP_READ_TIMEOUT => HttpError::Timeout,
        ESP_ERR_INVALID_ARG | ESP_ERR_HTTP_INVALID_TRANSPORT => HttpError::BadUrl,
        _ => HttpError::Io(e.to_string()),
    }
}
