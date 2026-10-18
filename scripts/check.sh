#!/usr/bin/env bash
# The project's check. Host workspace first (core, hal, runtime, the driver
# crates): format, clippy with warnings denied, tests, docs with warnings
# denied. Then the scripts. Then every firmware crate, formatted and built for
# its device.
#
# The firmware step needs the ESP toolchain (ldproxy on PATH; each crate's
# rust-toolchain.toml pulls its channel). Without it the check fails and says
# so: a pass that never built the product would be a lie.

set -euo pipefail
cd "$(dirname "$0")/.."

echo "== host workspace"
cargo fmt --all --check
# The simulator's window needs SDL2 to link. Where the library is absent the
# simulator is still built and tested in its headless shape, and that is
# said rather than skipped silently.
if pkg-config --exists sdl2 2>/dev/null; then
  sim_features=()
else
  echo "libsdl2-dev is not installed: the simulator is checked without its window"
  sim_features=(--exclude companion-sim)
fi
cargo clippy --workspace "${sim_features[@]}" --all-targets -- -D warnings
cargo test --workspace "${sim_features[@]}"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace "${sim_features[@]}" --no-deps
if [ "${#sim_features[@]}" -ne 0 ]; then
  cargo clippy -p companion-sim --no-default-features --all-targets -- -D warnings
  cargo test -p companion-sim --no-default-features
  RUSTDOCFLAGS="-D warnings" cargo doc -p companion-sim --no-default-features --no-deps
fi
# The headless simulator is run once against a closed port: it must reach
# the status page and write a PNG without a player or a window.
cargo run -q -p companion-sim --no-default-features -- --player http://127.0.0.1:9 --snapshot target/sim-status.png --after 1
test -s target/sim-status.png

echo "== no_std, on bare metal"
# core, hal, runtime and the drivers claim no_std; proving it means building
# them for a target that has no std at all. The RISC-V target the C5 and C6
# firmware use, minus ESP-IDF.
bare=riscv32imac-unknown-none-elf
if ! rustup target list --installed | grep -qx "$bare"; then
  echo "target $bare is not installed: rustup target add $bare" >&2
  exit 1
fi
cargo build -p companion-core -p companion-hal -p companion-runtime -p ch32v003-expander -p cst3530 -p es8311 --target "$bare"

echo "== scripts"
for f in scripts/*.sh; do bash -n "$f"; done
if command -v shellcheck >/dev/null; then
  shellcheck scripts/*.sh
else
  echo "shellcheck is not on PATH; scripts parsed only"
fi

echo "== firmware"
for tool in ldproxy cmake ninja; do
  if ! command -v "$tool" >/dev/null; then
    echo "$tool is not on PATH: the firmware cannot be built on this machine." >&2
    echo "  Use the builder image (scripts/builder/Dockerfile), or install the ESP-IDF prerequisites and ldproxy." >&2
    exit 1
  fi
done
for crate in firmware/*/; do
  [ -f "$crate/Cargo.toml" ] || continue
  echo "-- $crate"
  # The platform crate is formatted and linted through the firmware that
  # carries it; it has no build of its own.
  (cd "$crate" && cargo fmt --check && cargo fmt --check --manifest-path ../../platform/esp-idf/Cargo.toml \
    && cargo clippy --release -p companion-platform-esp-idf -- -D warnings \
    && cargo clippy --release -- -D warnings && cargo build --release)
done

echo "== ok"
