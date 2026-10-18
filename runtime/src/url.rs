//! URL assembly for the Volumio endpoints and the album-art path.
//!
//! Carried over from rpi-waveshare28 `runtime/src/http.rs` and `art.rs` at
//! `61ecf6d`: the request-target encoding and the art-path join. The Pi
//! encoded the path as it wrote the request line, so the string Volumio
//! published stayed the key the art gate compared. Here the HTTP client is
//! the device's and takes a URL, so the encoding happens when the URL is
//! built for it, and the gate still keys on the published string.

use alloc::format;
use alloc::string::String;

/// Turn an `albumart` value from the state API into a URL to fetch.
///
/// Relative paths join to the player's origin. Absolute ones are fetched
/// directly: Volumio's `/albumart?url=` is not a proxy for remote art (it
/// returns the default artwork whatever the parameter), and the browser UI
/// shows station logos because the browser loads the absolute URL itself.
pub fn join(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return String::from(path);
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Percent-encode the path and query of `url`, leaving scheme and
/// authority as they are.
///
/// Volumio's webradio Selection concatenates `station.title` into a URL
/// and leaves spaces in it; a device HTTP client given that string either
/// rejects it or sends a malformed request line.
pub fn encode(url: &str) -> String {
    let split = url
        .find("://")
        .map(|i| i + 3)
        .and_then(|start| url[start..].find('/').map(|i| start + i));
    match split {
        Some(i) => {
            let mut out = String::from(&url[..i]);
            out.push_str(&encode_request_target(&url[i..]));
            out
        }
        None => String::from(url),
    }
}

/// Encode an origin-form request-target.
///
/// Leaves RFC 3986 unreserved, sub-delims, and the structural `/ ? : @`
/// bytes as they stand, and leaves a valid `%HH` sequence alone so a URL
/// that is already encoded is not turned into `%2520`. Everything else,
/// including space, becomes `%XX`.
pub fn encode_request_target(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if is_unencoded_request_byte(b) {
            out.push(b as char);
            i += 1;
        } else if b == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push('%');
            out.push(bytes[i + 1] as char);
            out.push(bytes[i + 2] as char);
            i += 3;
        } else {
            out.push_str(&format!("%{b:02X}"));
            i += 1;
        }
    }
    out
}

/// Bytes that are legal in an origin-form request-target without encoding.
fn is_unencoded_request_byte(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'@'
            | b'/'
            | b'?'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_relative_path() {
        assert_eq!(
            join("http://localhost:3000", "/albumart?x=1"),
            "http://localhost:3000/albumart?x=1"
        );
    }

    #[test]
    fn tolerates_trailing_and_leading_slashes() {
        assert_eq!(join("http://h/", "/a"), "http://h/a");
        assert_eq!(join("http://h", "a"), "http://h/a");
    }

    #[test]
    fn passes_absolute_urls_through() {
        assert_eq!(
            join("http://localhost:3000", "https://cdn.example/logo.png"),
            "https://cdn.example/logo.png"
        );
    }

    #[test]
    fn join_keeps_the_published_space() {
        // Encoding is `encode`'s job at fetch time; the gate compares this.
        let published = "https://radio-directory.firebaseapp.com/volumio/src/images/radio-thumbnails/Classic FM.jpg";
        assert_eq!(join("http://localhost:3000", published), published);
    }

    #[test]
    fn getstate_path_is_unchanged() {
        assert_eq!(
            encode_request_target("/api/v1/getState"),
            "/api/v1/getState"
        );
        assert_eq!(
            encode_request_target("/api/v1/commands/?cmd=volume&volume=40"),
            "/api/v1/commands/?cmd=volume&volume=40"
        );
    }

    #[test]
    fn encodes_classic_fm_selection_space() {
        assert_eq!(
            encode_request_target("/volumio/src/images/radio-thumbnails/Classic FM.jpg"),
            "/volumio/src/images/radio-thumbnails/Classic%20FM.jpg"
        );
    }

    #[test]
    fn does_not_double_encode_percent_sequences() {
        assert_eq!(
            encode_request_target("/albumart?web=Classic%20FM.jpg"),
            "/albumart?web=Classic%20FM.jpg"
        );
    }

    #[test]
    fn encodes_a_lone_percent_and_keeps_plus() {
        assert_eq!(encode_request_target("/a%b+c"), "/a%25b+c");
        assert_eq!(encode_request_target("/%6"), "/%256");
    }

    #[test]
    fn encode_leaves_scheme_and_authority_alone() {
        assert_eq!(
            encode("https://radio-directory.firebaseapp.com/volumio/src/images/radio-thumbnails/Classic FM.jpg"),
            "https://radio-directory.firebaseapp.com/volumio/src/images/radio-thumbnails/Classic%20FM.jpg"
        );
        assert_eq!(
            encode("http://volumio.local:3000/albumart?web=Classic FM.jpg"),
            "http://volumio.local:3000/albumart?web=Classic%20FM.jpg"
        );
        assert_eq!(
            encode("http://volumio.local:3000"),
            "http://volumio.local:3000"
        );
    }
}
