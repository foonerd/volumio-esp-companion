//! The settings, and the store keys they live under.
//!
//! Carried over from rpi-waveshare28 `runtime/src/config.rs` at `61ecf6d`:
//! the keys that shaped the face and named the player, with the Pi's
//! device plumbing (SPI node, pins, framebuffer) left behind; the device
//! owns that. Values are read from the HAL's [`ConfigStore`] as strings
//! and parsed here; a value that does not parse is logged and the default
//! stands, because a device cannot refuse to boot over a typo the way the
//! Pi's service could.

use alloc::string::String;
use core::time::Duration;

use companion_core::{BarGap, Layout, StatusText, Strip, Theme};
use companion_hal::ConfigStore;

/// Store keys. One spelling, used by every device and by the settings UI.
///
/// Every key is at most [`key::MAX_KEY_LEN`] bytes: ESP-IDF's NVS, the store on
/// every ESP32, refuses longer names, and a key that works on one device
/// and not another is not one spelling.
pub mod key {
    /// The longest key any store must accept (NVS: 15 bytes plus the NUL).
    pub const MAX_KEY_LEN: usize = 15;

    /// The player's origin: `http://host:3000`.
    pub const PLAYER: &str = "player";
    /// Rotation in degrees clockwise: 0, 90, 180 or 270.
    pub const ROTATION: &str = "rotation";
    /// Colour theme: `ink`, `dusk`, `studio`, `night`.
    pub const THEME: &str = "theme";
    /// Status-screen type on portrait rotations.
    pub const STATUS_TEXT_PORTRAIT: &str = "text_portrait";
    /// Status-screen type on landscape rotations.
    pub const STATUS_TEXT_LANDSCAPE: &str = "text_landscape";
    /// Bar gap on portrait rotations.
    pub const BAR_GAP_PORTRAIT: &str = "gap_portrait";
    /// Bar gap on landscape rotations.
    pub const BAR_GAP_LANDSCAPE: &str = "gap_landscape";
    /// Strip occupancy on portrait rotations.
    pub const STRIP_PORTRAIT: &str = "strip_portrait";
    /// Strip occupancy on landscape rotations.
    pub const STRIP_LANDSCAPE: &str = "strip_landscape";
    /// State poll interval, milliseconds.
    pub const POLL_MS: &str = "poll_ms";
    /// Minimum gap between two accepted press-downs, milliseconds.
    pub const TOUCH_DEBOUNCE_MS: &str = "debounce_ms";

    /// Every key, for a store or a settings page to iterate.
    pub const ALL: &[&str] = &[
        PLAYER,
        ROTATION,
        THEME,
        STATUS_TEXT_PORTRAIT,
        STATUS_TEXT_LANDSCAPE,
        BAR_GAP_PORTRAIT,
        BAR_GAP_LANDSCAPE,
        STRIP_PORTRAIT,
        STRIP_LANDSCAPE,
        POLL_MS,
        TOUCH_DEBOUNCE_MS,
    ];

    /// The network to join. Not in [`ALL`]: a device's firmware reads
    /// these, the application and its settings page never do.
    pub const WIFI_SSID: &str = "wifi_ssid";
    /// The network's password; absent or empty is an open network.
    pub const WIFI_PASSWORD: &str = "wifi_password";
    /// The credential keys.
    pub const CREDENTIALS: &[&str] = &[WIFI_SSID, WIFI_PASSWORD];
}

