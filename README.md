# Volumio ESP Companion

Now playing, transport, volume and album art on an ESP32 touch panel, talking to a Volumio player over the network. Firmware written in Rust for the family of ESP32 display boards: the pages, the player state and the Volumio client are written once and know nothing of the board; a board contributes its pins, its bring-up and a record of what it has, and a controller chip has one driver however many vendors solder it down.

It grows out of [rpi-waveshare28](https://github.com/foonerd/rpi-waveshare28), the Raspberry Pi runtime for the Waveshare 2.8" SPI LCD. Peter of [PIXIS](https://github.com/PIXISREPO/VOLUMIO-4-TOUCH), who tested that runtime on the PIXIS CB-1, asked for the same on the Waveshare ESP32-C5-Touch-LCD-2.8 and sent the first board to start tinkering with ([the Pi thread](https://community.volumio.com/t/re-re-pi-3a-volumio-waveshare-2-8-spi-lcd-display-is-working-cst328-touch-still-wip-help-appreciated/77681), [the ESP32 thread](https://community.volumio.com/t/re-2-8inch-esp32-waveshare-panel/77872)). Thank you, Peter.

**Where the project stands.** Pre-alpha, and running on its first board: the Waveshare ESP32-C5-Touch-LCD-2.8 boots, takes its network and player from a microSD card, joins on 2.4 or 5 GHz, shows the player's face with its album art, and answers to touch. Not yet: sound, the settings page, a second board. The [wiki](https://github.com/foonerd/volumio-esp-companion/wiki) has the [connections](https://github.com/foonerd/volumio-esp-companion/wiki/Connections), the [boards](https://github.com/foonerd/volumio-esp-companion/wiki/Boards), the [build](https://github.com/foonerd/volumio-esp-companion/wiki/Building) and the [card](https://github.com/foonerd/volumio-esp-companion/wiki/Card-setup).

<p align="center"><img src="https://raw.githubusercontent.com/wiki/foonerd/volumio-esp-companion/images/connections/network.png" width="760" alt="The player and the panel on one network, no cable between them"></p>

The panel is a client of the player over the LAN, as the Volumio apps are: it polls the player's state over REST, sends transport and volume, fetches the art. Nothing is installed on the player. (Finding the player by mDNS is on the list; today the card names its address.)

## Install

No toolchain. [Install](https://github.com/foonerd/volumio-esp-companion/wiki/Install) on the wiki takes you from the [latest release](https://github.com/foonerd/volumio-esp-companion/releases/latest) to the player's face: two files to download, one `esptool` command from Windows, macOS or Linux, one text file on a microSD card. To build it yourself, see [Build](#build).

## Build

The host workspace (`core`, `hal`, `runtime`, `sim`, the drivers) builds on stable Rust, pinned by `rust-toolchain.toml`. Each firmware crate pins its own channel: ESP-IDF std targets need nightly with `rust-src`, plus `ldproxy` and python3 with esptool 5; the first build fetches ESP-IDF and the chip's toolchain.

```sh
scripts/check.sh                                            # format, clippy, tests, docs, no_std on bare metal, scripts, then every firmware built
(cd firmware/waveshare-esp32-c5-touch-lcd-28 && cargo build --release)
scripts/image.sh firmware/waveshare-esp32-c5-touch-lcd-28   # the merged image and its parts into dist/
```

The simulator runs the application against a player on the network, in a window (needs `libsdl2-dev`) or straight to a PNG (needs nothing):

```sh
cargo run -p companion-sim -- --player http://volumio.local:3000 --rotation 90 --theme night
cargo run -p companion-sim --no-default-features -- --player http://volumio.local:3000 --snapshot face.png --after 5
```

The mouse is the finger; Escape quits. `--help` lists the settings it takes, which are the same keys a device reads from its store.

`check.sh` refuses to pass on a machine without the ESP toolchain rather than report a pass that never built the product. Without the toolchain on the machine, the builder image of `scripts/builder/Dockerfile` has everything; the first build fetches ESP-IDF into the firmware crate's `.embuild/`, which the mounted checkout keeps. The checkout is mounted at its own path because ESP-IDF's build records absolute paths under `target/`, and a tree built under one path will not rebuild under another:

```sh
docker build -t companion-builder -f scripts/builder/Dockerfile .
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD:$PWD" -w "$PWD" companion-builder scripts/check.sh
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD:$PWD" -w "$PWD" companion-builder scripts/image.sh firmware/waveshare-esp32-c5-touch-lcd-28
```

Each firmware crate's README has its flash command.

## License

MIT. The pages, the surface and the state model descend from [rpi-waveshare28](https://github.com/foonerd/rpi-waveshare28) by the same author. The register maps and bring-up sequences of the CH32V003 expander and the CST3530 touch controller are transcribed from Waveshare's board repository, [ESP32-C5-Touch-LCD-2.8](https://github.com/waveshareteam/ESP32-C5-Touch-LCD-2.8), Apache-2.0, copyright Waveshare and Espressif Systems; the ES8311 codec's from Espressif's `esp_codec_dev` in [esp-adf](https://github.com/espressif/esp-adf), Apache-2.0, copyright Espressif Systems; each driver's source names the file it was read from.
