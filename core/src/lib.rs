//! The platform-free heart of Volumio ESP Companion: the pages, the drawing
//! surfaces, the player state model and the Volumio protocol's shapes.
//!
//! Carried over from the Raspberry Pi runtime of
//! [rpi-waveshare28](https://github.com/foonerd/rpi-waveshare28) at commit
//! `61ecf6d` (`waveshare28-panel` 1.6.0): `ui.rs`, `surface.rs`, `state.rs`
//! and the value types they lean on. Each module's header names its source.
//! The two code lines diverge from here; a common crate is extracted when
//! the ESP32 shape has settled.
//!
//! Rules this crate keeps, enforced by the compiler where it can be:
//!
//! - `no_std` with `alloc`: nothing here knows an operating system, a chip
//!   or a vendor.
//! - No `unsafe`.
//! - No platform crate in its dependencies. The outside world is reached
//!   only through the traits of `companion-hal`; this crate uses the HAL's
//!   value types (`HostInfo`, `TouchSample`) and nothing else from it.
//!
//! What is here and what is not:
//!
//! | Here | In the runtime |
//! | --- | --- |
//! | [`ui`]: layout, palette, the resting face, the status page, hit maps | the loop that calls them |
//! | [`surface`]: the transient surfaces and their hit maps | the hold timer |
//! | [`state`]: `PlayerState`, its parsing, `Command` and its query | the HTTP fetch and send |
//! | [`art::Art`]: a decoded cover | fetching, decoding, scaling |
//! | [`theme`]: the named choices a page is drawn with | the settings that pick them |
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod art;
pub mod state;
pub mod surface;
pub mod theme;
pub mod ui;

pub use art::Art;
pub use state::{Command, PlayerState, SystemStatus};
pub use surface::{SurfHit, Surface};
pub use theme::{BarGap, StatusText, Strip, Theme};
pub use ui::{Hotspot, Layout, Palette, TextPane, TouchEv};