/// What the application needs to know before it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The player's origin, `http://host:port`.
    pub player: String,
    /// Rotation in degrees clockwise.
    pub rotation: u16,
    /// Colour tokens.
    pub theme: Theme,
    /// Status type on portrait rotations.
    pub status_text_portrait: StatusText,
    /// Status type on landscape rotations.
    pub status_text_landscape: StatusText,
    /// Bar gap on portrait rotations.
    pub bar_gap_portrait: BarGap,
    /// Bar gap on landscape rotations.
    pub bar_gap_landscape: BarGap,
    /// Strip on portrait rotations.
    pub strip_portrait: Strip,
    /// Strip on landscape rotations.
    pub strip_landscape: Strip,
    /// How often to poll the player state.
    pub poll_interval: Duration,
    /// Minimum gap between two accepted press-downs.
    pub touch_debounce: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            player: String::from("http://volumio.local:3000"),
            rotation: 0,
            theme: Theme::Ink,
            status_text_portrait: StatusText::Normal,
            status_text_landscape: StatusText::Normal,
            bar_gap_portrait: BarGap::Default,
            bar_gap_landscape: BarGap::Default,
            strip_portrait: Strip::Progress,
            strip_landscape: Strip::Progress,
            poll_interval: Duration::from_millis(500),
            touch_debounce: Duration::from_millis(50),
        }
    }
}

impl Settings {
    /// Read every key from `store`, defaulting what is absent or malformed.
    pub fn load<S: ConfigStore>(store: &S) -> Self {
        let mut s = Self::default();
        let read = |k: &str| -> Option<String> {
            match store.get(k) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("settings: reading {k}: {e:?}");
                    None
                }
            }
        };
        if let Some(v) = read(key::PLAYER) {
            let v = v.trim();
            if v.starts_with("http://") || v.starts_with("https://") {
                s.player = String::from(v.trim_end_matches('/'));
            } else {
                log::warn!(
                    "settings: {} must start with http:// or https://, got {v:?}",
                    key::PLAYER
                );
            }
        }
        if let Some(v) = read(key::ROTATION) {
            match v.trim().parse::<u16>() {
                Ok(r @ (0 | 90 | 180 | 270)) => s.rotation = r,
                _ => log::warn!(
                    "settings: {} must be 0, 90, 180 or 270, got {v:?}",
                    key::ROTATION
                ),
            }
        }
        parse_into(&mut s.theme, key::THEME, read(key::THEME), Theme::parse);
        parse_into(
            &mut s.status_text_portrait,
            key::STATUS_TEXT_PORTRAIT,
            read(key::STATUS_TEXT_PORTRAIT),
            StatusText::parse,
        );
        parse_into(
            &mut s.status_text_landscape,
            key::STATUS_TEXT_LANDSCAPE,
            read(key::STATUS_TEXT_LANDSCAPE),
            StatusText::parse,
        );
        parse_into(
            &mut s.bar_gap_portrait,
            key::BAR_GAP_PORTRAIT,
            read(key::BAR_GAP_PORTRAIT),
            BarGap::parse,
        );
        parse_into(
            &mut s.bar_gap_landscape,
            key::BAR_GAP_LANDSCAPE,
            read(key::BAR_GAP_LANDSCAPE),
            BarGap::parse,
        );
        parse_into(
            &mut s.strip_portrait,
            key::STRIP_PORTRAIT,
            read(key::STRIP_PORTRAIT),
            Strip::parse,
        );
        parse_into(
            &mut s.strip_landscape,
            key::STRIP_LANDSCAPE,
            read(key::STRIP_LANDSCAPE),
            Strip::parse,
        );
        if let Some(v) = read(key::POLL_MS) {
            match v.trim().parse::<u64>() {
                Ok(ms) if ms > 0 => s.poll_interval = Duration::from_millis(ms),
                _ => log::warn!(
                    "settings: {} must be a positive integer, got {v:?}",
                    key::POLL_MS
                ),
            }
        }
        if let Some(v) = read(key::TOUCH_DEBOUNCE_MS) {
            match v.trim().parse::<u64>() {
                Ok(ms) => s.touch_debounce = Duration::from_millis(ms),
                _ => log::warn!(
                    "settings: {} must be an integer, got {v:?}",
                    key::TOUCH_DEBOUNCE_MS
                ),
            }
        }
        s
    }

    /// True for 90 and 270.
    pub fn landscape(&self) -> bool {
        matches!(self.rotation, 90 | 270)
    }

    /// Status type for the configured rotation.
    pub fn status_text(&self) -> StatusText {
        if self.landscape() {
            self.status_text_landscape
        } else {
            self.status_text_portrait
        }
    }

    /// Bar gap for the configured rotation.
    pub fn bar_gap(&self) -> BarGap {
        if self.landscape() {
            self.bar_gap_landscape
        } else {
            self.bar_gap_portrait
        }
    }

    /// Strip occupancy for the configured rotation.
    pub fn strip(&self) -> Strip {
        if self.landscape() {
            self.strip_landscape
        } else {
            self.strip_portrait
        }
    }

    /// The layout these settings compose.
    pub fn layout(&self) -> Layout {
        Layout::compose(
            self.rotation,
            self.bar_gap(),
            self.status_text(),
            self.strip(),
            self.theme,
        )
    }
}

