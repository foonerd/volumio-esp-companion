//! Volumio playback state: the model and the protocol's shapes.
//!
//! Carried over from rpi-waveshare28 `runtime/src/state.rs` at `61ecf6d`.
//! The model, its parsing quirks and the command query strings are here;
//! the fetching (the state poll, the command sink, the status poll) lives
//! in the runtime, generic over the HAL's HTTP client.
//!
//! The Pi runtime polled the REST endpoint rather than holding a socket.io
//! connection. The trade is latency against dependency weight: socket.io in
//! Rust means either a heavy client or hand-rolling the protocol, and a
//! half-second poll is indistinguishable from push on a display a person
//! glances at. If this ever needs to be push-driven, MPD's idle command over
//! a plain TCP socket is the lighter route, not socket.io.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::Deserialize;

/// Integer fields arrive as ints, floats, or numeric strings.
///
/// A type mismatch used to fail the whole poll. RP2 seek/duration are
/// floats (`35080.993`, `323.038`); `"323.038".parse::<u64>()` is `None`,
/// which left progress blank on a playing track. Truncate toward zero.
fn parse_loose_int<T: core::str::FromStr>(raw: &str) -> Option<T> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(v) = raw.parse() {
        return Some(v);
    }
    let f: f64 = raw.parse().ok()?;
    if !f.is_finite() {
        return None;
    }
    // `as u64` truncates toward zero and saturates, which is the
    // intent; `f64::trunc` is std-only.
    format!("{}", f.max(0.0) as u64).parse().ok()
}

fn opt_from_number_or_string<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: core::str::FromStr,
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Number(n)) => parse_loose_int(&n.to_string()),
        Some(serde_json::Value::String(s)) => parse_loose_int(&s),
        Some(_) => None,
    })
}

/// Display fields arrive as strings, numbers, or empty strings. A number
/// must not fail the poll: some sources send `bitrate: 320`.
fn opt_loose_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(_) => None,
    })
}

/// Service names that appear in `trackType`. They are not a codec.
fn is_service_type(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "airplay" | "webradio" | "spotify" | "tidal" | "qobuz" | "upnp" | "bluetooth" | "bt"
    )
}

/// A codec token is one short word (`flac`, `mp3`). RP2's default
/// `showChannel` writes the channel name into `trackType` ("The Main Mix").
fn looks_like_codec(s: &str) -> bool {
    let t = s.trim().trim_end_matches('-').trim();
    !t.is_empty()
        && t.len() <= 16
        && !is_service_type(t)
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-')
}

fn format_codec<'a>(track_type: Option<&'a str>, codec: Option<&'a str>) -> Option<&'a str> {
    let tt = track_type.map(str::trim).filter(|s| !s.is_empty());
    let c = codec.map(str::trim).filter(|s| !s.is_empty());
    match (tt, c) {
        (Some(t), _) if looks_like_codec(t) => Some(t),
        (_, Some(c)) if looks_like_codec(c) => Some(c),
        _ => None,
    }
}

fn looks_like_bitrate(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("bps")
}

fn format_bitrate(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_digit()) {
        format!("{s} kbps")
    } else {
        s.to_string()
    }
}

/// The subset of Volumio's state we render.
///
/// Field names match the API. Everything is optional because the endpoint
/// omits fields rather than nulling them, and a missing title should show an
/// empty line rather than fail the poll.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PlayerState {
    /// play, pause, stop.
    pub status: Option<String>,
    /// Track title.
    pub title: Option<String>,
    /// Artist name.
    pub artist: Option<String>,
    /// Album name.
    pub album: Option<String>,
    /// Album art path, relative to the Volumio host or absolute.
    #[serde(rename = "albumart")]
    pub album_art: Option<String>,
    /// Elapsed position. Volumio reports this in milliseconds.
    #[serde(default, deserialize_with = "opt_from_number_or_string")]
    pub seek: Option<u64>,
    /// Track length in seconds. Consume/webradio often sends this as `"0"`.
    #[serde(default, deserialize_with = "opt_from_number_or_string")]
    pub duration: Option<u64>,
    /// Sample rate as a display string, e.g. "44.1 kHz".
    ///
    /// Radio Paradise (RP2) copies bitrate here so the Web UI has a place
    /// to print it. That string is IN, not a sample rate.
    #[serde(default, deserialize_with = "opt_loose_string")]
    pub samplerate: Option<String>,
    /// Bit depth as a display string, e.g. "16 bit".
    #[serde(default, deserialize_with = "opt_loose_string")]
    pub bitdepth: Option<String>,
    /// Codec or container when the source wrote one, e.g. "flac".
    #[serde(default, rename = "trackType", deserialize_with = "opt_loose_string")]
    pub track_type: Option<String>,
    /// Alternate codec field. RP2 writes this; MPD usually does not.
    #[serde(default, deserialize_with = "opt_loose_string")]
    pub codec: Option<String>,
    /// Bitrate as a display string or a bare number.
    #[serde(default, deserialize_with = "opt_loose_string")]
    pub bitrate: Option<String>,
    /// Output volume, 0 to 100. The API sends a number or a string.
    #[serde(default, deserialize_with = "opt_from_number_or_string")]
    pub volume: Option<u8>,
    /// Mute state.
    pub mute: Option<bool>,
    /// Shuffle. From getState; REST can toggle it.
    #[serde(default)]
    pub random: Option<bool>,
    /// Repeat all. From getState; REST can toggle it.
    #[serde(default)]
    pub repeat: Option<bool>,
    /// Repeat one. From getState only: REST cannot set repeatSingle.
    #[serde(default, rename = "repeatSingle")]
    pub repeat_single: Option<bool>,
    /// Source plugin name, e.g. `mpd`, `webradio`. Not an ALSA device.
    #[serde(default, deserialize_with = "opt_loose_string")]
    pub service: Option<String>,
}

