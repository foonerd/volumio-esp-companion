//! Firmware for the Waveshare ESP32-C5-Touch-LCD-2.8.
//!
//! Boots std Rust on ESP-IDF and brings the board up to the application,
//! in order:
//! 1. The image boots and Rust std runs (log output, threads, heap).
//! 2. Chip identity and revision as the ROM/IDF report them.
//! 3. PSRAM detected and sized (board has 8 MB).
//! 4. The card, if one is in the slot: `volumio.txt` read and written
//!    into the settings store, so the card is how a user configures the
//!    device (`sdcard/volumio.txt` is the template).
//! 5. The settings read from NVS through the platform's store.
//! 6. Wi-Fi driver starts through the platform crate, dual-band mode
//!    accepted, scan returns APs; with `wifi_ssid` set, the network is
//!    joined and the address reported. `status.txt` is written to the
//!    card with the outcome.
//! 7. The board: the expander, the panel through it, the touch controller,
//!    the backlight ([`board`]).
//! 8. The application: `companion-runtime`'s `App` stepped on this loop
//!    with the panel as its `DrawTarget`, the touch controller as its
//!    finger, the platform's HTTP and network status, and album art
//!    fetched and decoded on `companion-art`'s thread into PSRAM. The
//!    status page until the player answers, then the face.

use std::time::{Duration, Instant};

use companion_art::ArtLoader;
use companion_hal::{ConfigStore, NetworkStatus, TouchSource};
use companion_platform_esp_idf::{Band, Card, Http, NvsStore, WifiNet};
use companion_runtime::settings::key;
use companion_runtime::{card, App, Settings, TICK};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::{Gpio23, Pins as GpioPins};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::task::thread::{MallocCap, ThreadSpawnConfiguration};
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::{info, warn};

/// The NVS namespace the settings live in.
const SETTINGS_NAMESPACE: &str = "companion";
/// How often the loop logs the heap.
const HEARTBEAT: Duration = Duration::from_secs(30);
/// The art thread's stack, in PSRAM. The 48 KB measured on a desktop
/// overflowed on the device inside the JPEG decoder's table setup (bench
/// run 5); external RAM makes the size a non-question.
const ART_THREAD_STACK: usize = 256 * 1024;
/// The art thread's task name, for the heartbeat's low-water lookup.
const ART_THREAD_NAME: &core::ffi::CStr = c"art";

