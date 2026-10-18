//! What every ESP board shares, over esp-idf-svc.
//!
//! The device half of the HAL for ESP-IDF: [`Http`] for the player's REST
//! and album art, [`NvsStore`] for the settings, [`WifiNet`] for joining a
//! network and for the status page. A firmware crate builds these once
//! and hands them to `companion-runtime`'s `App`; the panel, the touch
//! controller and the expander are the board's own and come from
//! `drivers/`.
//!
//! No `unsafe` here; the ESP-IDF bindings that need it stay in the
//! firmware crate, where the board is named.

#![deny(unsafe_code)]

pub mod http;
pub mod net;
pub mod nvs;

pub use http::Http;
pub use net::WifiNet;
pub use nvs::{NvsError, NvsStore};
