# Firmware

One crate per board; each is the device, and what it builds is the image you flash.

A firmware crate:

- is named `<vendor>-<model>` as the vendor writes it, lower case, dots dropped: `waveshare-esp32-c5-touch-lcd-28`;
- is a workspace of its own, excluded from the host workspace at the root, because it carries its own target (`.cargo/config.toml`), its own toolchain channel (`rust-toolchain.toml`), its own `sdkconfig.defaults` and, later, its own partition table;
- owns the board: the pin map, the bring-up order, the IO expander, the backlight, the power, and the capabilities record it hands to the runtime;
- builds the board from the shared platform crate and the driver crates, then hands it to the runtime. Its `main.rs` is thin on purpose: wiring, no protocol and no pages;
- is built from its own directory: `cargo build --release`, then `scripts/image.sh firmware/<crate>` for the flashable image.

| Crate | Board | Standing |
| --- | --- | --- |
| `waveshare-esp32-c5-touch-lcd-28` | Waveshare ESP32-C5-Touch-LCD-2.8 | Boots on std ESP-IDF v5.5.6 and reports the chip, the PSRAM and the Wi-Fi. No panel yet. |
