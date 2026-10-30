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
| [`es8311`](es8311/) | the Everest ES8311 codec at `0x18`, mono, its master clock made from BCLK; the DAC path | Waveshare's BSP sequence; embedded-hal-mock on the host; plays on the ESP32-C5-Touch-LCD-2.8. |
| [`es9018k2m`](es9018k2m/) | the ESS ES9018K2M DAC at `0x48` as LightSABRE-class Pi HATs carry it, its own oscillator on the HAT | the es9018k2m-plugin's sequence; embedded-hal-mock on the host. |
| [`pcm512x`](pcm512x/) | the TI PCM5121/PCM5122 DAC at `0x4C` or `0x4D` as IQaudIO, HiFiBerry and Waveshare Pi HATs carry it, no master clock: the PLL on BCK, the dividers auto-set | the Linux `pcm512x` driver's clock-consumer path and the data sheet (SLAS763C, Table 33); embedded-hal-mock on the host. First board: Waveshare's PCM5122 Audio Board (A). |

The boards that carry both are the three Waveshare 2.8" ESP32 touch panels. Each crate's README says what it does and what remains to be seen on the bench.
