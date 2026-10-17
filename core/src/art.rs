//! A decoded cover, ready to copy to the panel.
//!
//! Carried over from rpi-waveshare28 `runtime/src/art.rs` at `61ecf6d`:
//! the picture only. Fetching, decoding and scaling are the runtime's, on
//! whatever thread and with whatever decoder the device affords.

use alloc::vec::Vec;
use embedded_graphics::pixelcolor::Rgb565;

/// A decoded cover, scaled to fit and converted to the panel's colour
/// format.
///
/// Converting to Rgb565 at decode time rather than at draw time means the
/// drawing loop only copies pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Art {
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
    /// Row-major pixels, `w * h` of them.
    pub px: Vec<Rgb565>,
}

impl Art {
    /// A cover of one colour, for tests and placeholders.
    pub fn solid(w: u32, h: u32, colour: Rgb565) -> Self {
        Self {
            w,
            h,
            px: alloc::vec![colour; (w * h) as usize],
        }
    }
}
