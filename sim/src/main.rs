//! Volumio ESP Companion on a desktop: the application in a window, or
//! rendered to a PNG, talking to a real player.
//!
//! The device parts are the host's: `ureq` for HTTP, the host's interfaces
//! for the status page, the command line for the settings store,
//! `companion-art`'s thread for album art, SDL2 for the panel and the
//! finger. The application is the same `companion-runtime` the firmware
//! runs.
//!
//! ```text
//! companion-sim --player http://volumio.local:3000 --rotation 90 --theme night
//! companion-sim --snapshot face.png --after 3
//! ```

mod host;
mod http;
mod net;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use companion_art::ArtLoader;
use companion_hal::{Capabilities, Resolution, WifiBands};
use companion_runtime::settings::key;
use companion_runtime::{card, App, Settings, TICK};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::Size;
use embedded_graphics_simulator::{OutputSettingsBuilder, SimulatorDisplay};

use crate::host::ArgStore;
use crate::http::Http;
use crate::net::HostNet;

/// What the simulator declares itself to be: a board with room for art.
const CAPS: Capabilities = Capabilities {
    board: "desktop simulator",
    resolution: Resolution::new(240, 320),
    psram_bytes: 8 << 20,
    flash_bytes: 16 << 20,
    wifi: WifiBands::DualBand,
    microphone: false,
    speaker: false,
    imu: false,
    rtc: false,
    battery: false,
    removable_storage: false,
};

const USAGE: &str = "\
companion-sim: Volumio ESP Companion on a desktop

  --player URL          the player's origin (default http://volumio.local:3000)
  --rotation DEG        0, 90, 180 or 270 (default 0)
  --theme NAME          ink, dusk, studio, night
  --strip MODE          progress, stream, off
  --status-text SIZE    normal, large
  --card PATH           read settings from a volumio.txt as a device would
  --scale N             window pixels per panel pixel (default 2)
  --snapshot PATH       render to a PNG instead of a window
  --after SECS          seconds to run before the snapshot (default 3)
  --help";

struct Cli {
    store: ArgStore,
    scale: u32,
    snapshot: Option<PathBuf>,
    after: Duration,
}

fn parse_args() -> Result<Cli, String> {
    let mut cli = Cli {
        store: ArgStore::default(),
        scale: 2,
        snapshot: None,
        after: Duration::from_secs(3),
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            println!("{USAGE}");
            std::process::exit(0);
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} needs a value\n{USAGE}"))?;
        let keys: &[&str] = match flag.as_str() {
            "--player" => &[key::PLAYER],
            "--rotation" => &[key::ROTATION],
            "--theme" => &[key::THEME],
            "--strip" => &[key::STRIP_PORTRAIT, key::STRIP_LANDSCAPE],
            "--status-text" => &[key::STATUS_TEXT_PORTRAIT, key::STATUS_TEXT_LANDSCAPE],
            "--card" => {
                let text =
                    std::fs::read_to_string(&value).map_err(|e| format!("--card {value}: {e}"))?;
                let parsed = card::parse(&text);
                for (n, k) in &parsed.unknown {
                    eprintln!("{value}:{n}: unknown key {k:?}");
                }
                for n in &parsed.malformed {
                    eprintln!("{value}:{n}: no '=' on this line");
                }
                let Ok(()) = card::apply(&parsed, &mut cli.store);
                continue;
            }
            "--scale" => {
                cli.scale = value
                    .parse()
                    .ok()
                    .filter(|s| (1..=8).contains(s))
                    .ok_or_else(|| format!("--scale must be 1 to 8, got {value:?}"))?;
                continue;
            }
            "--snapshot" => {
                cli.snapshot = Some(PathBuf::from(value));
                continue;
            }
            "--after" => {
                cli.after =
                    Duration::from_secs(value.parse().map_err(|_| {
                        format!("--after must be a number of seconds, got {value:?}")
                    })?);
                continue;
            }
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        };
        for k in keys {
            cli.store.0.insert((*k).into(), value.clone());
        }
    }
    Ok(cli)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = match parse_args() {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let settings = Settings::load(&cli.store);
    let mut app = App::new(&settings, CAPS, Duration::ZERO);
    let frame = app.layout().frame.size;
    let mut display = SimulatorDisplay::<Rgb565>::new(Size::new(frame.width, frame.height));
    let loader = match ArtLoader::spawn(Http::new()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("starting the art thread: {e}");
            std::process::exit(1);
        }
    };
    let mut host = Host {
        http: Http::new(),
        net: HostNet,
        loader,
        start: Instant::now(),
    };
    let output = OutputSettingsBuilder::new().scale(cli.scale).build();

    if let Some(path) = cli.snapshot {
        while host.now() < cli.after {
            host.tick(&mut app, &mut display);
            std::thread::sleep(TICK);
        }
        if let Err(e) = display.to_rgb_output_image(&output).save_png(&path) {
            eprintln!("writing {}: {e}", path.display());
            std::process::exit(1);
        }
        log::info!("wrote {}", path.display());
        return;
    }

    #[cfg(feature = "window")]
    window::run(
        &mut app,
        &mut display,
        &mut host,
        &output,
        settings.rotation,
    );
    #[cfg(not(feature = "window"))]
    {
        eprintln!("built without the window feature: use --snapshot PATH");
        std::process::exit(2);
    }
}

/// The device parts the host supplies, and the clock they share.
struct Host {
    http: Http,
    net: HostNet,
    loader: ArtLoader,
    start: Instant,
}

impl Host {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    /// One pass: step the application, move art in and out.
    fn tick(&mut self, app: &mut App, display: &mut SimulatorDisplay<Rgb565>) {
        let now = self.now();
        let Ok(()) = app.step(now, &mut self.http, &mut self.net, display);
        if let Some(req) = app.art_request(now) {
            self.loader.request(req);
        }
        if let Some(art) = self.loader.poll() {
            app.art_arrived(art, now);
        }
    }
}

#[cfg(feature = "window")]
mod window {
    use super::*;
    use companion_runtime::App;
    use embedded_graphics_simulator::sdl2::{Keycode, MouseButton};
    use embedded_graphics_simulator::{OutputSettings, SimulatorEvent, Window};

    /// The window loop: the mouse is the finger, Escape or closing quits.
    pub fn run(
        app: &mut App,
        display: &mut SimulatorDisplay<Rgb565>,
        host: &mut Host,
        output: &OutputSettings,
        rotation: u16,
    ) {
        let mut window = Window::new("Volumio ESP Companion", output);
        let mut down = false;
        loop {
            host.tick(app, display);
            window.update(display);
            for event in window.events() {
                let now = host.now();
                match event {
                    SimulatorEvent::Quit
                    | SimulatorEvent::KeyDown {
                        keycode: Keycode::Escape,
                        ..
                    } => return,
                    SimulatorEvent::MouseButtonDown {
                        mouse_btn: MouseButton::Left,
                        point,
                    } => {
                        down = true;
                        app.touch(Some(host::unmap(rotation, point)), now, &mut host.http);
                    }
                    SimulatorEvent::MouseMove { point } if down => {
                        app.touch(Some(host::unmap(rotation, point)), now, &mut host.http);
                    }
                    SimulatorEvent::MouseButtonUp {
                        mouse_btn: MouseButton::Left,
                        ..
                    } => {
                        down = false;
                        app.touch(None, now, &mut host.http);
                    }
                    _ => {}
                }
            }
            std::thread::sleep(TICK);
        }
    }
}
