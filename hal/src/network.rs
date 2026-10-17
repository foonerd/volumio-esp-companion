//! The device's own network state, as the status page shows it.
//!
//! The data types are shared with the pages; the gathering is the device's.
//! On the Raspberry Pi the addresses came from the kernel and the hotspot
//! from Volumio's wireless daemon; on an ESP32 they come from the Wi-Fi
//! driver and the provisioning state.

use alloc::string::String;
use alloc::vec::Vec;

/// Wired or wireless, for the word on the status page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// Ethernet.
    Wired,
    /// Wi-Fi.
    Wireless,
}

impl Link {
    /// What a person should read, not the kernel's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Wired => "LAN",
            Self::Wireless => "Wi-Fi",
        }
    }
}

/// One address, with the interface it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// Interface name, kept for a stable sort. The page uses [`Link::label`].
    pub iface: String,
    /// Wired or wireless.
    pub link: Link,
    /// The address, formatted for display.
    pub addr: String,
}

/// An access point the device is broadcasting for provisioning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotspot {
    /// SSID being broadcast.
    pub ssid: String,
    /// The access point's own address.
    pub addr: String,
}

/// Hostname and network state, as displayed.
///
/// Addresses and the hotspot are independent: a device can hold an address
/// on one interface while broadcasting a provisioning network on another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostInfo {
    /// Hostname with the mDNS suffix, because that is what people type.
    pub hostname: String,
    /// Usable addresses, wired first.
    pub addrs: Vec<Address>,
    /// Present while a provisioning access point is up.
    pub hotspot: Option<Hotspot>,
}

impl HostInfo {
    /// True when there is something a person can type or join.
    pub fn has_address(&self) -> bool {
        !self.addrs.is_empty() || self.hotspot.is_some()
    }
}

/// The device's view of its own network.
pub trait NetworkStatus {
    /// The current state. Called on a cadence; cheap to answer.
    fn host_info(&mut self) -> HostInfo;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_or_a_hotspot_counts() {
        let mut h = HostInfo::default();
        assert!(!h.has_address());
        h.hotspot = Some(Hotspot {
            ssid: "Volumio".into(),
            addr: "192.168.211.1".into(),
        });
        assert!(h.has_address());
        h.hotspot = None;
        h.addrs.push(Address {
            iface: "wlan0".into(),
            link: Link::Wireless,
            addr: "10.0.0.7".into(),
        });
        assert!(h.has_address());
    }

    #[test]
    fn link_labels_are_what_people_read() {
        assert_eq!(Link::Wired.label(), "LAN");
        assert_eq!(Link::Wireless.label(), "Wi-Fi");
    }
}
