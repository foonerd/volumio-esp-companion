//! What a board has, declared once by its firmware crate.
//!
//! The runtime reads this to decide what to offer: album art only where
//! there is memory to decode it, voice only where there is a microphone,
//! a battery gauge only where there is a battery. A firmware declares it
//! as a `const` and hands it to the runtime at start; nothing is probed.

/// Native pixel size of the panel, before any rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    /// Width in the controller's native frame.
    pub width: u16,
    /// Height in the controller's native frame.
    pub height: u16,
}

impl Resolution {
    /// A resolution, width by height, in the controller's native frame.
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// Which Wi-Fi bands the radio can join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiBands {
    /// 2.4 GHz only.
    Band2G4,
    /// 2.4 and 5 GHz.
    DualBand,
}

/// The board, as its firmware declares it.
///
/// Every field is a fact about the hardware, not a setting: a board either
/// has a microphone or it does not. Settings live behind
/// [`crate::ConfigStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Vendor and model, as the vendor writes them.
    pub board: &'static str,
    /// The panel's native size.
    pub resolution: Resolution,
    /// Bytes of external PSRAM, 0 where there is none.
    pub psram_bytes: u32,
    /// Bytes of flash the firmware may use.
    pub flash_bytes: u32,
    /// The bands the Wi-Fi radio can join.
    pub wifi: WifiBands,
    /// Whether a microphone is on the board.
    pub microphone: bool,
    /// Whether a speaker output (codec and amplifier) is on the board.
    pub speaker: bool,
    /// Whether an inertial measurement unit is on the board.
    pub imu: bool,
    /// Whether a real-time clock is on the board.
    pub rtc: bool,
    /// Whether a battery can be connected and its level read.
    pub battery: bool,
    /// Whether removable storage (a TF card slot) is on the board.
    pub removable_storage: bool,
}

impl Capabilities {
    /// Whether the board has the memory to decode album art at panel size.
    ///
    /// A 240x320 cover at 16 bits a pixel is 150 KB before the decoder's own
    /// working set; without PSRAM that does not fit beside Wi-Fi and a frame
    /// buffer in a few hundred kilobytes of SRAM.
    pub const fn can_decode_art(&self) -> bool {
        self.psram_bytes > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn board(psram_bytes: u32) -> Capabilities {
        Capabilities {
            board: "test",
            resolution: Resolution::new(240, 320),
            psram_bytes,
            flash_bytes: 16 << 20,
            wifi: WifiBands::Band2G4,
            microphone: false,
            speaker: false,
            imu: false,
            rtc: false,
            battery: false,
            removable_storage: false,
        }
    }

    #[test]
    fn art_needs_psram() {
        assert!(!board(0).can_decode_art());
        assert!(board(8 << 20).can_decode_art());
    }

    #[test]
    fn capabilities_are_const_constructible() {
        const C: Capabilities = board(0);
        assert_eq!(C.resolution, Resolution::new(240, 320));
    }
}
