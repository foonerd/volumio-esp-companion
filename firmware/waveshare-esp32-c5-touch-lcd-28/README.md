# Waveshare ESP32-C5-Touch-LCD-2.8

The firmware for the Waveshare ESP32-C5-Touch-LCD-2.8: ESP32-C5-WROOM-1 (RISC-V, 2.4 and 5 GHz Wi-Fi), 8 MB PSRAM, 32 MB flash, a 2.8" 240x320 ST7789 panel over SPI, a CST3530 touch controller on I2C, an ES8311 codec with microphone and speaker, a QMI8658 IMU, an SHTC3 humidity sensor, a PCF85063 RTC and a CH32V003 IO expander.

## What it does today

Boots on std Rust over ESP-IDF and proves the stack from the ROM up. It touches no board peripheral yet: no panel, no touch, no audio. On boot it logs, in order:

1. Chip: the target it was built for, the model the ROM reports (23, `esp32c5`), the revision, the cores and the feature flags
2. Memory: the PSRAM detected, the internal and SPIRAM heap totals
3. Wi-Fi: the driver started, the band mode set to AUTO (2.4 and 5 GHz) and read back, then a scan listing every access point with its band, channel, strength and auth. Any access point on a channel above 14 proves 5 GHz receive.
4. A connection, only if `WIFI_SSID` and `WIFI_PASS` were set at build time: channel, band and the DHCP address
5. A heartbeat every 5 s with the free heap and its low-water mark

## Stack

| Item | Version |
| --- | --- |
| esp-idf-sys / esp-idf-hal / esp-idf-svc | 0.38.1 / 0.47.0 / 0.53.0 |
| ESP-IDF | v5.5.6, fetched by embuild into `.embuild/` on the first build |
| Rust | nightly-2026-10-17 with `rust-src` (`build-std`), pinned by `rust-toolchain.toml` |
| Target | `riscv32imac-esp-espidf`, `MCU=esp32c5` |
| Linker | ldproxy 0.3.5 |
| Image tool | esptool 5 |

## Build

Prerequisites: rustup, `cargo install ldproxy`, python3 with `pip install esptool`. The first build downloads ESP-IDF and the RISC-V toolchain, which takes several minutes.

```sh
cd firmware/waveshare-esp32-c5-touch-lcd-28
cargo build --release
```

With the connection step, credentials compiled into the image:

```sh
WIFI_SSID=yournet WIFI_PASS=yourpass cargo build --release
```

## Image

From the repository root:

```sh
scripts/image.sh firmware/waveshare-esp32-c5-touch-lcd-28
```

Writes the merged image (flash at 0x0) and its parts (bootloader at 0x2000, partition table at 0x8000, app at 0x10000) into `dist/`, with a `SHA256SUMS`. The offsets and flash settings are read from ESP-IDF's `flasher_args.json`, not written by hand.

## Flash

```sh
esptool --chip esp32c5 --port <PORT> write-flash 0x0 dist/volumio-esp-companion-waveshare-esp32-c5-touch-lcd-28-<version>.bin
```

Then read the log at 115200 on the same port.

## Known gaps

- `esp_chip_info()` is not in esp-idf-sys 0.38.1's generated bindings. It is declared by hand in `src/main.rs`, with the struct layout checked against ESP-IDF v5.5.6 `esp_chip_info.h`.
- The image name embedded by ESP-IDF reads `libespidf` (ESP-IDF's placeholder project name). Cosmetic.
- Flash is declared as 4 MB on a 32 MB board on purpose: smaller than the part is safe, and a bring-up image has no reason to touch addresses above 16 MB. The partition plan proper (OTA slots, an art cache) comes with the pages.
