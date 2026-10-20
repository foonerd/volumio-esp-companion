//! The HAL's network status over the Wi-Fi driver's interfaces.
//!
//! Wraps the blocking Wi-Fi so the firmware has one thing to start,
//! connect and hand to the application. The status page reads the
//! station's address and hostname; a provisioning access point, when the
//! firmware runs one, shows as the hotspot.

use companion_hal::{Address, ConfigStore, HostInfo, Hotspot, Link, NetworkStatus};
use core::time::Duration;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::ipv4::{
    ClientConfiguration as IpClientConfiguration, Configuration as IpConfiguration,
    DHCPClientSettings,
};
use esp_idf_svc::netif::{EspNetif, NetifConfiguration, NetifStack};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{self, EspError, ESP_ERR_INVALID_ARG};
use esp_idf_svc::wifi::{
    AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi, ScanMethod, WifiDriver,
};
use std::time::Instant;

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

/// The bands the station searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// 2.4 GHz only.
    TwoPointFour,
    /// 5 GHz only.
    Five,
    /// Both, the driver's default.
    Both,
}

impl Band {
    /// The band a channel number is on.
    pub fn of_channel(channel: u8) -> Band {
        if channel > 14 {
            Band::Five
        } else {
            Band::TwoPointFour
        }
    }

    fn mode(self) -> sys::wifi_band_mode_t {
        match self {
            Band::TwoPointFour => sys::wifi_band_mode_t_WIFI_BAND_MODE_2G_ONLY,
            Band::Five => sys::wifi_band_mode_t_WIFI_BAND_MODE_5G_ONLY,
            Band::Both => sys::wifi_band_mode_t_WIFI_BAND_MODE_AUTO,
        }
    }
}

/// How long a full search may take before `connect` gives up. The
/// driver's own figure for both bands is around thirteen seconds.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a hinted join may take: the AP is either there or it has
/// moved, and the caller falls back to the search.
const HINTED_TIMEOUT: Duration = Duration::from_secs(10);

/// Store keys for the remembered access point: where the station last
/// found its network, so the next boot goes straight there, and the key
/// derived for it, so no boot derives it again.
const HINT_SSID: &str = "hint_ssid";
const HINT_CHANNEL: &str = "hint_channel";
const HINT_BSSID: &str = "hint_bssid";
const HINT_PSK: &str = "hint_psk";
/// Set when the network took the passphrase and not the key: a WPA3
/// network, whose SAE exchange needs the passphrase itself.
const HINT_SAE: &str = "hint_sae";

