//! The HAL's network status over the Wi-Fi driver's interfaces.
//!
//! Wraps the blocking Wi-Fi so the firmware has one thing to start,
//! connect and hand to the application. The status page reads the
//! station's address and hostname; a provisioning access point, when the
//! firmware runs one, shows as the hotspot.

use companion_hal::{Address, HostInfo, Hotspot, Link, NetworkStatus};
use core::time::Duration;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::ipv4::{
    ClientConfiguration as IpClientConfiguration, Configuration as IpConfiguration,
    DHCPClientSettings,
};
use esp_idf_svc::netif::{EspNetif, NetifConfiguration, NetifStack};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{EspError, ESP_ERR_INVALID_ARG};
use esp_idf_svc::wifi::{
    AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi, ScanMethod, WifiDriver,
};

/// The Wi-Fi, started, as the application's view of the network.
pub struct WifiNet<'d> {
    wifi: BlockingWifi<EspWifi<'d>>,
}

/// Where a station last found its access point. Offered back to
/// [`WifiNet::connect`], it lets the driver go straight to that channel and
/// BSSID instead of searching both bands, which is most of a cold boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApHint {
    /// The primary channel.
    pub channel: u8,
    /// The access point's MAC.
    pub bssid: [u8; 6],
}

/// How long a full search may take before `connect` gives up. The
/// driver's own figure for both bands is around thirteen seconds.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a hinted join may take: the AP is either there or it has
/// moved, and the caller falls back to the search.
const HINTED_TIMEOUT: Duration = Duration::from_secs(10);

impl<'d> WifiNet<'d> {
    /// Take the modem, start the driver in station mode with no network
    /// yet. Scanning works from here; `connect` joins one. `hostname` is
    /// what the station tells DHCP, so the router's device list shows it;
    /// `None` keeps ESP-IDF's default (`espressif`). Nothing answers for
    /// it as `.local` until an mDNS responder is built in. Longer than the
    /// netif accepts (30) is `ESP_ERR_INVALID_ARG`.
    pub fn start(
        modem: Modem<'d>,
        sys_loop: EspSystemEventLoop,
        nvs: EspDefaultNvsPartition,
        hostname: Option<&str>,
    ) -> Result<Self, EspError> {
        let driver = WifiDriver::new(modem, sys_loop.clone(), Some(nvs))?;
        let sta = match hostname {
            Some(name) => {
                let name = name
                    .try_into()
                    .map_err(|_| EspError::from_infallible::<ESP_ERR_INVALID_ARG>())?;
                EspNetif::new_with_conf(&NetifConfiguration {
                    ip_configuration: Some(IpConfiguration::Client(IpClientConfiguration::DHCP(
                        DHCPClientSettings {
                            hostname: Some(name),
                        },
                    ))),
                    ..NetifConfiguration::wifi_default_client()
                })?
            }
            None => EspNetif::new(NetifStack::Sta)?,
        };
        let wifi = EspWifi::wrap_all(driver, sta, EspNetif::new(NetifStack::Ap)?)?;
        let mut wifi = BlockingWifi::wrap(wifi, sys_loop)?;
        wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))?;
        wifi.start()?;
        Ok(Self { wifi })
    }

    /// Join `ssid` and wait for an address. An empty password is an open
    /// network. Without `hint` the driver scans every channel on both bands
    /// and joins the strongest match, which takes it about thirteen seconds
    /// and is worth it once. With `hint` it scans from that channel for
    /// that BSSID and stops at the first sight of it, in about a second;
    /// after [`HINTED_TIMEOUT`] it has moved and the caller falls back to
    /// the search. A failed hinted join leaves the driver ready for one.
    pub fn connect(
        &mut self,
        ssid: &str,
        password: &str,
        hint: Option<ApHint>,
    ) -> Result<(), EspError> {
        let auth = if password.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        };
        let mut conf = ClientConfiguration {
            auth_method: auth,
            ..Default::default()
        };
        // heapless strings: a name too long for the driver is not a name.
        if conf.ssid.push_str(ssid).is_err() || conf.password.push_str(password).is_err() {
            return Err(EspError::from_infallible::<{ ESP_ERR_INVALID_ARG }>());
        }
        let timeout = match hint {
            Some(h) => {
                conf.channel = Some(h.channel);
                conf.bssid = Some(h.bssid);
                // The default is a complete scan of both bands, the thirteen
                // seconds the hint exists to save.
                conf.scan_method = ScanMethod::FastScan;
                HINTED_TIMEOUT
            }
            None => SEARCH_TIMEOUT,
        };
        // A previous attempt may still be pending in the driver.
        let _ = self.wifi.wifi_mut().disconnect();
        self.wifi.set_configuration(&Configuration::Client(conf))?;
        self.wifi.wifi_mut().connect()?;
        self.wifi.wifi_wait_while(
            || self.wifi.wifi().is_connected().map(|c| !c),
            Some(timeout),
        )?;
        self.wifi.wait_netif_up()
    }

    /// The access point the station is joined to, as a hint for next time.
    pub fn joined_ap(&self) -> Option<ApHint> {
        self.wifi.wifi().get_ap_info().ok().map(|ap| ApHint {
            channel: ap.channel,
            bssid: ap.bssid,
        })
    }

    /// The driver, for what the application does not need: scans, band
    /// mode, the raw interfaces.
    pub fn wifi(&self) -> &BlockingWifi<EspWifi<'d>> {
        &self.wifi
    }

    /// The driver, mutably.
    pub fn wifi_mut(&mut self) -> &mut BlockingWifi<EspWifi<'d>> {
        &mut self.wifi
    }
}

impl NetworkStatus for WifiNet<'_> {
    fn host_info(&mut self) -> HostInfo {
        let sta = self.wifi.wifi().sta_netif();
        // The bare name: there is no mDNS responder in this build, so a
        // `.local` suffix would name something that does not answer.
        let hostname = sta
            .get_hostname()
            .map(|h| h.as_str().to_string())
            .unwrap_or_default();
        let mut addrs = Vec::new();
        if self.wifi.is_up().unwrap_or(false) {
            if let Ok(info) = sta.get_ip_info() {
                if !info.ip.is_unspecified() {
                    addrs.push(Address {
                        iface: "wlan0".into(),
                        link: Link::Wireless,
                        addr: info.ip.to_string(),
                    });
                }
            }
        }
        let hotspot = match self.wifi.get_configuration() {
            Ok(Configuration::AccessPoint(ap)) | Ok(Configuration::Mixed(_, ap)) => self
                .wifi
                .wifi()
                .ap_netif()
                .get_ip_info()
                .ok()
                .map(|info| Hotspot {
                    ssid: ap.ssid.to_string(),
                    addr: info.ip.to_string(),
                }),
            _ => None,
        };
        HostInfo {
            hostname,
            addrs,
            hotspot,
        }
    }
}
