//! Firmware for the Waveshare ESP32-C5-Touch-LCD-2.8.
//!
//! Today it boots std Rust on ESP-IDF and proves the stack from the ROM up,
//! in order:
//! 1. The image boots and Rust std runs (log output, threads, heap).
//! 2. Chip identity and revision as the ROM/IDF report them.
//! 3. PSRAM detected and sized (board has 8 MB).
//! 4. Wi-Fi driver starts, dual-band mode accepted, scan returns APs.
//!    Any AP on channel > 14 proves 5 GHz receive.
//! 5. Optional: if WIFI_SSID / WIFI_PASS were set at build time, connect
//!    and report the DHCP address.
//!
//! No board peripherals (display, touch, audio) are touched yet.

use std::time::Duration;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use log::{info, warn};

const SSID: Option<&str> = option_env!("WIFI_SSID");
const PASS: Option<&str> = option_env!("WIFI_PASS");

fn main() -> anyhow::Result<()> {
    sys::link_patches();
    EspLogger::initialize_default();

    info!(
        "{} {} starting",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    report_chip();
    report_memory();

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    // Station mode with an empty config is enough to scan.
    wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()))?;
    wifi.start()?;
    info!("wifi: started");

    set_dual_band();
    scan(&mut wifi);

    if let (Some(ssid), Some(pass)) = (SSID, PASS) {
        if let Err(e) = connect(&mut wifi, ssid, pass) {
            warn!("wifi: connect to '{ssid}' failed: {e:?}");
        }
    } else {
        info!("wifi: no WIFI_SSID/WIFI_PASS at build time, connect step skipped");
    }

    let mut n: u32 = 0;
    loop {
        n = n.wrapping_add(1);
        // SAFETY: plain getters with no preconditions.
        let free = unsafe { sys::esp_get_free_heap_size() };
        let min = unsafe { sys::esp_get_minimum_free_heap_size() };
        info!("heartbeat {n}: free heap {free} B, low-water {min} B");
        std::thread::sleep(Duration::from_secs(5));
    }
}

// esp_chip_info.h is not in esp-idf-sys 0.38.1's generated bindings.
// Declared by hand; layout verified against ESP-IDF v5.5.6
// components/esp_hw_support/include/esp_chip_info.h.
#[repr(C)]
#[derive(Default)]
struct EspChipInfo {
    model: i32,    // esp_chip_model_t (C enum, int-sized)
    features: u32, // CHIP_FEATURE_x bit mask
    revision: u16, // MXX: M = wafer major, XX = wafer minor
    cores: u8,
}

/// esp_chip_model_t value for the ESP32-C5 in ESP-IDF v5.5.6.
const CHIP_ESP32C5: i32 = 23;

extern "C" {
    fn esp_chip_info(out_info: *mut EspChipInfo);
}

fn report_chip() {
    let mut chip = EspChipInfo::default();
    // SAFETY: fills the caller-owned struct; layout matches the C header.
    unsafe { esp_chip_info(&mut chip) };
    let target = core::str::from_utf8(sys::CONFIG_IDF_TARGET)
        .unwrap_or("?")
        .trim_end_matches('\0');
    info!(
        "chip: built for {target}, runtime model {} ({}), revision v{}.{}, cores {}, features 0x{:x}",
        chip.model,
        if chip.model == CHIP_ESP32C5 { "esp32c5" } else { "UNEXPECTED" },
        chip.revision / 100,
        chip.revision % 100,
        chip.cores,
        chip.features
    );
}

fn report_memory() {
    // SAFETY: plain getters with no preconditions.
    let psram = unsafe { sys::esp_psram_get_size() };
    let internal = unsafe { sys::heap_caps_get_total_size(sys::MALLOC_CAP_INTERNAL) };
    let spiram = unsafe { sys::heap_caps_get_total_size(sys::MALLOC_CAP_SPIRAM) };
    info!("psram: detected {psram} B, heap internal {internal} B, heap spiram {spiram} B");
}

fn set_dual_band() {
    // SAFETY: called after esp_wifi_start; the argument is a valid enum value.
    let rc = unsafe { sys::esp_wifi_set_band_mode(sys::wifi_band_mode_t_WIFI_BAND_MODE_AUTO) };
    if rc == sys::ESP_OK {
        info!("wifi: band mode set to AUTO (2.4 + 5 GHz)");
    } else {
        warn!("wifi: esp_wifi_set_band_mode(AUTO) returned {rc}");
    }
    let mut mode: sys::wifi_band_mode_t = 0;
    // SAFETY: writes into the caller-owned value.
    let rc = unsafe { sys::esp_wifi_get_band_mode(&mut mode) };
    info!("wifi: band mode readback rc={rc} mode={mode} (3 = AUTO)");
}

fn scan(wifi: &mut BlockingWifi<EspWifi<'static>>) {
    match wifi.scan() {
        Ok(aps) => {
            let five = aps.iter().filter(|a| a.channel > 14).count();
            info!(
                "scan: {} APs, {} on 5 GHz, {} on 2.4 GHz",
                aps.len(),
                five,
                aps.len() - five
            );
            for ap in &aps {
                let band = if ap.channel > 14 { "5G " } else { "2G4" };
                info!(
                    "  {band} ch {:>3}  {:>4} dBm  {:?}  {}",
                    ap.channel, ap.signal_strength, ap.auth_method, ap.ssid
                );
            }
        }
        Err(e) => warn!("scan: failed: {e:?}"),
    }
}

fn connect(
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    ssid: &str,
    pass: &str,
) -> anyhow::Result<()> {
    let auth = if pass.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow::anyhow!("ssid too long"))?,
        password: pass
            .try_into()
            .map_err(|_| anyhow::anyhow!("password too long"))?,
        auth_method: auth,
        ..Default::default()
    }))?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    let ip = wifi.wifi().sta_netif().get_ip_info()?;
    let mut ch: u8 = 0;
    let mut second: sys::wifi_second_chan_t = 0;
    // SAFETY: writes into caller-owned values.
    unsafe { sys::esp_wifi_get_channel(&mut ch, &mut second) };
    let band = if ch > 14 { "5 GHz" } else { "2.4 GHz" };
    info!(
        "wifi: connected to '{ssid}' on ch {ch} ({band}), ip {:?}",
        ip.ip
    );
    Ok(())
}
