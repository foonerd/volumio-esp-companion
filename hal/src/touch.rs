//! A touch controller, as the application sees it.
//!
//! The controller chip is behind a driver crate under `drivers/`; the
//! device wires the interrupt line and calls [`TouchSource::read`] when it
//! fires. The application turns samples into press, move and release.

/// One validated single-finger sample in the panel's native frame.
///
/// Native means before rotation: `x` runs across the controller's short
/// side and `y` down its long side, whatever way up the panel is mounted.
/// The application maps the sample through the current layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchSample {
    /// X in the native frame.
    pub x: u16,
    /// Y in the native frame.
    pub y: u16,
    /// Contact pressure as the controller reports it; 0 where it has none.
    pub pressure: u8,
    /// Number of fingers the controller reports down.
    pub fingers: u8,
}

/// Something that yields touch samples on request.
///
/// `read` is called when the device's interrupt line says a report is
/// waiting, and may be polled otherwise. `Ok(None)` is a release or an
/// empty report; an `Err` is a bus or packet failure the device may log
/// and carry on from.
pub trait TouchSource {
    /// The driver's error.
    type Error: core::fmt::Debug;

    /// Read one report. `None` when no finger is down.
    fn read(&mut self) -> Result<Option<TouchSample>, Self::Error>;
}
