//! The contract between the shared code and a device.
//!
//! Two things live here:
//!
//! - The traits `companion-core` and the runtime talk to: a touch source, a
//!   network transport, a configuration store. The display needs no trait of
//!   its own; embedded-graphics' `DrawTarget` is it.
//! - The capabilities record a firmware crate fills in for its board:
//!   resolution, PSRAM, microphone, speaker, IMU, Wi-Fi bands, battery, RTC,
//!   storage. The runtime reads it to decide what to offer, such as album art
//!   only where there is memory to decode it.
//!
//! Rules, the same as the core's: `no_std`, no `unsafe`, no platform crate.
//! Implementations of these traits live with the device that provides them,
//! under `firmware/<vendor>-<model>`, and in the shared platform crates. The
//! crate is empty until the first trait is needed by the port.
#![cfg_attr(not(test), no_std)]