/// WPA2's pairwise master key for `ssid` and `password`, as the driver
/// takes it: 64 hex characters in the password's place, which it uses
/// as the key instead of deriving one. The derivation (PBKDF2-SHA1,
/// 4096 rounds) is a few hundred milliseconds of arithmetic; the driver
/// does it in its own task at a priority above everything else in the
/// firmware, and a player's audio ran dry while it did. Done here, it
/// runs at the caller's priority.
pub fn psk(ssid: &str, password: &str) -> String {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), ssid.as_bytes(), 4096, &mut key);
    key.iter().map(|b| format!("{b:02x}")).collect()
}

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

    /// Join `ssid` the quick way when the store remembers where it was
    /// (one band, one channel, one BSSID: about a second), else by the
    /// full search; then remember where it landed for next time. The band
    /// is left on both afterwards. A password is handed to the driver as
    /// the key derived from it ([`psk`]), remembered too; a network that
    /// refuses the key is offered the passphrase, which a WPA3 network
    /// needs for its SAE exchange, and which of the two it took is
    /// remembered.
    pub fn join<S: ConfigStore>(
        &mut self,
        store: &mut S,
        ssid: &str,
        password: &str,
    ) -> Result<(), EspError> {
        let remembered = read_hint(store, ssid);
        let mut secret = Secret::for_network(store, ssid, password);
        let hinted = remembered.and_then(|h| {
            self.set_band(Band::of_channel(h.channel));
            let started = Instant::now();
            let outcome = self.connect(ssid, secret.as_password(password), Some(h));
            self.set_band(Band::Both);
            match outcome {
                Ok(()) => {
                    log::info!(
                        "wifi: joined by hint {h:?} in {} ms",
                        started.elapsed().as_millis()
                    );
                    Some(())
                }
                Err(e) => {
                    log::info!("wifi: hint {h:?} did not answer ({e}); searching");
                    None
                }
            }
        });
        if hinted.is_none() {
            let started = Instant::now();
            let mut outcome = self.connect(ssid, secret.as_password(password), None);
            while outcome.is_err() && secret.next(ssid, password) {
                log::info!("wifi: refused; trying {}", secret.name());
                outcome = self.connect(ssid, secret.as_password(password), None);
            }
            outcome?;
            log::info!(
                "wifi: joined by search with the {} in {} ms",
                secret.name(),
                started.elapsed().as_millis()
            );
        }
        if let Some(now) = self.joined_ap() {
            if remembered != Some(now) {
                write_hint(store, ssid, now);
            }
        }
        secret.remember(store, ssid);
        Ok(())
    }

    /// Restrict the search to `band`, or open it to both. Takes effect
    /// on the next connect; a failure is logged, not fatal.
    #[allow(unsafe_code)]
    pub fn set_band(&mut self, band: Band) {
        let mode = band.mode();
        // SAFETY: a plain setter on the started driver; `mode` is one of
        // the enum's values.
        let rc = unsafe { sys::esp_wifi_set_band_mode(mode) };
        if rc != sys::ESP_OK {
            log::warn!("wifi: esp_wifi_set_band_mode({mode}) returned {rc}");
        }
    }

    /// The access point the station is joined to, as a hint for next time.
    pub fn joined_ap(&self) -> Option<ApHint> {
        self.wifi.wifi().get_ap_info().ok().map(|ap| ApHint {
            channel: ap.channel,
            bssid: ap.bssid,
        })
    }

    /// The station's address, when it has one.
    pub fn address(&self) -> Option<String> {
        if !self.wifi.is_up().unwrap_or(false) {
            return None;
        }
        let info = self.wifi.wifi().sta_netif().get_ip_info().ok()?;
        (!info.ip.is_unspecified()).then(|| info.ip.to_string())
    }

    /// The hostname the station told DHCP.
    pub fn hostname(&self) -> String {
        self.wifi
            .wifi()
            .sta_netif()
            .get_hostname()
            .map(|h| h.as_str().to_string())
            .unwrap_or_default()
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

/// What the driver is given in the password's place, and the order the
/// candidates are tried in: a remembered key, a freshly derived key
/// (the password may have changed), the passphrase (a WPA3 network).
enum Secret {
    /// An open network: nothing.
    Open,
    /// A key from the store, not yet proven.
    Remembered(String),
    /// A key derived this time; to be stored once the join succeeds.
    Derived(String),
    /// The passphrase itself, for SAE; to be noted once the join succeeds.
    Passphrase,
    /// The passphrase, remembered as what this network takes.
    PassphraseRemembered,
}

impl Secret {
    fn for_network<S: ConfigStore>(store: &S, ssid: &str, password: &str) -> Secret {
        if password.is_empty() {
            return Secret::Open;
        }
        let get = |k: &str| store.get(k).ok().flatten();
        if get(HINT_SSID).as_deref() != Some(ssid) {
            return Secret::Derived(psk(ssid, password));
        }
        if get(HINT_SAE).is_some() {
            return Secret::PassphraseRemembered;
        }
        match get(HINT_PSK) {
            Some(key) => Secret::Remembered(key),
            None => Secret::Derived(psk(ssid, password)),
        }
    }

    fn as_password<'a>(&'a self, password: &'a str) -> &'a str {
        match self {
            Secret::Open => "",
            Secret::Remembered(k) | Secret::Derived(k) => k,
            Secret::Passphrase | Secret::PassphraseRemembered => password,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Secret::Open => "no key",
            Secret::Remembered(_) => "remembered key",
            Secret::Derived(_) => "derived key",
            Secret::Passphrase | Secret::PassphraseRemembered => "passphrase",
        }
    }

    /// The next candidate after a refusal; `false` when there is none.
    fn next(&mut self, ssid: &str, password: &str) -> bool {
        *self = match self {
            Secret::Remembered(old) => {
                let fresh = psk(ssid, password);
                if fresh == *old {
                    Secret::Passphrase
                } else {
                    Secret::Derived(fresh)
                }
            }
            Secret::Derived(_) | Secret::PassphraseRemembered => Secret::Passphrase,
            Secret::Open | Secret::Passphrase => return false,
        };
        true
    }

    fn remember<S: ConfigStore>(&self, store: &mut S, ssid: &str) {
        let result = match self {
            Secret::Derived(key) => store
                .set(HINT_SSID, ssid)
                .and_then(|()| store.remove(HINT_SAE))
                .and_then(|()| store.set(HINT_PSK, key)),
            Secret::Passphrase => store
                .set(HINT_SSID, ssid)
                .and_then(|()| store.remove(HINT_PSK))
                .and_then(|()| store.set(HINT_SAE, "1")),
            _ => Ok(()),
        };
        if let Err(e) = result {
            log::warn!("wifi: could not remember what the network takes: {e:?}");
        }
    }
}

fn read_hint<S: ConfigStore>(store: &S, ssid: &str) -> Option<ApHint> {
    let get = |k: &str| store.get(k).ok().flatten();
    if get(HINT_SSID)?.as_str() != ssid {
        return None;
    }
    let channel = get(HINT_CHANNEL)?.parse().ok()?;
    let bssid_text = get(HINT_BSSID)?;
    let mut bssid = [0u8; 6];
    let mut parts = bssid_text.split(':');
    for b in &mut bssid {
        *b = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(ApHint { channel, bssid })
}

fn write_hint<S: ConfigStore>(store: &mut S, ssid: &str, hint: ApHint) {
    let b = hint.bssid;
    let bssid = format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5]
    );
    let result = store
        .set(HINT_SSID, ssid)
        .and_then(|()| store.set(HINT_CHANNEL, &hint.channel.to_string()))
        .and_then(|()| store.set(HINT_BSSID, &bssid));
    match result {
        Ok(()) => log::info!("wifi: remembered ch {} {bssid} for '{ssid}'", hint.channel),
        Err(e) => log::warn!("wifi: could not remember the access point: {e:?}"),
    }
}

impl NetworkStatus for WifiNet<'_> {
    fn host_info(&mut self) -> HostInfo {
        // The bare name: there is no mDNS responder in this build, so a
        // `.local` suffix would name something that does not answer.
        let hostname = self.hostname();
        let addrs = self
            .address()
            .map(|addr| Address {
                iface: "wlan0".into(),
                link: Link::Wireless,
                addr,
            })
            .into_iter()
            .collect();
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
