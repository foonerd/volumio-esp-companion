Volumio ESP Companion {{VERSION}} for the Waveshare ESP32-C5-Touch-LCD-2.8.

**Files.** `volumio-esp-companion-waveshare-esp32-c5-touch-lcd-28-{{VERSION}}.bin` is the whole image; flash it at address 0. The `-bootloader`, `-partition-table` and `-app` files are its three parts for anyone flashing them separately. `SHA256SUMS` covers all four. `volumio.txt` is the card template.

## Install

1. **esptool 5** on your computer. With Python: `pip install esptool` (Windows: `py -m pip install esptool`). Without Python: the standalone `esptool` binary for Windows, macOS or Linux from [esptool's releases](https://github.com/espressif/esptool/releases).
2. **Plug the board in** over its USB-C port. It appears as a serial port with no driver to install: Windows `COM3` or similar (Device Manager, Ports), macOS `/dev/cu.usbmodem…`, Linux `/dev/ttyACM0` (your user needs to be in the `dialout` group, or run with `sudo`).
3. **Flash**, with your port in place of `PORT` and the file you downloaded:

       esptool --chip esp32c5 --port PORT write-flash 0x0 volumio-esp-companion-waveshare-esp32-c5-touch-lcd-28-{{VERSION}}.bin

   esptool verifies the write by itself. If it cannot connect, hold the board's BOOT button while plugging it in, then run the command again.
4. **The card.** Copy `volumio.txt` to the root of a FAT32 microSD card (32 GB or less works as sold), fill in your Wi-Fi name and password and the player's IP address as `http://192.168.1.50:3000`, put the card in the slot. The full steps for Windows, macOS and Linux: [Card setup](https://github.com/foonerd/volumio-esp-companion/wiki/Card-setup).
5. **Power on.** Within about twenty seconds the first time, and about seven after that, the screen shows its network and address, then the player's face once the player answers. `status.txt` on the card says what the device read and whether it joined.

**Updating** is the same flash command with the new file. A full flash clears the device's stored settings; the card puts them back on the next boot, so leave it in.

Build, pins, boot order and the rest: [the wiki](https://github.com/foonerd/volumio-esp-companion/wiki).

## Changes
