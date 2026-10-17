#!/usr/bin/env bash
# The project's check. Host workspace first (core, hal, the driver crates):
# format, clippy with warnings denied, tests, docs with warnings denied. Then
# the scripts. Then every firmware crate, formatted and built for its device.
#
# The firmware step needs the ESP toolchain (ldproxy on PATH; each crate's
# rust-toolchain.toml pulls its channel). Without it the check fails and says
# so: a pass that never built the product would be a lie.

set -euo pipefail
cd "$(dirname "$0")/.."

echo "== host workspace"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

echo "== scripts"
for f in scripts/*.sh; do bash -n "$f"; done
if command -v shellcheck >/dev/null; then
  shellcheck scripts/*.sh
else
  echo "shellcheck is not on PATH; scripts parsed only"
fi

echo "== firmware"
if ! command -v ldproxy >/dev/null; then
  echo "ldproxy is not on PATH: the firmware cannot be built on this machine." >&2
  echo "  cargo install ldproxy; each firmware crate's rust-toolchain.toml names its channel." >&2
  exit 1
fi
for crate in firmware/*/; do
  [ -f "$crate/Cargo.toml" ] || continue
  echo "-- $crate"
  (cd "$crate" && cargo fmt --check && cargo build --release)
done

echo "== ok"
