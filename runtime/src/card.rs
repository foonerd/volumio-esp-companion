//! The configuration file a removable card carries.
//!
//! A device with a card slot reads `volumio.txt` from the card's root on
//! every boot and writes what it found into its settings store, so the
//! card is the way in for a user with no console and no keyboard: write
//! the file on any computer, insert, power on. The same file is read by
//! the simulator (`--card`), so there is one format and it is tested on a
//! desktop before it meets a card.
//!
//! The format is the smallest thing a text editor on any OS produces:
//! UTF-8, one `key = value` per line, blank lines and lines starting with
//! `#` or `;` ignored, CRLF or LF, a leading byte-order mark tolerated.
//! Keys are the store's ([`key`] plus the device's own), case-insensitive.
//! The value is everything after the first `=`, trimmed; a value wrapped
//! in matching double quotes keeps its inner spaces. There are no inline
//! comments, because `#` is a legal character in a password. An empty
//! value removes the key, so the default stands again.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use companion_hal::ConfigStore;

use crate::settings::key;

/// The file's name on the card's root. 8.3-safe so a card without long
/// file names still carries it.
pub const FILE_NAME: &str = "volumio.txt";
/// The report the device writes back next to it after applying.
pub const STATUS_NAME: &str = "status.txt";

/// A line that set or cleared a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The store key, lower-case.
    pub key: String,
    /// The value; empty means remove.
    pub value: String,
}

/// What a file said.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// Known keys in file order; a key repeated keeps its last line.
    pub entries: Vec<Entry>,
    /// `(line, key)` for keys the store does not know.
    pub unknown: Vec<(usize, String)>,
    /// Line numbers with no `=`.
    pub malformed: Vec<usize>,
}

impl Parsed {
    /// True when nothing in the file needs the user's attention.
    pub fn is_clean(&self) -> bool {
        self.unknown.is_empty() && self.malformed.is_empty()
    }
}

/// Parse the file's text. Never fails: a bad line is reported, the rest
/// is applied, because a device cannot refuse to boot over a typo.
pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            out.malformed.push(i + 1);
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        if !is_known(&k) {
            out.unknown.push((i + 1, k));
            continue;
        }
        let v = unquote(v.trim()).to_string();
        if let Some(e) = out.entries.iter_mut().find(|e| e.key == k) {
            e.value = v;
        } else {
            out.entries.push(Entry { key: k, value: v });
        }
    }
    out
}

/// Write every entry into `store`: set when the value is non-empty,
/// remove when it is empty. Stops at the store's first error.
pub fn apply<S: ConfigStore>(parsed: &Parsed, store: &mut S) -> Result<(), S::Error> {
    for e in &parsed.entries {
        if e.value.is_empty() {
            store.remove(&e.key)?;
        } else {
            store.set(&e.key, &e.value)?;
        }
    }
    Ok(())
}

fn is_known(k: &str) -> bool {
    key::ALL.contains(&k) || key::DEVICE.contains(&k)
}

fn unquote(v: &str) -> &str {
    match v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) if v.len() >= 2 => inner,
        _ => v,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;

    #[derive(Default)]
    struct Map(BTreeMap<String, String>);

    impl ConfigStore for Map {
        type Error = core::convert::Infallible;

        fn get(&self, key: &str) -> Result<Option<String>, Self::Error> {
            Ok(self.0.get(key).cloned())
        }

        fn set(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
            self.0.insert(key.into(), value.into());
            Ok(())
        }

        fn remove(&mut self, key: &str) -> Result<(), Self::Error> {
            self.0.remove(key);
            Ok(())
        }
    }

    const TEMPLATE: &str = include_str!("../../sdcard/volumio.txt");

    #[test]
    fn the_shipped_template_is_clean_and_sets_the_documented_keys() {
        let p = parse(TEMPLATE);
        assert!(p.is_clean(), "{p:?}");
        let keys: Vec<&str> = p.entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                key::WIFI_SSID,
                key::WIFI_PASSWORD,
                key::NAME,
                key::PLAYER,
                key::ROTATION,
                key::THEME
            ]
        );
    }

    #[test]
    fn windows_line_endings_bom_and_case_are_tolerated() {
        let p = parse("\u{feff}WIFI_SSID = Home\r\nWifi_Password = s3cret # not a comment\r\n");
        assert!(p.is_clean());
        assert_eq!(
            p.entries[0],
            Entry {
                key: "wifi_ssid".into(),
                value: "Home".into()
            }
        );
        assert_eq!(p.entries[1].value, "s3cret # not a comment");
    }

    #[test]
    fn quotes_keep_spaces_and_the_first_equals_splits() {
        let p = parse("wifi_password = \" pa=ss \"\nplayer=http://10.0.0.7:3000\n");
        assert_eq!(p.entries[0].value, " pa=ss ");
        assert_eq!(p.entries[1].value, "http://10.0.0.7:3000");
    }

    #[test]
    fn a_lone_quote_is_a_value() {
        assert_eq!(parse("wifi_password = \"\n").entries[0].value, "\"");
    }

    #[test]
    fn comments_blanks_unknowns_and_malformed_lines_are_reported_not_fatal() {
        let p = parse("# head\n\n; also\nfoo = 1\nno equals here\ntheme = dusk\n");
        assert_eq!(p.unknown, [(4, "foo".to_string())]);
        assert_eq!(p.malformed, [5]);
        assert_eq!(
            p.entries,
            [Entry {
                key: "theme".into(),
                value: "dusk".into()
            }]
        );
    }

    #[test]
    fn a_repeated_key_keeps_the_last_line() {
        let p = parse("theme = ink\ntheme = night\n");
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].value, "night");
    }

    #[test]
    fn apply_sets_and_an_empty_value_removes() {
        let mut store = Map::default();
        store.set(key::ROTATION, "90").unwrap();
        apply(&parse("theme = dusk\nrotation =\n"), &mut store).unwrap();
        assert_eq!(store.get(key::THEME).unwrap().as_deref(), Some("dusk"));
        assert_eq!(store.get(key::ROTATION).unwrap(), None);
    }
}
