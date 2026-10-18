# Waveshare ESP32-C5-Touch-LCD-2.8

The firmware for the Waveshare ESP32-C5-Touch-LCD-2.8: ESP32-C5-WROOM-1 (RISC-V, 2.4 and 5 GHz Wi-Fi), 8 MB PSRAM, 32 MB flash, a 2.8" 240x320 ST7789 panel over SPI, a CST3530 touch controller on I2C, an ES8311 codec with microphone and speaker, a QMI8658 IMU, an SHTC3 humidity sensor, a PCF85063 RTC and a CH32V003 IO expander.

## What it does today

Boots on std Rust over ESP-IDF, brings the board up and runs the application on it. On boot it logs, in order:

1. Chip: the target it was built for, the model the ROM reports (23, `esp32c5`), the revision, the cores and the feature flags
2. Memory: the PSRAM detected, the internal heap's total, free and largest block, the SPIRAM free
3. The card, if one is in the slot: its size, then `volumio.txt` read from its root and written into the settings store, with every unknown key and malformed line named. No card, or no FAT volume on it, is one line and the boot goes on. See [Configuring by card](#configuring-by-card).
4. The settings, read from the NVS namespace `companion` through `NvsStore` (the runtime's keys; defaults when the namespace is empty)
5. Wi-Fi, through `platform/esp-idf`'s `WifiNet`: the driver started, the band mode set to AUTO (2.4 and 5 GHz) and read back, then a scan listing every access point with its band, channel, strength and auth. Any access point on a channel above 14 proves 5 GHz receive. With `wifi_ssid` in the store, the network is joined and the channel, band, hostname and address logged as the status page reads them through `NetworkStatus`. Then `status.txt` is written to the card with everything so far.
6. The board (`src/board.rs`), in the vendor BSP's order: the CH32V003 expander up with its outputs high; the panel reset pulsed through it (200 ms low, 200 ms settled); the ST7789 initialised through mipidsi over SPI2 at 40 MHz, RGB565 inverted, rotated as the settings say; the CST3530 reset through the expander (10 ms each side); the touch interrupt on GPIO5
7. The application: `companion-runtime`'s `App` draws its first frame, then the backlight comes on through the expander's PWM, so nothing shows before the first frame. From here the loop steps the application every 40 ms: the status page with the hostname and address until the player answers `/status` and `getState`, then the face; the touch controller is read while its interrupt is low and the gestures go to the pages; album art is fetched and decoded (JPEG, PNG) on `companion-art`'s thread into PSRAM and drawn when it arrives
8. A heartbeat every 30 s with the free heap, its low-water mark and whether the player is up

Audio is not touched.

### Pins and parts, as the board module names them

| Part | Where |
| --- | --- |
| I2C (expander, touch, codec, IMU, RTC) | SDA GPIO0, SCL GPIO1, 400 kHz, one bus shared through `embedded-hal-bus` |
| CH32V003 expander | I2C `0x24`: EXIO0 touch reset, EXIO1 panel reset, EXIO3 amplifier enable, PWM backlight |
| SPI2, shared by the panel and the TF card | SCK GPIO6, MOSI GPIO7, MISO GPIO8; one `SpiDriver` behind an `Rc`, each device with its own chip select and clock |
| ST7789 panel | SPI2 CS GPIO10, DC GPIO9, 40 MHz; no reset pin of its own |
| TF card | SPI2 CS GPIO23, SPI mode at 20 MHz (the C5 has no SD/MMC host); FAT through ESP-IDF's FatFs at `/sdcard` |
| CST3530 touch | I2C `0x58`, INT GPIO5 pulled up, reset on EXIO0 |

## Stack

| Item | Version |
| --- | --- |
| esp-idf-sys / esp-idf-hal / esp-idf-svc | 0.38.1 / 0.47.0 / 0.53.0 |
| ESP-IDF | v5.5.5, fetched by embuild into `.embuild/` on the first build. Not v5.5.6: that release added `wifi_task_stack_size` to `wifi_init_config_t`, which esp-idf-svc 0.53.0 does not set, so the Wi-Fi task is created with no stack and `esp_wifi_init` fails with `ESP_ERR_NO_MEM` (found on the first bench run). Move up when esp-idf-svc sets the field. |
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

The image carries no network or player of its own; those come from the card, below, and stay in NVS once read.

## Configuring by card

The device reads `volumio.txt` from the root of a microSD card on every boot and writes what it says into its settings store, then writes `status.txt` next to it with the outcome. The card can stay in or come out afterwards; the settings are in NVS either way.

1. A microSD card formatted FAT32. A card of 32 GB or less is FAT32 as sold. A larger card must be formatted FAT32 first (ESP-IDF's FatFs does not read exFAT); Windows' own formatter will not do that above 32 GB, so use a card of 32 GB or less, or a third-party formatter.
2. Copy [`sdcard/volumio.txt`](../../sdcard/volumio.txt) from this repository to the card's root and edit it in any text editor: at least `wifi_ssid`, `wifi_password` and `player` (the player's IP address, as `http://192.168.1.50:3000`; `.local` names do not resolve on the device). Every other settings key is accepted under its store name; the template lists them.
3. Eject the card, insert it, power the device.
4. After a boot, `status.txt` on the card says what the device is, which keys it applied, which lines it did not understand, and whether the network was joined and at which address. It never contains the password. Once it says the network was joined, the `wifi_password` line may be removed from `volumio.txt`; NVS keeps it.

The file is `key = value` per line, UTF-8, CRLF or LF, `#` comments on their own lines only (a `#` after a value is part of the value, since passwords contain them), a value in double quotes keeps its spaces, an empty value puts the setting back to its default. The simulator reads the same file with `--card PATH`, which is how the format is tested on a desktop.

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

- Not yet seen on a board. The bring-up order, the panel's inversion and colour order, the touch controller's protocol and the expander's bit sense are the vendor BSP's, read from its source; the first bench run confirms or corrects them, and the panel clock (40 MHz, half the BSP's) goes up once the frames are clean.
- The art thread's stack is `companion-art::DEVICE_THREAD_STACK` (48 KB), a figure measured on the host in release; the device's high-water mark is the next measurement, and the figure moves with it. A cover is decoded at its own size before scaling, so a large JPEG costs a few megabytes of PSRAM for a moment (`SPIRAM_USE_MALLOC` is on). SVG station logos are not decoded.
- The touch controller is polled while its interrupt line is low, from the 40 ms loop, rather than woken by the edge. Fine for a finger; the edge-driven version comes with the first bench measurements.
- `esp_chip_info()` is not in esp-idf-sys 0.38.1's generated bindings. It is declared by hand in `src/main.rs`, with the struct layout checked against ESP-IDF v5.5.6 `esp_chip_info.h`.
- The image name embedded by ESP-IDF reads `libespidf` (ESP-IDF's placeholder project name). Cosmetic.
- Flash is declared as 4 MB on a 32 MB board on purpose: smaller than the part is safe, and a bring-up image has no reason to touch addresses above 16 MB. The partition table is this crate's `partitions.csv` (nvs 24 KB, phy 4 KB, one 3 MB app), generated into the image by `scripts/image.sh` with ESP-IDF's own tool: esp-idf-sys does not pass a crate's table to the ESP-IDF build, and ESP-IDF's stock tables stop at a 1500 KB app, which this one outgrew (the first bench run found it, as a boot loop). `image.sh` now refuses an app that would not fit. The partition plan proper (OTA slots, an art cache) comes with the pages.
