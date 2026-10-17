# Changelog

All notable changes to Volumio ESP Companion are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- `firmware/waveshare-esp32-c5-touch-lcd-28`: the first firmware, std Rust on ESP-IDF v5.5.6. On boot it reports the chip model and revision, the PSRAM and heap sizes, a dual-band Wi-Fi scan with each access point's band, channel, strength and auth, an optional connection when credentials were given at build time, and a heap heartbeat every five seconds. No panel, touch or audio yet.
- `core` and `hal`: the two shared crates laid down as enforced contracts (`no_std`, no `unsafe`, no platform crate), empty of code until the port.
- `scripts/check.sh`: format, clippy, tests and docs on the host workspace, the scripts, then every firmware crate built; refuses to pass without the ESP toolchain. `scripts/image.sh`: the merged image and its parts into `dist/` from the offsets ESP-IDF writes to `flasher_args.json`.