fn main() -> anyhow::Result<()> {
    sys::link_patches();
    EspLogger::initialize_default();

    info!(
        "{} {} starting",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    let mut status = Status::new();
    report_chip(&mut status);
    report_memory();

    let Peripherals {
        modem,
        i2c0,
        spi2,
        pins,
        ..
    } = Peripherals::take()?;
    let GpioPins {
        gpio0,
        gpio1,
        gpio5,
        gpio6,
        gpio7,
        gpio8,
        gpio9,
        gpio10,
        gpio23,
        ..
    } = pins;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut store = NvsStore::open(nvs.clone(), SETTINGS_NAMESPACE)?;

    let spi = board::spi_bus(spi2, gpio6, gpio7, gpio8)?;
    let card = read_card(spi.clone(), gpio23, &mut store, &mut status);

    let settings = Settings::load(&store);
    let line = format!(
        "settings: player {}, rotation {}, theme {}, strip {}",
        settings.player,
        settings.rotation,
        settings.theme.as_str(),
        settings.strip().as_str()
    );
    info!("{line}");
    status.line(line);
    let ssid = store.get(key::WIFI_SSID).ok().flatten();
    let password = store
        .get(key::WIFI_PASSWORD)
        .ok()
        .flatten()
        .unwrap_or_default();
    let name = store.get(key::NAME).ok().flatten();
    let hostname = match name.as_deref() {
        Some(raw) => match companion_runtime::settings::hostname(raw) {
            Some(h) => Some(h),
            None => {
                let line = format!(
                    "{}: {raw:?} is not a hostname (letters, digits, hyphens, up to {}); keeping the default",
                    key::NAME,
                    companion_runtime::settings::MAX_HOSTNAME_LEN
                );
                warn!("{line}");
                status.line(line);
                None
            }
        },
        None => None,
    };

    let mut net = match WifiNet::start(modem, sys_loop, nvs, hostname) {
        Ok(mut net) => {
            info!("wifi: started");
            net.set_band(Band::Both);
            // The scan is a bring-up diagnostic; with a network to join it
            // only costs the boot twelve seconds.
            if ssid.is_none() {
                scan(net.wifi_mut());
            }
            match ssid.as_deref() {
                Some(ssid) => match net.join(&mut store, ssid, &password, Band::Both) {
                    Ok(()) => status.line(report_link(&mut net, ssid)),
                    Err(e) => {
                        let line = format!("wifi: could not join '{ssid}': {e}");
                        warn!("{line}");
                        status.line(line);
                    }
                },
                None => {
                    let line = format!(
                        "wifi: no {} set; put it in {} on a card",
                        key::WIFI_SSID,
                        card::FILE_NAME
                    );
                    info!("{line}");
                    status.line(line);
                }
            }
            Net::Wifi(net)
        }
        Err(e) => {
            // The panel is still worth lighting; the status page says offline.
            let line = format!("wifi: start failed: {e}; running without a network");
            warn!("{line}");
            status.line(line);
            report_memory();
            Net::Offline
        }
    };

    if let Some(card) = &card {
        match card.write(card::STATUS_NAME, &status.text()) {
            Ok(()) => info!("card: {} written", card::STATUS_NAME),
            Err(e) => warn!("card: could not write {}: {e}", card::STATUS_NAME),
        }
    }

    let mut board = board::bring_up(
        board::Pins {
            i2c0,
            sda: gpio0,
            scl: gpio1,
            dc: gpio9,
            cs: gpio10,
            touch_int: gpio5,
        },
        spi,
        settings.rotation,
    )?;
    let start = Instant::now();
    let mut app = App::new(&settings, board::CAPABILITIES, Duration::ZERO);
    let mut http = Http::new();
    let loader = spawn_art()?;

    // The first frame before the light, so the panel never shows garbage.
    app.step(start.elapsed(), &mut http, &mut net, &mut board.panel)
        .map_err(|e| anyhow::anyhow!("first frame: {e:?}"))?;
    board.backlight(100)?;
    info!("panel lit; the application is running");

    let mut last_beat = Instant::now();
    loop {
        let now = start.elapsed();
        // A report is waiting while INT is low; a finger that was down and
        // has gone quiet is read once more so the controller can say so.
        if board.touch_int.is_low() || app.finger_down() {
            match board.touch.read() {
                Ok(report) => app.touch(report, now, &mut http),
                Err(e) => log::debug!("touch: {e:?}"),
            }
        }
        if let Err(e) = app.step(now, &mut http, &mut net, &mut board.panel) {
            warn!("frame: {e:?}");
        }
        if let Some(req) = app.art_request(now) {
            loader.request(req);
        }
        if let Some(art) = loader.poll() {
            app.art_arrived(art, now);
        }
        if last_beat.elapsed() >= HEARTBEAT {
            last_beat = Instant::now();
            // SAFETY: plain getters with no preconditions.
            let free = unsafe { sys::esp_get_free_heap_size() };
            let min = unsafe { sys::esp_get_minimum_free_heap_size() };
            info!(
                "heartbeat: free heap {free} B, low-water {min} B, art stack low-water {}, player {}",
                art_stack_low_water()
                    .map(|b| format!("{b} B"))
                    .unwrap_or_else(|| "?".into()),
                if app.on_player() { "up" } else { "waiting" }
            );
        }
        std::thread::sleep(TICK);
    }
}

/// The art thread with its stack in PSRAM. The pthread config is per
/// calling thread and names the allocation caps; the stack size itself
/// comes from the builder.
fn spawn_art() -> anyhow::Result<ArtLoader> {
    ThreadSpawnConfiguration {
        name: Some(ART_THREAD_NAME),
        stack_alloc_caps: MallocCap::Spiram | MallocCap::Cap8bit,
        ..Default::default()
    }
    .set()?;
    let loader = ArtLoader::spawn_with_stack(Http::new(), ART_THREAD_STACK);
    ThreadSpawnConfiguration::default().set()?;
    Ok(loader?)
}

/// The least stack the art thread has had left, in bytes.
fn art_stack_low_water() -> Option<u32> {
    // SAFETY: a lookup by name and a getter on the handle it returns.
    let task = unsafe { sys::xTaskGetHandle(ART_THREAD_NAME.as_ptr()) };
    if task.is_null() {
        return None;
    }
    // SAFETY: the handle is a live task's; ESP-IDF's StackType_t is a byte.
    Some(unsafe { sys::uxTaskGetStackHighWaterMark(task) })
}

/// What the status page would show, through the same trait it reads.
fn report_link(net: &mut WifiNet, ssid: &str) -> String {
    let host = net.host_info();
    let mut ch: u8 = 0;
    let mut second: sys::wifi_second_chan_t = 0;
    // SAFETY: writes into caller-owned values.
    unsafe { sys::esp_wifi_get_channel(&mut ch, &mut second) };
    let band = if ch > 14 { "5 GHz" } else { "2.4 GHz" };
    let line = format!(
        "wifi: joined '{ssid}' on ch {ch} ({band}); hostname {}, address {}",
        host.hostname,
        host.addrs
            .iter()
            .map(|a| a.addr.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    info!("{line}");
    line
}

/// Mount the card, apply `volumio.txt` to the store, say what happened.
/// `None` when there is no card or no FAT volume on it.
fn read_card(
    spi: board::Spi,
    cs: Gpio23<'static>,
    store: &mut NvsStore,
    status: &mut Status,
) -> Option<Card> {
    let card = match Card::mount(spi, cs) {
        Ok(card) => card,
        Err(e) => {
            info!("card: none mounted ({e}); settings stay as stored");
            return None;
        }
    };
    info!("card: mounted, {} MB", card.capacity >> 20);
    status.line(format!("card: {} MB", card.capacity >> 20));
    match card.read(card::FILE_NAME) {
        Ok(Some(text)) => {
            let parsed = card::parse(&text);
            let keys: Vec<&str> = parsed.entries.iter().map(|e| e.key.as_str()).collect();
            let line = match card::apply(&parsed, store) {
                Ok(()) => format!("{}: applied {}", card::FILE_NAME, keys.join(", ")),
                Err(e) => format!("{}: store refused: {e}", card::FILE_NAME),
            };
            info!("card: {line}");
            status.line(line);
            for (n, k) in &parsed.unknown {
                let line = format!("{}: line {n}: unknown key {k:?}", card::FILE_NAME);
                warn!("card: {line}");
                status.line(line);
            }
            for n in &parsed.malformed {
                let line = format!("{}: line {n}: no '=' on this line", card::FILE_NAME);
                warn!("card: {line}");
                status.line(line);
            }
        }
        Ok(None) => {
            let line = format!("{}: not on the card", card::FILE_NAME);
            info!("card: {line}");
            status.line(line);
        }
        Err(e) => {
            let line = format!("{}: could not be read: {e}", card::FILE_NAME);
            warn!("card: {line}");
            status.line(line);
        }
    }
    Some(card)
}

/// The lines `status.txt` gets: what the device is, what it read, what
/// the network said. Never a password.
struct Status(Vec<String>);

impl Status {
    fn new() -> Self {
        Self(vec![format!(
            "Volumio ESP Companion {} {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        )])
    }

    fn line(&mut self, s: String) {
        self.0.push(s);
    }

    fn text(&self) -> String {
        let mut t = self.0.join("\r\n");
        t.push_str("\r\n");
        t
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

fn report_chip(status: &mut Status) {
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
    status.line(format!(
        "board: {}, chip {target} v{}.{}",
        board::CAPABILITIES.board,
        chip.revision / 100,
        chip.revision % 100
    ));
}

fn report_memory() {
    // SAFETY: plain getters with no preconditions.
    let (psram, internal, free, largest, spiram) = unsafe {
        (
            sys::esp_psram_get_size(),
            sys::heap_caps_get_total_size(sys::MALLOC_CAP_INTERNAL),
            sys::heap_caps_get_free_size(sys::MALLOC_CAP_INTERNAL),
            sys::heap_caps_get_largest_free_block(sys::MALLOC_CAP_INTERNAL),
            sys::heap_caps_get_free_size(sys::MALLOC_CAP_SPIRAM),
        )
    };
    info!(
        "memory: psram {psram} B; internal heap {internal} B total, {free} B free, largest block {largest} B; spiram free {spiram} B"
    );
}

/// The network the application sees: Wi-Fi, or nothing when the driver
/// would not start.
enum Net<'d> {
    Wifi(WifiNet<'d>),
    Offline,
}

impl NetworkStatus for Net<'_> {
    fn host_info(&mut self) -> companion_hal::HostInfo {
        match self {
            Net::Wifi(w) => w.host_info(),
            Net::Offline => companion_hal::HostInfo::default(),
        }
    }
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
