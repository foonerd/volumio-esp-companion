//! Showing a drawn frame.
//!
//! The display is embedded-graphics' `DrawTarget`. Some targets paint as
//! they are drawn (an SPI panel through mipidsi); others hold a frame
//! until told to show it (a simulator window, a double-buffered panel).
//! The application calls [`Present::present`] once per frame and does not
//! know which kind it has.

/// A frame boundary for targets that need one.
pub trait Present {
    /// Make everything drawn since the last call visible.
    fn present(&mut self);
}

/// A target that paints as it is drawn needs no frame boundary.
#[derive(Debug, Clone, Copy, Default)]
pub struct Immediate;

impl Present for Immediate {
    fn present(&mut self) {}
}