impl PlayerState {
    /// True when the player is actively playing.
    pub fn is_playing(&self) -> bool {
        self.status.as_deref() == Some("play")
    }

    /// True when output is muted.
    pub fn is_muted(&self) -> bool {
        self.mute.unwrap_or(false)
    }

    /// Elapsed fraction of the track, 0.0 to 1.0, if both fields are present.
    ///
    /// Note the unit mismatch in the API: `seek` is milliseconds and
    /// `duration` is seconds. Getting this wrong makes the progress bar jump
    /// to full within the first second, which is exactly how the bug presents.
    pub fn progress(&self) -> Option<f32> {
        let (seek_ms, dur_s) = (self.seek?, self.duration?);
        if dur_s == 0 {
            return None;
        }
        let dur_ms = dur_s.saturating_mul(1000);
        Some((seek_ms.min(dur_ms) as f32) / (dur_ms as f32))
    }

    /// IN format line the source authored. Empty when nothing is known.
    ///
    /// Never invents PCM, webradio, or an ALSA OUT. Service names in
    /// `trackType` are skipped. RP2's bitrate-in-samplerate hack is shown
    /// as written.
    pub fn stream_info(&self) -> Option<String> {
        let codec = format_codec(self.track_type.as_deref(), self.codec.as_deref());
        let depth = self
            .bitdepth
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let rate = self
            .samplerate
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let br = self
            .bitrate
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let mut parts = Vec::new();
        if let Some(c) = codec {
            parts.push(c.to_string());
        }
        if let Some(r) = rate {
            if looks_like_bitrate(r) {
                parts.push(r.to_string());
            } else {
                if let Some(d) = depth {
                    parts.push(d.to_string());
                }
                parts.push(r.to_string());
            }
        } else if let Some(d) = depth {
            parts.push(d.to_string());
            if let Some(b) = br {
                parts.push(format_bitrate(b));
            }
        } else if let Some(b) = br {
            parts.push(format_bitrate(b));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("  "))
        }
    }

    /// True when two states describe the same screen apart from the parts
    /// that are repainted individually.
    ///
    /// `seek` advances every second while playing, so comparing whole states
    /// would flicker. Volume, mute and modes sit on the dock and surfaces, so
    /// they gate a full redraw. Progress is still painted in place.
    pub fn same_scene(&self, other: &Self) -> bool {
        self.status == other.status
            && self.title == other.title
            && self.artist == other.artist
            && self.album == other.album
            && self.duration == other.duration
            && self.samplerate == other.samplerate
            && self.bitdepth == other.bitdepth
            && self.track_type == other.track_type
            && self.codec == other.codec
            && self.bitrate == other.bitrate
            && self.volume == other.volume
            && self.mute == other.mute
            && self.random == other.random
            && self.repeat == other.repeat
            && self.repeat_single == other.repeat_single
            && self.service == other.service
    }
}

/// Body of `GET /status`. Set by the backend at boot: `starting` until
/// plugins finish plus seven seconds, then `ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatus {
    /// Express is up, plugins are still loading.
    Starting,
    /// `BOOT COMPLETED`. Safe to call getState for the player screen.
    Ready,
    /// Backend reported `error`.
    Error,
}

