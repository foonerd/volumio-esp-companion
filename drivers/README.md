# Drivers

One crate per controller chip, written once and used by every board that carries the chip, whoever soldered it down.

A driver crate:

- is named after the chip, lower case, as the data sheet writes it: `cst3530`, `ch32v003-expander`, `es8311`;
- is `no_std`, has no `unsafe`, and is generic over the embedded-hal 1.0 traits (`I2c`, `SpiDevice`, `OutputPin`, `DelayNs`) so it builds on the host for its tests and on any chip;
- knows the protocol and nothing else: no pin numbers, no bus speeds the board decides, no vendor's name;
- carries the data sheet's timings as named constants with the data sheet's section beside them, and a note where a figure was measured rather than read;
- has its tests against embedded-hal-mock, and a `README.md` saying what the chip is, which boards carry it, and what was verified on which one.

SPI display controllers of the ST77xx and ILI9xxx families need no crate here: mipidsi already drives them and the firmware crate uses its builder.

| Crate | Chip | Verified against |
| --- | --- | --- |
| [`ch32v003-expander`](ch32v003-expander/) | the CH32V003 running Waveshare's IO-expander firmware: eight GPIO, a PWM output, an ADC input | Waveshare's register map and BSP sequences in the board repository; embedded-hal-mock on the host. Not yet on a board. |
| [`cst3530`](cst3530/) | the Hynitron CST3530 touch controller | Waveshare's `esp_lcd_touch_cst3530` component; embedded-hal-mock on the host. Not yet on a board. |

The boards that carry both are the three Waveshare 2.8" ESP32 touch panels. Each crate's README says what it does and what remains to be seen on the bench.
