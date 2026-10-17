//! Settings that survive a reboot.
//!
//! A flat key-value store of strings. On an ESP32 it is NVS; on a desktop
//! it is a file; on the Raspberry Pi it was a TOML file on the boot
//! partition. The application keeps its keys few and its values short:
//! the player's address, the rotation, the theme, the Wi-Fi credentials.
//!
//! Strings rather than typed values because every backing store speaks
//! strings and the application owns the parsing; a typed store would only
//! move the same `parse` behind a trait.

use alloc::string::String;

/// A persistent key-value store.
pub trait ConfigStore {
    /// The store's error.
    type Error: core::fmt::Debug;

    /// The value under `key`, if set.
    fn get(&self, key: &str) -> Result<Option<String>, Self::Error>;

    /// Set `key` to `value`, replacing any previous value.
    fn set(&mut self, key: &str, value: &str) -> Result<(), Self::Error>;

    /// Remove `key`. Removing an absent key is not an error.
    fn remove(&mut self, key: &str) -> Result<(), Self::Error>;
}
