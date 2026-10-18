//! The HAL's network status from the host's interfaces.
//!
//! Carried over from rpi-waveshare28 `runtime/src/net.rs` at `61ecf6d`:
//! the address filter and the ordering, without Volumio's hotspot file.
//! A desktop is never in provisioning mode.

use std::net::{IpAddr, Ipv6Addr};
use std::path::Path;

use companion_hal::{Address, HostInfo, Link, NetworkStatus};

/// The host, as the status page would show it.
#[derive(Debug, Default)]
pub struct HostNet;

impl NetworkStatus for HostNet {
    fn host_info(&mut self) -> HostInfo {
        HostInfo {
            hostname: hostname(),
            addrs: addresses(),
            hotspot: None,
        }
    }
}

/// Hostname with the mDNS suffix, since that is what a person types.
fn hostname() -> String {
    let name = gethostname::gethostname()
        .to_string_lossy()
        .trim()
        .to_string();
    if name.is_empty() {
        String::new()
    } else {
        format!("{name}.local")
    }
}

/// Usable addresses: wired before wireless, v4 before v6, then by name.
fn addresses() -> Vec<Address> {
    let Ok(ifaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut out: Vec<(bool, bool, Address)> = ifaces
        .into_iter()
        .filter_map(|i| {
            let ip = i.addr.ip();
            if !usable(&ip) {
                return None;
            }
            let wireless = is_wireless(&i.name);
            Some((
                !wireless,
                ip.is_ipv6(),
                Address {
                    iface: i.name,
                    link: if wireless {
                        Link::Wireless
                    } else {
                        Link::Wired
                    },
                    addr: ip.to_string(),
                },
            ))
        })
        .collect();
    out.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.iface.cmp(&b.2.iface))
            .then(a.2.addr.cmp(&b.2.addr))
    });
    out.dedup_by(|a, b| a.2 == b.2);
    out.into_iter().map(|(_, _, a)| a).collect()
}

/// True for an interface the kernel considers wireless. Checked against
/// sysfs rather than by name prefix; off Linux every link reads as wired.
fn is_wireless(name: &str) -> bool {
    Path::new(&format!("/sys/class/net/{name}/wireless")).exists()
}

/// True for an address someone could type into a browser.
fn usable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified() && !v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            !v6.is_loopback() && !v6.is_unspecified() && !v6.is_multicast() && !is_v6_link_local(v6)
        }
    }
}

/// fe80::/10.
fn is_v6_link_local(a: &Ipv6Addr) -> bool {
    let o = a.octets();
    o[0] == 0xfe && (o[1] & 0xc0) == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from_str(s).unwrap())
    }

    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(Ipv6Addr::from_str(s).unwrap())
    }

    #[test]
    fn keeps_routable_and_drops_the_rest() {
        assert!(usable(&v4("192.168.1.20")));
        assert!(usable(&v6("fd00::1")));
        assert!(usable(&v6("2001:db8::1")));
        assert!(!usable(&v4("127.0.0.1")));
        assert!(!usable(&v4("169.254.3.4")));
        assert!(!usable(&v6("::1")));
        assert!(!usable(&v6("fe80::1")));
    }

    #[test]
    fn the_hostname_carries_the_suffix() {
        let h = hostname();
        assert!(h.is_empty() || h.ends_with(".local"));
    }
}
