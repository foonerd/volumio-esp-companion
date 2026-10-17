# Drivers

One crate per controller chip, written once and used by every board that carries the chip, whoever soldered it down.

A driver crate:

- is named after the chip, lower case, as the data sheet writes it: `cst3530`, `ch32v003-expander`, `es8311`;
- is `no_std`, has no `unsafe`, and is generic over the embedded-hal 1.0 traits (`I2c`, `SpiDevice`, `OutputPin`, `DelayNs`) so it builds on the host for its tests and on any chip;
- knows the protocol and nothing else: no pin numbers, no bus speeds the board decides, no vendor's name;
- carries the data sheet's timings as named constants with the data sheet's section beside them, and a note where a figure was measured rather than read;
- has its tests against embedded-hal-mock, and a `README.md` saying what the chip is, which boards carry it, and what was verified on which one.

SPI display controllers of the ST77xx and ILI9xxx families need no crate here: mipidsi already drives them and the firmware crate uses its builder.

None exists yet. The first will be `cst3530`, the touch controller of the three Waveshare 2.8" boards.
