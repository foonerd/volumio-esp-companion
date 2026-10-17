//! Finding a player on the network.
//!
//! Volumio advertises itself over mDNS as `_Volumio._tcp` on port 3000,
//! with the player's name as the instance name and its UUID in the TXT
//! record. A device that can browse mDNS implements this; one that cannot
//! returns nothing and the application falls back to a configured address.

use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

/// The mDNS service type Volumio advertises.
pub const VOLUMIO_SERVICE: &str = "_Volumio._tcp";

/// A player found on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// The name the player was given, as advertised.
    pub name: String,
    /// Host name or address to reach it at.
    pub host: String,
    /// TCP port of its HTTP interface; Volumio uses 3000.
    pub port: u16,
    /// The player's UUID from the TXT record, if present.
    pub uuid: Option<String>,
}

/// An mDNS browser for Volumio players.
pub trait Discovery {
    /// The browser's error.
    type Error: core::fmt::Debug;

    /// Browse for `timeout` and return every player seen.
    fn find_players(&mut self, timeout: Duration) -> Result<Vec<Player>, Self::Error>;
}

/// A device without mDNS: finds nothing, never fails.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDiscovery;

impl Discovery for NoDiscovery {
    type Error = core::convert::Infallible;

    fn find_players(&mut self, _timeout: Duration) -> Result<Vec<Player>, Self::Error> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_discovery_finds_nothing_and_never_fails() {
        let found = NoDiscovery.find_players(Duration::from_secs(1)).unwrap();
        assert!(found.is_empty());
    }
}
