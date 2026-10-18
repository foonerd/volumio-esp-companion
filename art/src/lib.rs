//! Album art for a device with std: fetched and decoded on a thread.
//!
//! Carried over from rpi-waveshare28 `runtime/src/art.rs` at `61ecf6d`,
//! the worker and the raster decode, generic over the HAL's HTTP client
//! so the desktop simulator and an ESP32 running std on ESP-IDF share
//! one implementation. The gate that decides *which* cover to ask for
//! and when to ask again is `companion-runtime`'s; this crate fulfils an
//! [`ArtRequest`] and hands back an [`Art`] or `None`.
//!
//! JPEG and PNG, which is what Volumio's `/albumart` and most station
//! logos are. SVG logos are not decoded; the face shows "Retrieving
//! artwork" and the gate retries on its backoff.
//!
//! Memory: a cover is decoded at its own size before it is scaled, so a
//! 1000x1000 JPEG costs about 3 MB for a moment. On an ESP32 that lands
//! in PSRAM when the SDK's `SPIRAM_USE_MALLOC` is on, which the firmware
//! sets; a board without PSRAM reports `can_decode_art()` false and the
//! runtime never asks.

use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use companion_core::Art;
use companion_hal::HttpClient;
use companion_runtime::ArtRequest;
use embedded_graphics::pixelcolor::Rgb565;
use image::imageops::FilterType;

/// Covers take longer than the loop's tick; this is their own budget.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// A starting stack for the decode thread on a device, where a thread's
/// stack is named rather than inherited.
///
/// Measured on x86_64 in a release build with the crate's own tests (a
/// small PNG through `image` 0.25 and the scaler): 32 KB survives. A real
/// cover through a real HTTP client needs more than a toy; on the host a
/// debug build with ureq and rustls on the same thread overflows 64 KB,
/// which is why [`ArtLoader::spawn`] leaves the host its default stack.
/// The device logs its high-water mark; this number moves when it has.
pub const DEVICE_THREAD_STACK: usize = 48 * 1024;

/// Handle to the art thread.
pub struct ArtLoader {
    tx: Sender<ArtRequest>,
    rx: Receiver<Option<Art>>,
}

impl ArtLoader {
    /// Start the thread with its own HTTP client and the platform's
    /// default stack. For a desktop.
    pub fn spawn<H>(http: H) -> std::io::Result<Self>
    where
        H: HttpClient + Send + 'static,
    {
        Self::start(http, thread::Builder::new().name("art".into()))
    }

    /// Start the thread with a stack of `bytes`. For a device, where the
    /// default is a few kilobytes and the decoder wants more.
    pub fn spawn_with_stack<H>(http: H, bytes: usize) -> std::io::Result<Self>
    where
        H: HttpClient + Send + 'static,
    {
        Self::start(
            http,
            thread::Builder::new().name("art".into()).stack_size(bytes),
        )
    }

    fn start<H>(http: H, builder: thread::Builder) -> std::io::Result<Self>
    where
        H: HttpClient + Send + 'static,
    {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<ArtRequest>();
        let (res_tx, res_rx) = std::sync::mpsc::channel::<Option<Art>>();
        builder.spawn(move || worker(http, req_rx, res_tx))?;
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

fn worker<H: HttpClient>(mut http: H, rx: Receiver<ArtRequest>, tx: Sender<Option<Art>>) {
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
    use companion_hal::HttpError;

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

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn decodes_a_png_to_fit() {
        let art = decode(&png(40, 20), 10).unwrap();
        assert_eq!((art.w, art.h), (10, 5));
        assert_eq!(art.px.len(), 50);
        assert_eq!(art.px[0], Rgb565::new(31, 0, 0));
    }

    #[test]
    fn garbage_is_none() {
        assert!(decode(b"<html>404</html>", 10).is_none());
    }

    /// Serves one canned body, or an error, to the worker thread.
    struct Canned(Result<Vec<u8>, HttpError>);

    impl HttpClient for Canned {
        fn get(&mut self, _url: &str, _timeout: Duration) -> Result<Vec<u8>, HttpError> {
            self.0.clone()
        }
    }

    fn wait(loader: &ArtLoader) -> Option<Art> {
        for _ in 0..200 {
            if let Some(a) = loader.poll() {
                return a;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("the art thread did not answer");
    }

    #[test]
    fn the_thread_fetches_and_decodes() {
        let loader = ArtLoader::spawn(Canned(Ok(png(8, 8)))).unwrap();
        loader.request(ArtRequest {
            url: "http://p/a.png".into(),
            size: 4,
        });
        let art = wait(&loader).unwrap();
        assert_eq!((art.w, art.h), (4, 4));
    }

    #[test]
    fn a_device_sized_stack_decodes_a_small_png_in_release() {
        // The measurement behind DEVICE_THREAD_STACK; debug builds use
        // more stack and are not what a device runs.
        if cfg!(debug_assertions) {
            return;
        }
        let loader =
            ArtLoader::spawn_with_stack(Canned(Ok(png(64, 64))), DEVICE_THREAD_STACK).unwrap();
        loader.request(ArtRequest {
            url: "http://p/a.png".into(),
            size: 32,
        });
        assert!(wait(&loader).is_some());
    }

    #[test]
    fn a_failed_fetch_is_none() {
        let loader = ArtLoader::spawn(Canned(Err(HttpError::Status(404)))).unwrap();
        loader.request(ArtRequest {
            url: "http://p/a.png".into(),
            size: 4,
        });
        assert!(wait(&loader).is_none());
    }
}
