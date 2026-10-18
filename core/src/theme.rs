//! The named choices a page is drawn with.
//!
//! Carried over from rpi-waveshare28 `runtime/src/config.rs` at `61ecf6d`:
//! the four enums the layout and the palette take. The settings that pick
//! them per orientation live in the runtime; these are only the values.

use serde::{Deserialize, Serialize};

/// Type size on the status page.
///
/// `large` is the biggest stock mono font (`FONT_10X20`). A literal 2x of
/// the meta face would be 12x20 and an IPv6 address would be 468 px, which
/// is wider than either frame. Large mode wraps; it does not scale one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusText {
    /// The title and meta faces.
    #[default]
    Normal,
    /// `FONT_10X20` for both, wrapping.
    Large,
}

impl StatusText {
    /// The settings-store spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
        }
    }

    /// The type size a settings-store spelling names, if any.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "large" => Some(Self::Large),
            _ => None,
        }
    }
}

/// Named gap step between the volume and progress bars.
///
/// The face layout ignores this: the boxes are fixed. The key stays so a
/// settings store cannot smuggle a typo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarGap {
    /// The smallest gap.
    Tight,
    /// The shipped gap.
    #[default]
    Default,
    /// The largest gap.
    Roomy,
}

impl BarGap {
    /// The settings-store spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tight => "tight",
            Self::Default => "default",
            Self::Roomy => "roomy",
        }
    }

    /// The gap a settings-store spelling names, if any.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "tight" => Some(Self::Tight),
            "default" => Some(Self::Default),
            "roomy" => Some(Self::Roomy),
            _ => None,
        }
    }
}

/// What occupies the seek slot on the resting face.
///
/// Default is the progress bar. `stream` paints the IN fields the source
/// wrote (`trackType` / `codec` / `bitdepth` / `samplerate` / `bitrate`).
/// `off` hides the slot and grows art. Never invents an ALSA OUT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strip {
    /// The progress bar.
    #[default]
    Progress,
    /// The stream's format line.
    Stream,
    /// Nothing; art takes the room.
    Off,
}

impl Strip {
    /// The settings-store spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Stream => "stream",
            Self::Off => "off",
        }
    }

    /// The strip a settings-store spelling names, if any.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "progress" => Some(Self::Progress),
            "stream" => Some(Self::Stream),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// Named colour set. Tokens, not a CSS engine. `ink` is the shipped panel;
/// `dusk`, `studio` and `night` are the table words. Live after set; no
/// reboot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// The shipped set.
    #[default]
    Ink,
    /// Dusk.
    Dusk,
    /// Studio.
    Studio,
    /// Night.
    Night,
}

impl Theme {
    /// The settings-store spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ink => "ink",
            Self::Dusk => "dusk",
            Self::Studio => "studio",
            Self::Night => "night",
        }
    }

    /// The theme a settings-store spelling names, if any.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ink" => Some(Self::Ink),
            "dusk" => Some(Self::Dusk),
            "studio" => Some(Self::Studio),
            "night" => Some(Self::Night),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_spellings_round_trip() {
        for t in [Theme::Ink, Theme::Dusk, Theme::Studio, Theme::Night] {
            assert_eq!(Theme::parse(t.as_str()), Some(t));
        }
        assert_eq!(Theme::parse(" Night "), Some(Theme::Night));
        assert_eq!(Theme::parse("neon"), None);
    }

    #[test]
    fn the_other_spellings_round_trip_too() {
        for s in [StatusText::Normal, StatusText::Large] {
            assert_eq!(StatusText::parse(s.as_str()), Some(s));
        }
        for g in [BarGap::Tight, BarGap::Default, BarGap::Roomy] {
            assert_eq!(BarGap::parse(g.as_str()), Some(g));
        }
        for s in [Strip::Progress, Strip::Stream, Strip::Off] {
            assert_eq!(Strip::parse(s.as_str()), Some(s));
        }
        assert_eq!(Strip::parse("OFF"), Some(Strip::Off));
        assert_eq!(BarGap::parse("wide"), None);
    }

    #[test]
    fn defaults_are_the_shipped_panel() {
        assert_eq!(Theme::default(), Theme::Ink);
        assert_eq!(Strip::default(), Strip::Progress);
        assert_eq!(StatusText::default(), StatusText::Normal);
        assert_eq!(BarGap::default(), BarGap::Default);
    }
}
