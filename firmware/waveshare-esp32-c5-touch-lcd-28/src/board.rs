//! The board: pins, bring-up order, and the parts the application needs.
//!
//! The facts are the schematic's and the vendor BSP's (board repository
//! `examples/esp-idf/components/esp32_c5_touch_lcd_2_8`):
//!
//! | Part | Where |
//! | --- | --- |
//! | I2C (expander, touch, codec, IMU, RTC) | SDA GPIO0, SCL GPIO1, 400 kHz |
//! | CH32V003 expander | I2C `0x24`: EXIO0 touch reset, EXIO1 panel reset, EXIO3 amp enable, PWM backlight |
//! | ST7789 panel | SPI2: SCK GPIO6, MOSI GPIO7, MISO GPIO8 (the TF card's), CS GPIO10, DC GPIO9; RGB565, inverted |
//! | CST3530 touch | I2C `0x58`, INT GPIO5, reset on EXIO0 |
//!
//! The order is the BSP's: the expander up with its outputs high, the
//! panel reset pulsed 200 ms low and 200 ms settled, the panel
//! initialised, the touch reset pulsed 10 ms each side, the backlight
//! last so nothing shows before the first frame.

use std::cell::RefCell;

use ch32v003_expander::{Expander, Pin as Exio};
use companion_hal::{Capabilities, Resolution, WifiBands};
use cst3530::Cst3530;
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{
    Gpio0, Gpio1, Gpio10, Gpio5, Gpio6, Gpio7, Gpio8, Gpio9, Input, Output, PinDriver,
    Pins as GpioPins, Pull,
};
use esp_idf_svc::hal::i2c::{I2cConfig, I2cDriver, I2cError, I2C0};
use esp_idf_svc::hal::spi::{
    config::{Config as SpiConfig, DriverConfig},
    Dma, SpiDeviceDriver, SpiDriver, SPI2,
};
use esp_idf_svc::hal::units::FromValueType;
use esp_idf_svc::sys::EspError;
use mipidsi::interface::SpiInterface;
use mipidsi::models::ST7789;
use mipidsi::options::{ColorInversion, ColorOrder, Orientation, Rotation};
use mipidsi::{Builder, NoResetPin};

/// The board, as its firmware declares it to the runtime.
pub const CAPABILITIES: Capabilities = Capabilities {
    board: "Waveshare ESP32-C5-Touch-LCD-2.8",
    resolution: Resolution::new(240, 320),
    psram_bytes: 8 << 20,
    flash_bytes: 32 << 20,
    wifi: WifiBands::DualBand,
    microphone: true,
    speaker: true,
    imu: true,
    rtc: true,
    battery: true,
    removable_storage: true,
};

/// SPI clock to the panel. The BSP runs 80 MHz; half that until the
/// first frames on the bench say the wiring takes more.
const PANEL_HZ: u32 = 40_000_000;

/// The expander lines this board drives.
const TOUCH_RST: Exio = Exio::P0;
const PANEL_RST: Exio = Exio::P1;
const AMP_EN: Exio = Exio::P3;

/// The I2C bus, shared by the expander and the touch controller for the
/// life of the firmware.
type Bus = &'static RefCell<I2cDriver<'static>>;

/// The panel as a `DrawTarget`.
pub type Panel = mipidsi::Display<
    SpiInterface<'static, SpiDeviceDriver<'static, SpiDriver<'static>>, PinDriver<'static, Output>>,
    ST7789,
    NoResetPin,
>;

/// What bring-up hands back.
pub struct Board {
    /// The panel, oriented as the settings ask.
    pub panel: Panel,
    /// The touch controller on the shared bus.
    pub touch: Cst3530<RefCellDevice<'static, I2cDriver<'static>>>,
    /// The touch interrupt line, low while a report is waiting.
    pub touch_int: PinDriver<'static, Input>,
    /// The expander, for the backlight and the amplifier.
    pub expander: Expander<RefCellDevice<'static, I2cDriver<'static>>>,
}

/// The peripherals a bring-up consumes, moved out of the peripherals by
/// name so the compiler knows each is used once. The modem is taken
/// before this by the network.
pub struct Pins {
    /// `I2C0`.
    pub i2c0: I2C0<'static>,
    /// `SPI2`.
    pub spi2: SPI2<'static>,
    /// SDA.
    pub sda: Gpio0<'static>,
    /// SCL.
    pub scl: Gpio1<'static>,
    /// Panel SCK.
    pub sck: Gpio6<'static>,
    /// Panel MOSI.
    pub mosi: Gpio7<'static>,
    /// Bus MISO (the TF card's; the panel does not read).
    pub miso: Gpio8<'static>,
    /// Panel DC.
    pub dc: Gpio9<'static>,
    /// Panel CS.
    pub cs: Gpio10<'static>,
    /// Touch interrupt.
    pub touch_int: Gpio5<'static>,
}