impl SystemStatus {
    /// Parse the plain-text `/status` body. Unknown values are `None`.
    pub fn parse(body: &str) -> Option<Self> {
        match body.trim() {
            "ready" => Some(Self::Ready),
            "starting" => Some(Self::Starting),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Playback commands the panel can issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Previous track.
    Prev,
    /// Toggle between play and pause.
    Toggle,
    /// Next track.
    Next,
    /// Set volume to a percentage.
    Volume(u8),
    /// Seek to a position in seconds.
    Seek(u32),
    /// Mute output. Volumio restores the previous level on unmute.
    Mute,
    /// Unmute output.
    Unmute,
    /// Toggle shuffle. REST has no set-absolute without a value dance.
    Random,
    /// Toggle repeat-all. REST cannot set repeatSingle (`one`).
    Repeat,
}

impl Command {
    /// The query string Volumio expects, after the base URL.
    pub fn query(self) -> String {
        match self {
            Command::Prev => "cmd=prev".into(),
            Command::Toggle => "cmd=toggle".into(),
            Command::Next => "cmd=next".into(),
            Command::Volume(v) => format!("cmd=volume&volume={}", v.min(100)),
            Command::Seek(secs) => format!("cmd=seek&position={secs}"),
            Command::Mute => "cmd=volume&volume=mute".into(),
            Command::Unmute => "cmd=volume&volume=unmute".into(),
            Command::Random => "cmd=random".into(),
            Command::Repeat => "cmd=repeat".into(),
        }
    }
}

#[cfg(test)]
mod status_tests {
    use super::SystemStatus;

    #[test]
    fn parses_the_three_known_bodies() {
        assert_eq!(SystemStatus::parse("ready\n"), Some(SystemStatus::Ready));
        assert_eq!(
            SystemStatus::parse("starting"),
            Some(SystemStatus::Starting)
        );
        assert_eq!(SystemStatus::parse("error"), Some(SystemStatus::Error));
        assert_eq!(SystemStatus::parse("nope"), None);
    }
}

#[cfg(test)]
mod state_tests {
    use super::PlayerState;

    fn parse(json: &str) -> PlayerState {
        serde_json::from_str(json).expect(json)
    }

    #[test]
    fn volume_accepts_a_number_or_a_string() {
        assert_eq!(parse(r#"{"volume":40}"#).volume, Some(40));
        assert_eq!(parse(r#"{"volume":"40"}"#).volume, Some(40));
        assert_eq!(parse(r#"{"volume":" 40 "}"#).volume, Some(40));
        assert_eq!(parse(r#"{"volume":null}"#).volume, None);
        assert_eq!(parse(r#"{}"#).volume, None);
        assert_eq!(parse(r#"{"volume":""}"#).volume, None);
        assert_eq!(parse(r#"{"volume":"loud"}"#).volume, None);
    }

    #[test]
    fn seek_and_duration_accept_a_number_or_a_string() {
        let state = parse(r#"{"seek":1234,"duration":"180"}"#);
        assert_eq!(state.seek, Some(1234));
        assert_eq!(state.duration, Some(180));
        assert_eq!(parse(r#"{"duration":"0"}"#).duration, Some(0));
    }

    #[test]
    fn seek_and_duration_accept_a_float() {
        // RP2 playing body from pi5dev: seek ms and duration s are floats.
        let state = parse(r#"{"seek":35080.99300000002,"duration":323.038}"#);
        assert_eq!(state.seek, Some(35080));
        assert_eq!(state.duration, Some(323));
        let p = state.progress().expect("progress");
        assert!((p - 35080.0 / 323000.0).abs() < 0.0001);
    }

    #[test]
    fn rp2_playing_body_from_pi5dev() {
        let state = parse(
            r#"{
                "status":"play",
                "title":"Visions of You",
                "artist":"Jah Wobble",
                "album":"Rising Above Bedlam",
                "trackType":"The Main Mix",
                "codec":"flac",
                "seek":35080.99300000002,
                "duration":323.038,
                "volatile":true,
                "service":"rp2"
            }"#,
        );
        assert_eq!(state.status.as_deref(), Some("play"));
        assert_eq!(state.duration, Some(323));
        assert_eq!(state.seek, Some(35080));
        assert!(state.progress().is_some());
        // Channel name is not a codec; format is in `codec`. No rate on this body.
        assert_eq!(state.stream_info().as_deref(), Some("flac"));

        let later = parse(
            r#"{
                "trackType":"The Main Mix - ",
                "codec":"flac",
                "samplerate":"44.1 kHz",
                "bitdepth":"16-bit",
                "duration":323.038
            }"#,
        );
        assert_eq!(
            later.stream_info().as_deref(),
            Some("flac  16-bit  44.1 kHz")
        );
    }

    #[test]
    fn string_volume_does_not_fail_the_poll() {
        // The Pi 5 getState body that left the panel on the status screen.
        let state =
            parse(r#"{"status":"play","title":"Track","artist":"A","volume":"40","mute":false}"#);
        assert_eq!(state.status.as_deref(), Some("play"));
        assert_eq!(state.volume, Some(40));
        assert!(!state.is_muted());
    }

    #[test]
    fn same_scene_ignores_albumart_and_seek() {
        // Art is requested from the current poll, not from a scene change.
        // Seek advances every second while playing; gating on it flickers.
        let a = parse(
            r#"{"status":"play","title":"Classic FM","albumart":"https://cdn/Classic FM.jpg","seek":1,"duration":0}"#,
        );
        let b = parse(
            r#"{"status":"play","title":"Classic FM","albumart":"https://cdn/Classic%20FM.jpg","seek":2000,"duration":0}"#,
        );
        assert!(a.same_scene(&b));
        let c = parse(r#"{"status":"pause","title":"Classic FM","seek":2000,"duration":0}"#);
        assert!(!a.same_scene(&c));
    }

    #[test]
    fn format_fields_accept_a_number_or_a_string() {
        let state = parse(r#"{"bitrate":320,"samplerate":"44.1 kHz","trackType":"flac"}"#);
        assert_eq!(state.bitrate.as_deref(), Some("320"));
        assert_eq!(state.samplerate.as_deref(), Some("44.1 kHz"));
        assert_eq!(state.track_type.as_deref(), Some("flac"));
        assert_eq!(parse(r#"{"bitrate":""}"#).bitrate, None);
    }

    #[test]
    fn stream_info_follows_what_the_source_wrote() {
        let local = parse(r#"{"trackType":"flac","bitdepth":"16 bit","samplerate":"44.1 kHz"}"#);
        assert_eq!(
            local.stream_info().as_deref(),
            Some("flac  16 bit  44.1 kHz")
        );

        // RP2: codec from format, bitrate stuffed into samplerate, bitrate cleared.
        let rp2 = parse(r#"{"codec":"flac","samplerate":"320 kbps"}"#);
        assert_eq!(rp2.stream_info().as_deref(), Some("flac  320 kbps"));

        // RP2 showChannel default: trackType is the channel, codec is flac.
        let channel = parse(r#"{"trackType":"The Main Mix","codec":"flac"}"#);
        assert_eq!(channel.stream_info().as_deref(), Some("flac"));

        let numbered = parse(r#"{"trackType":"mp3","bitrate":128}"#);
        assert_eq!(numbered.stream_info().as_deref(), Some("mp3  128 kbps"));

        // Selection / MPD webradio: service name is not a codec, and the
        // consume path wipes rate/depth. Do not invent PCM.
        let radio = parse(r#"{"trackType":"webradio","duration":"0"}"#);
        assert_eq!(radio.stream_info(), None);

        let empty = parse(r#"{}"#);
        assert_eq!(empty.stream_info(), None);
    }

    #[test]
    fn command_query_matches_the_rest_contract() {
        use super::Command;
        assert_eq!(Command::Seek(42).query(), "cmd=seek&position=42");
        assert_eq!(Command::Mute.query(), "cmd=volume&volume=mute");
        assert_eq!(Command::Unmute.query(), "cmd=volume&volume=unmute");
        assert_eq!(Command::Random.query(), "cmd=random");
        assert_eq!(Command::Repeat.query(), "cmd=repeat");
    }

    #[test]
    fn same_scene_includes_volume_mute_and_modes() {
        let a = parse(r#"{"status":"play","title":"T","volume":40,"mute":false,"random":false}"#);
        let b = parse(
            r#"{"status":"play","title":"T","volume":40,"mute":false,"random":false,"seek":2000}"#,
        );
        assert!(a.same_scene(&b));
        let c = parse(r#"{"status":"play","title":"T","volume":42,"mute":false,"random":false}"#);
        assert!(!a.same_scene(&c));
        let d = parse(r#"{"status":"play","title":"T","volume":40,"mute":true,"random":false}"#);
        assert!(!a.same_scene(&d));
        let e = parse(r#"{"status":"play","title":"T","volume":40,"mute":false,"random":true}"#);
        assert!(!a.same_scene(&e));
    }

    #[test]
    fn same_scene_includes_format_fields() {
        let a = parse(r#"{"status":"play","title":"T","codec":"flac","seek":1}"#);
        let b = parse(r#"{"status":"play","title":"T","codec":"flac","seek":2000}"#);
        assert!(a.same_scene(&b));
        let c = parse(r#"{"status":"play","title":"T","codec":"mp3","seek":2000}"#);
        assert!(!a.same_scene(&c));
    }
}