fn parse_into<T: Copy>(slot: &mut T, key: &str, raw: Option<String>, parse: fn(&str) -> Option<T>) {
    if let Some(v) = raw {
        match parse(&v) {
            Some(t) => *slot = t,
            None => log::warn!("settings: {key} has no value {v:?}, keeping the default"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    struct Map(Vec<(&'static str, &'static str)>);

    impl ConfigStore for Map {
        type Error = core::convert::Infallible;

        fn get(&self, key: &str) -> Result<Option<String>, Self::Error> {
            Ok(self
                .0
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| String::from(*v)))
        }

        fn set(&mut self, _key: &str, _value: &str) -> Result<(), Self::Error> {
            Ok(())
        }

        fn remove(&mut self, _key: &str) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn an_empty_store_is_the_defaults() {
        assert_eq!(Settings::load(&Map(Vec::new())), Settings::default());
    }

    #[test]
    fn every_key_fits_an_nvs_name() {
        for k in key::ALL.iter().chain(key::CREDENTIALS) {
            assert!(k.len() <= key::MAX_KEY_LEN, "{k} is {} bytes", k.len());
            assert!(k.is_ascii(), "{k}");
        }
        assert_eq!(key::ALL.len(), 11);
        assert_eq!(key::CREDENTIALS.len(), 2);
    }

    #[test]
    fn every_key_is_read() {
        let s = Settings::load(&Map(alloc::vec![
            (key::PLAYER, "http://10.0.0.7:3000/"),
            (key::ROTATION, "270"),
            (key::THEME, "night"),
            (key::STATUS_TEXT_PORTRAIT, "large"),
            (key::STATUS_TEXT_LANDSCAPE, "normal"),
            (key::BAR_GAP_PORTRAIT, "tight"),
            (key::BAR_GAP_LANDSCAPE, "roomy"),
            (key::STRIP_PORTRAIT, "off"),
            (key::STRIP_LANDSCAPE, "stream"),
            (key::POLL_MS, "250"),
            (key::TOUCH_DEBOUNCE_MS, "80"),
        ]));
        assert_eq!(s.player, "http://10.0.0.7:3000");
        assert_eq!(s.rotation, 270);
        assert_eq!(s.theme, Theme::Night);
        assert_eq!(s.status_text(), StatusText::Normal);
        assert_eq!(s.bar_gap(), BarGap::Roomy);
        assert_eq!(s.strip(), Strip::Stream);
        assert_eq!(s.poll_interval, Duration::from_millis(250));
        assert_eq!(s.touch_debounce, Duration::from_millis(80));
        let l = s.layout();
        assert_eq!(l.rotation, 270);
        assert_eq!(l.theme, Theme::Night);
        assert_eq!(l.strip, Strip::Stream);
    }

    #[test]
    fn a_bad_value_keeps_the_default() {
        let s = Settings::load(&Map(alloc::vec![
            (key::PLAYER, "volumio.local"),
            (key::ROTATION, "45"),
            (key::THEME, "neon"),
            (key::POLL_MS, "0"),
        ]));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn portrait_picks_the_portrait_values() {
        let s = Settings {
            rotation: 180,
            strip_portrait: Strip::Off,
            strip_landscape: Strip::Stream,
            ..Settings::default()
        };
        assert_eq!(s.strip(), Strip::Off);
    }
}
