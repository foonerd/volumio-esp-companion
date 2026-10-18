//! Album art on the host: fetched and decoded on a thread, as the Pi did.
//!
//! Carried over from rpi-waveshare28 `runtime/src/art.rs` at `61ecf6d`:
//! the worker and the raster decode. SVG station logos are not decoded
//! here; the face shows "Retrieving artwork" and retries on the gate's
//! backoff, which is the honest state until a vector path exists.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use companion_core::Art;
use companion_hal::HttpClient;
use companion_runtime::ArtRequest;
use embedded_graphics::pixelcolor::Rgb565;
use image::imageops::FilterType;

use crate::http::Http;

/// Covers take longer than the loop's tick; this is their own budget.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Handle to the art thread.
pub struct ArtLoader {
    tx: Sender<ArtRequest>,
    rx: Receiver<Option<Art>>,
}

impl ArtLoader {
    /// Start the thread.
    pub fn spawn() -> std::io::Result<Self> {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<ArtRequest>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<Option<Art>>();
        thread::Builder::new()
            .name("art".into())
            .spawn(move || worker(req_rx, res_tx))?;
        Ok(Self {
            tx: req_tx,
            rx: res_rx,
        })
    }

    /// Hand a request to the thread.
    pub fn request(&self, req: ArtRequest) {
        let _ = self.tx.send(req);
    }

    /// Collect a result if one is ready. Never blocks.
    pub fn poll(&self) -> Option<Option<Art>> {
        match self.rx.try_recv() {
            Ok(art) => Some(art),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

fn worker(rx: Receiver<ArtRequest>, tx: Sender<Option<Art>>) {
    let mut http = Http::new();
    while let Ok(mut req) = rx.recv() {
        // Skipping through a playlist should render where the user stopped.
        while let Ok(newer) = rx.try_recv() {
            req = newer;
        }
        let art = match http.get(&req.url, FETCH_TIMEOUT) {
            Ok(bytes) => decode(&bytes, req.size),
            Err(e) => {
                log::debug!("album art {}: {e}", req.url);
                None
            }
        };
        if tx.send(art).is_err() {
            return;
        }
    }
}

/// Turn fetched bytes into a cover scaled to fit `size`.
///
/// Dispatched on the magic bytes rather than Content-Type: art is served by
/// whatever server the stream points at, and the declared type is not
/// always what the bytes are.
pub fn decode(bytes: &[u8], size: u32) -> Option<Art> {
    let img = match image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(e) => {
            log::debug!("album art decode: {e}");
            return None;
        }
    };
    // Fit inside the box preserving aspect; Triangle because covers are
    // photographic and nearest aliases at this size.
    let img = img.resize(size, size, FilterType::Triangle).to_rgba8();
    let (w, h) = img.dimensions();
    // Composite over black: station logos are routinely transparent PNGs.
    let px = img.pixels().map(|p| blend_on_black(p.0)).collect();
    Some(Art { w, h, px })
}

fn blend_on_black(p: [u8; 4]) -> Rgb565 {
    let a = u32::from(p[3]);
    let ch = |v: u8| ((u32::from(v) * a) / 255) as u8;
    Rgb565::new(ch(p[0]) >> 3, ch(p[1]) >> 2, ch(p[2]) >> 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blends_alpha_over_black() {
        assert_eq!(
            blend_on_black([255, 255, 255, 255]),
            Rgb565::new(31, 63, 31)
        );
        assert_eq!(blend_on_black([255, 255, 255, 0]), Rgb565::new(0, 0, 0));
        assert_eq!(
            blend_on_black([255, 255, 255, 128]),
            Rgb565::new(16, 32, 16)
        );
    }

    #[test]
    fn decodes_a_png_to_fit() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let art = decode(&png, 10).unwrap();
        assert_eq!((art.w, art.h), (10, 5));
        assert_eq!(art.px.len(), 50);
        assert_eq!(art.px[0], Rgb565::new(31, 0, 0));
    }

    #[test]
    fn garbage_is_none() {
        assert!(decode(b"<html>404</html>", 10).is_none());
    }
}
