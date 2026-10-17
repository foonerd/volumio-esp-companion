//! The contract between the shared code and a device.
//!
//! Two things live here:
//!
//! - The traits `companion-core` and the runtime talk to. A device
//!   implements them; the shared code never sees the implementation.
//! - The capabilities record a firmware crate fills in for its board, so
//!   the runtime can decide what to offer.
//!
//! The display needs no trait of its own: embedded-graphics' `DrawTarget`
//! is it. The one addition is [`display::Present`], for targets that hold
//! a frame until told to show it.
//!
//! Rules this crate keeps: `no_std` with `alloc`, no `unsafe`, no platform
//! crate. Every trait is blocking and synchronous; a device that works
//! asynchronously adapts at its own edge. Implementations live with the
//! device that provides them, under `firmware/<vendor>-<model>`, in the
//! shared platform crates, and in `sim/` for the desktop.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod capabilities;
pub mod config;
pub mod discovery;
pub mod display;
pub mod http;
pub mod network;
pub mod touch;

pub use capabilities::{Capabilities, Resolution, WifiBands};
pub use config::ConfigStore;
pub use discovery::{Discovery, NoDiscovery, Player};
pub use display::{Immediate, Present};
pub use http::{HttpClient, HttpError};
pub use network::{Address, HostInfo, Hotspot, Link, NetworkStatus};
pub use touch::{TouchSample, TouchSource};
