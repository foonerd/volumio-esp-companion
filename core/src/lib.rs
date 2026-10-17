//! The platform-free heart of Volumio ESP Companion: the pages, the drawing
//! surface, the player state model and the Volumio protocol types.
//!
//! Rules this crate keeps, enforced by the compiler where it can be:
//!
//! - `no_std`: nothing here knows an operating system, a chip or a vendor.
//! - No `unsafe`.
//! - No platform crate in its dependencies: no `esp-idf-*`, no `std::net`,
//!   no embedded-hal implementation. The outside world is reached only
//!   through the traits of `companion-hal`.
//!
//! What lands here, from the Raspberry Pi runtime of rpi-waveshare28: the
//! pages (`ui.rs`), the surface (`surface.rs`) and the state model
//! (`state.rs`). The crate is empty until the first of them is ported.
#![cfg_attr(not(test), no_std)]