impl Pins {
    /// The board's pins, out of the peripherals' parts after the modem
    /// has been moved out for the network.
    pub fn new(i2c0: I2C0<'static>, spi2: SPI2<'static>, pins: GpioPins) -> Self {
        Self {
            i2c0,
            spi2,
            sda: pins.gpio0,
            scl: pins.gpio1,
            sck: pins.gpio6,
            mosi: pins.gpio7,
            miso: pins.gpio8,
            dc: pins.gpio9,
            cs: pins.gpio10,
            touch_int: pins.gpio5,
        }
    }
}

/// Bring the board up in the BSP's order and return its parts.
pub fn bring_up(pins: Pins, rotation: u16) -> Result<Board, BringUpError> {
    // The bus, leaked: it lives as long as the firmware, and both the
    // expander and the touch controller borrow it.
    let bus: Bus = Box::leak(Box::new(RefCell::new(I2cDriver::new(
        pins.i2c0,
        pins.sda,
        pins.scl,
        &I2cConfig::new().baudrate(400.kHz().into()),
    )?)));

    let mut expander = Expander::new(RefCellDevice::new(bus));
    expander.init(TOUCH_RST.mask() | PANEL_RST.mask() | AMP_EN.mask())?;
    expander.write_outputs(TOUCH_RST.mask() | PANEL_RST.mask() | AMP_EN.mask())?;
    log::info!("expander: up, outputs high");

    expander.set(PANEL_RST, false)?;
    FreeRtos::delay_ms(200);
    expander.set(PANEL_RST, true)?;
    FreeRtos::delay_ms(200);

    let spi = SpiDriver::new(
        pins.spi2,
        pins.sck,
        pins.mosi,
        Some(pins.miso),
        &DriverConfig::new().dma(Dma::Auto(4096)),
    )?;
    let device = SpiDeviceDriver::new(
        spi,
        Some(pins.cs),
        &SpiConfig::new().baudrate(PANEL_HZ.Hz()).write_only(true),
    )?;
    let dc = PinDriver::output(pins.dc)?;
    let buffer: &'static mut [u8] = Box::leak(vec![0u8; 4096].into_boxed_slice());
    let di = SpiInterface::new(device, dc, buffer);
    let panel = Builder::new(ST7789, di)
        .display_size(240, 320)
        .color_order(ColorOrder::Rgb)
        .invert_colors(ColorInversion::Inverted)
        .orientation(Orientation::new().rotate(match rotation {
            90 => Rotation::Deg90,
            180 => Rotation::Deg180,
            270 => Rotation::Deg270,
            _ => Rotation::Deg0,
        }))
        .init(&mut FreeRtos)
        .map_err(|e| BringUpError::Panel(format!("{e:?}")))?;
    log::info!("panel: initialised at {} Hz, rotation {rotation}", PANEL_HZ);

    expander.set(TOUCH_RST, false)?;
    FreeRtos::delay_ms(10);
    expander.set(TOUCH_RST, true)?;
    FreeRtos::delay_ms(10);
    let touch = Cst3530::new(RefCellDevice::new(bus));
    let touch_int = PinDriver::input(pins.touch_int, Pull::Up)?;
    log::info!("touch: reset, interrupt on GPIO5");

    Ok(Board {
        panel,
        touch,
        touch_int,
        expander,
    })
}

impl Board {
    /// Light the backlight, 0 to 100.
    pub fn backlight(&mut self, percent: u8) -> Result<(), I2cError> {
        self.expander.set_pwm_percent(percent)
    }
}

/// Why the board did not come up.
#[derive(Debug)]
pub enum BringUpError {
    /// A driver refused.
    Esp(EspError),
    /// The expander or the touch controller did not answer on the bus.
    I2c(I2cError),
    /// The panel did not initialise.
    Panel(String),
}

impl std::fmt::Display for BringUpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Esp(e) => write!(f, "{e}"),
            Self::I2c(e) => write!(f, "i2c: {e}"),
            Self::Panel(e) => write!(f, "panel: {e}"),
        }
    }
}

impl std::error::Error for BringUpError {}

impl From<EspError> for BringUpError {
    fn from(e: EspError) -> Self {
        Self::Esp(e)
    }
}

impl From<I2cError> for BringUpError {
    fn from(e: I2cError) -> Self {
        Self::I2c(e)
    }
}
