#!/usr/bin/env bash
# Assemble the flashable image for one firmware crate.
#
# Usage: scripts/image.sh firmware/<vendor>-<model>     (after cargo build --release there)
# Needs: python3 with esptool 5.x
#
# Offsets and flash settings come from ESP-IDF's flasher_args.json, so they
# track the crate's sdkconfig. The app image is built from the Rust ELF; the
# "libespidf.bin" that flasher_args.json lists is ESP-IDF's placeholder, not
# this firmware. Output goes to dist/ with a SHA256SUMS.

set -euo pipefail

usage() { echo "usage: $0 firmware/<vendor>-<model>" >&2; exit 2; }
[ $# -eq 1 ] || usage

root="$(cd "$(dirname "$0")/.." && pwd)"
crate="$root/${1%/}"
[ -f "$crate/Cargo.toml" ] || { echo "no Cargo.toml in $1" >&2; exit 1; }

read -r name version target < <(python3 -I - "$crate" <<'PY'
import sys, tomllib
crate = sys.argv[1]
with open(f"{crate}/Cargo.toml", "rb") as f:
    pkg = tomllib.load(f)["package"]
with open(f"{crate}/.cargo/config.toml", "rb") as f:
    target = tomllib.load(f)["build"]["target"]
print(pkg["name"], pkg["version"], target)
PY
)

elf="$crate/target/$target/release/$name"
[ -f "$elf" ] || { echo "missing $elf: run cargo build --release in $1 first" >&2; exit 1; }

mapfile -t found < <(find "$crate/target/$target/release/build" -path '*esp-idf-sys*' -name flasher_args.json)
[ "${#found[@]}" -eq 1 ] || { echo "expected one flasher_args.json under $1/target, found ${#found[@]}" >&2; exit 1; }
args="${found[0]}"
build="$(dirname "$args")"

read -r chip mode freq size boot_off boot_file pt_off pt_file app_off < <(python3 -I - "$args" <<'PY'
import json, sys
a = json.load(open(sys.argv[1]))
fs = a["flash_settings"]
print(a["extra_esptool_args"]["chip"], fs["flash_mode"], fs["flash_freq"], fs["flash_size"],
      a["bootloader"]["offset"], a["bootloader"]["file"],
      a["partition-table"]["offset"], a["partition-table"]["file"],
      a["app"]["offset"])
PY
)

out="$root/dist"
stem="volumio-esp-companion-$name-$version"
mkdir -p "$out"
cp "$build/$boot_file" "$out/$stem-bootloader.bin"
cp "$build/$pt_file" "$out/$stem-partition-table.bin"

python3 -m esptool --chip "$chip" elf2image \
    --flash-mode "$mode" --flash-freq "$freq" --flash-size "$size" \
    -o "$out/$stem-app.bin" "$elf"

python3 -m esptool --chip "$chip" merge-bin -o "$out/$stem.bin" \
    --flash-mode "$mode" --flash-freq "$freq" --flash-size "$size" \
    "$boot_off" "$out/$stem-bootloader.bin" \
    "$pt_off" "$out/$stem-partition-table.bin" \
    "$app_off" "$out/$stem-app.bin"

(cd "$out" && sha256sum "$stem"*.bin | tee "$stem.SHA256SUMS")
echo "flash: esptool --chip $chip --port <PORT> write-flash 0x0 dist/$stem.bin"
