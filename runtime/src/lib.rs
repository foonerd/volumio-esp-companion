//! The application of Volumio ESP Companion, generic over the HAL.
//!
//! Carried over from the Raspberry Pi runtime of
//! [rpi-waveshare28](https://github.com/foonerd/rpi-waveshare28) at commit
//! `61ecf6d` (`waveshare28-panel` 1.6.0): `main.rs`, `input.rs`, the
//! fetching half of `state.rs`, the gate of `art.rs`, the request-target
//! encoding of `http.rs` and the face keys of `config.rs`. Each module's
//! header names its source.
//!
//! What changed in the carry is the shape, not the behaviour. The Pi owned
//! threads, channels and the clock; a device here owns all of that and
//! steps the [`App`]. Nothing in this crate blocks on anything but the
//! HAL's HTTP client, and nothing in it knows a chip, an operating system
//! or a vendor.
//!
//! Rules this crate keeps, enforced by the compiler where it can be:
//!
//! - `no_std` with `alloc`, proved by the bare-metal build in `check.sh`.
//! - No `unsafe`.
//! - The outside world is reached only through `companion-hal`'s traits:
//!   [`HttpClient`](companion_hal::HttpClient) for the player,
//!   [`NetworkStatus`](companion_hal::NetworkStatus) for the status page,
//!   [`ConfigStore`](companion_hal::ConfigStore) for the settings, and
//!   embedded-graphics' `DrawTarget` for the panel.
//!
//! | Module | What it is |
//! | --- | --- |
//! | [`app`] | the loop as a state machine: status screen, face, surfaces, gestures |
//! | [`gesture`] | press, move and release from raw touch reports |
//! | [`volumio`] | the REST client: `/status`, `getState`, commands |
//! | [`art`] | which cover to fetch and when to ask again |
//! | [`settings`] | the store keys and their defaults |
//! | [`url`] | the art-path join and the request-target encoding |
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod app;
pub mod art;
pub mod gesture;
pub mod settings;
pub mod url;
pub mod volumio;

pub use app::{App, TICK};
pub use art::{ArtGate, ArtRequest};
pub use gesture::Gestures;
pub use settings::Settings;
pub use volumio::{Client, ClientError};
