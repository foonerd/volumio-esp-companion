//! The application: the Pi's main loop as a state machine a device steps.
//!
//! Carried over from rpi-waveshare28 `runtime/src/main.rs` at `61ecf6d`.
//! The Pi owned its loop: threads for touch and art, channels between
//! them, `Instant` for time. A device here owns all of that and calls in:
//!
//! 1. [`App::touch`] with each report its touch controller yields;
//! 2. [`App::step`] once per tick with its clock, its HTTP client, its
//!    network status and its display;
//! 3. [`App::art_request`] to learn which cover to fetch, and
//!    [`App::art_arrived`] with the result, inline or from another thread;
//! 4. then presents the frame and waits [`TICK`] or a touch interrupt,
//!    whichever is first.
//!
//! The rules the face lives by are the Pi's and do not move: the status
//! screen until `/status` says ready and `getState` succeeds once, then
//! the player and never the status screen again; a failed poll keeps what
//! is on screen; a scene change repaints in full, a seek advance repaints
//! the strip in place; a surface holds ten seconds past the last touch.

use alloc::string::String;
use core::time::Duration;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::Point;

use companion_core::surface::{self, SurfHit, Surface};
use companion_core::ui::{self, face_hit, face_text_slot, Hotspot, TextPane, TouchEv};
use companion_core::{Art, Command, Layout, PlayerState, Strip, SystemStatus};
use companion_hal::{Capabilities, HostInfo, HttpClient, NetworkStatus, TouchSample};

use crate::art::{ArtGate, ArtRequest};
use crate::gesture::Gestures;
use crate::settings::Settings;
use crate::volumio::Client;

/// How long the device should wait for a touch before stepping again.
///
/// Also the ticker step, so text scrolls at one pixel per tick. A press
/// wakes the loop immediately regardless.
pub const TICK: Duration = Duration::from_millis(40);
/// How often to ask `/status` while still on the address screen.
const STATUS_POLL: Duration = Duration::from_secs(1);
/// Fall through to `getState` if `/status` never becomes `ready`.
const STATUS_WAIT_CAP: Duration = Duration::from_secs(120);
/// Spinner step on the `starting` footer.
const SPIN_INTERVAL: Duration = Duration::from_millis(400);
const SPIN: [char; 4] = ['|', '/', '-', '\\'];
/// How often to ask the device for its addresses.
const NET_POLL: Duration = Duration::from_secs(1);
/// Time the player gets to answer a request.
const NET_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a surface stays up with no touch.
const SURFACE_HOLD: Duration = Duration::from_secs(10);
const HOLD_DELAY: Duration = Duration::from_millis(400);
const HOLD_REPEAT: Duration = Duration::from_millis(150);
const MUTE_HOLD: Duration = Duration::from_millis(600);
const TAP_SLOP: i32 = 6;

/// A finger that is down, and what it landed on.
#[derive(Debug, Clone, Copy)]
struct Press {
    at: Point,
    from: Duration,
    face: Option<Hotspot>,
    surf: Option<SurfHit>,
    moved: bool,
    acted: bool,
}

/// The whole of the application's state.
#[derive(Debug)]
pub struct App {
    caps: Capabilities,
    client: Client,
    layout: Layout,
    poll_interval: Duration,
    gestures: Gestures,
    gate: ArtGate,
    art: Option<Art>,
    shown: Option<PlayerState>,
    current: PlayerState,
    next_poll: Duration,
    player: bool,
    backend_ready: bool,
    wait_from: Duration,
    next_status: Duration,
    next_spin: Duration,
    spin: usize,
    last_footer: Option<String>,
    status_dirty: bool,
    host: HostInfo,
    next_net: Duration,
    open: Option<Surface>,
    surface_until: Option<Duration>,
    press: Option<Press>,
    scrub: Option<f32>,
    strip_view: Strip,
    last_step: Duration,
    pane: TextPane,
    hold_shown: Option<u8>,
}

impl App {
    /// An application for `caps`, shaped by `settings`, started at `now`.
    pub fn new(settings: &Settings, caps: Capabilities, now: Duration) -> Self {
        let layout = settings.layout();
        if caps.resolution.width != ui::NATIVE_W || caps.resolution.height != ui::NATIVE_H {
            log::warn!(
                "panel is {}x{}, the face is drawn for {}x{}",
                caps.resolution.width,
                caps.resolution.height,
                ui::NATIVE_W,
                ui::NATIVE_H
            );
        }
        log::info!(
            "{}: player {}, rotation {}, theme {}, art {}",
            caps.board,
            settings.player,
            settings.rotation,
            settings.theme.as_str(),
            if caps.can_decode_art() {
                "on"
            } else {
                "off (no psram)"
            }
        );
        Self {
            caps,
            client: Client::new(&settings.player, NET_TIMEOUT),
            layout,
            poll_interval: settings.poll_interval,
            gestures: Gestures::new(settings.touch_debounce),
            gate: ArtGate::new(),
            art: None,
            shown: None,
            current: PlayerState::default(),
            next_poll: now,
            player: false,
            backend_ready: false,
            wait_from: now,
            next_status: now,
            next_spin: now + SPIN_INTERVAL,
            spin: 0,
            last_footer: None,
            status_dirty: true,
            host: HostInfo::default(),
            next_net: now,
            open: None,
            surface_until: None,
            press: None,
            scrub: None,
            strip_view: layout.strip,
            last_step: now,
            pane: TextPane::default(),
            hold_shown: None,
        }
    }

    /// The layout the face is drawn with.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// True once the player has been reached and the face is up.
    pub fn on_player(&self) -> bool {
        self.player
    }

    /// The last state the player reported.
    pub fn state(&self) -> &PlayerState {
        &self.current
    }

    /// The surface open over the face, if any.
    pub fn open_surface(&self) -> Option<Surface> {
        self.open
    }

    /// True while a finger is on the panel.
    pub fn finger_down(&self) -> bool {
        self.gestures.is_down()
    }

    /// Feed one touch report. Commands a gesture issues go out through
    /// `http` at once.
    pub fn touch<H: HttpClient>(
        &mut self,
        report: Option<TouchSample>,
        now: Duration,
        http: &mut H,
    ) {
        let Some(ev) = self.gestures.feed(report, now, &self.layout) else {
            return;
        };
        if !self.player {
            log::debug!("touch before player ready, ignored: {ev:?}");
            return;
        }
        self.handle_touch(ev, now, http);
    }

    /// The cover the device should fetch now, if any.
    ///
    /// Nothing on a board without the memory to decode one, nothing while
    /// a fetch is in flight or a failed one is in backoff.
    pub fn art_request(&mut self, now: Duration) -> Option<ArtRequest> {
        if !self.player || !self.caps.can_decode_art() {
            return None;
        }
        let path = self.current.album_art.as_deref()?;
        let size = self.layout.art.size.width.min(self.layout.art.size.height);
        self.gate.request(self.client.base(), path, size, now)
    }

    /// The outcome of the last [`ArtRequest`]. `None` means the fetch or
    /// decode failed; whatever was showing is cleared rather than left under
    /// the new track's title, and the same cover is asked again after a
    /// backoff.
    pub fn art_arrived(&mut self, art: Option<Art>, now: Duration) {
        self.gate.completed(art.is_some(), now);
        if art.is_none() && self.gate.failures() == 1 {
            if let Some(path) = self.gate.pending() {
                log::warn!("album art {path:?} failed; will retry");
            }
        }
        self.art = art;
        self.shown = None;
    }

    /// One pass of the loop: poll what is due, repaint what changed.
    ///
    /// Draws onto `target` without presenting; the device presents after.
    pub fn step<H, N, D>(
        &mut self,
        now: Duration,
        http: &mut H,
        net: &mut N,
        target: &mut D,
    ) -> Result<(), D::Error>
    where
        H: HttpClient,
        N: NetworkStatus,
        D: DrawTarget<Color = Rgb565>,
    {
        if now >= self.next_net {
            let host = net.host_info();
            self.next_net = now + NET_POLL;
            if host != self.host {
                self.host = host;
                self.status_dirty = true;
                if self.open == Some(Surface::Status) {
                    self.shown = None;
                }
            }
        }

        if !self.player {
            self.step_status(now, http, target)?;
        }

        if now >= self.next_poll {
            if self.backend_ready {
                match self.client.state(http) {
                    Ok(s) => {
                        if !self.player {
                            log::info!("player available, switching from status screen");
                            self.player = true;
                            self.shown = None;
                        }
                        self.current = s;
                    }
                    Err(e) => log::warn!("state poll failed: {e}"),
                }
            }
            self.next_poll = now + self.poll_interval;
        }

        if !self.player {
            return Ok(());
        }

        if self.surface_until.is_some_and(|until| now >= until) {
            self.close_surface();
        }
        let hold_left = self
            .surface_until
            .map(|until| surface::hold_secs(until.saturating_sub(now)));

        if self.layout.strip != self.strip_view {
            self.layout.strip = self.strip_view;
        }

        self.pane.set(
            self.current.title.as_deref().unwrap_or(""),
            self.current.artist.as_deref().unwrap_or(""),
            self.current.album.as_deref().unwrap_or(""),
            face_text_slot(&self.layout),
        );

        match self.shown.as_ref() {
            Some(prev) if prev.same_scene(&self.current) && self.scrub.is_none() => {
                if prev.seek != self.current.seek {
                    match self.open {
                        None if self.strip_view == Strip::Progress => {
                            ui::draw_progress(target, &self.layout, &self.current, None)?;
                        }
                        Some(Surface::Controls) => {
                            surface::draw_controls_seek(target, &self.layout, &self.current, None)?;
                        }
                        _ => {}
                    }
                }
                if self.open.is_some() && hold_left != self.hold_shown {
                    if let Some(left) = hold_left {
                        surface::draw_hold(target, &self.layout, left)?;
                    }
                    self.hold_shown = hold_left;
                }
                self.shown = Some(self.current.clone());
            }
            _ => {
                if let Some(kind) = self.open {
                    surface::draw(
                        target,
                        &self.layout,
                        kind,
                        &self.current,
                        self.art.as_ref(),
                        &self.host,
                        self.scrub,
                    )?;
                    surface::draw_hold(
                        target,
                        &self.layout,
                        hold_left.unwrap_or(surface::HOLD_SECS),
                    )?;
                    self.hold_shown = hold_left;
                } else {
                    let art_wait = self.current.album_art.is_some() && self.gate.retrieving();
                    ui::draw(
                        target,
                        &self.layout,
                        &self.current,
                        self.art.as_ref(),
                        &self.pane,
                        self.scrub,
                        art_wait,
                    )?;
                    self.hold_shown = None;
                }
                self.shown = Some(self.current.clone());
            }
        }

        if self.open.is_none() && self.pane.step() {
            ui::draw_rows(target, &self.layout, &self.pane)?;
        }

        if self.tick_hold(now, http) {
            self.next_poll = now;
            self.shown = None;
        }

        Ok(())
    }

    /// The address screen: `/status` on a cadence, the spinner, the repaint.
    fn step_status<H, D>(
        &mut self,
        now: Duration,
        http: &mut H,
        target: &mut D,
    ) -> Result<(), D::Error>
    where
        H: HttpClient,
        D: DrawTarget<Color = Rgb565>,
    {
        if !self.backend_ready && now >= self.next_status {
            if self.client.status(http) == Some(SystemStatus::Ready) {
                log::info!("backend ready, waiting for player state");
                self.backend_ready = true;
                self.next_poll = now;
            }
            self.next_status = now + STATUS_POLL;
            if !self.backend_ready && now.saturating_sub(self.wait_from) >= STATUS_WAIT_CAP {
                log::warn!(
                    "backend still not ready after {} s, falling through to getState",
                    STATUS_WAIT_CAP.as_secs()
                );
                self.backend_ready = true;
                self.next_poll = now;
            }
        }

        let want_spin = self.host.has_address() && !self.backend_ready;
        let spin_due = want_spin && now >= self.next_spin;
        if spin_due {
            self.spin = (self.spin + 1) % SPIN.len();
            self.next_spin = now + SPIN_INTERVAL;
        }
        let footer = want_spin.then(|| alloc::format!("starting {}", SPIN[self.spin]));
        if self.status_dirty || spin_due || footer != self.last_footer {
            ui::draw_status(target, &self.layout, &self.host, footer.as_deref())?;
            self.last_footer = footer;
            self.status_dirty = false;
        }
        Ok(())
    }

    fn close_surface(&mut self) {
        self.open = None;
        self.surface_until = None;
        self.scrub = None;
        self.shown = None;
        self.hold_shown = None;
    }

    fn open_surface_at(&mut self, kind: Surface, now: Duration) {
        self.open = Some(kind);
        self.surface_until = Some(now + SURFACE_HOLD);
        self.shown = None;
    }

    /// Hold-to-repeat on the volume steps and hold-to-mute on the speaker.
    /// True when a command went out and the state should be re-read.
    fn tick_hold<H: HttpClient>(&mut self, now: Duration, http: &mut H) -> bool {
        let Some(press) = self.press.as_mut() else {
            return false;
        };
        match press.surf {
            Some(SurfHit::VolUp) | Some(SurfHit::VolDown) => {
                if now.saturating_sub(press.from) < HOLD_DELAY {
                    return false;
                }
                if now.saturating_sub(self.last_step) < HOLD_REPEAT {
                    return false;
                }
                self.last_step = now;
                press.acted = true;
                let delta = if press.surf == Some(SurfHit::VolUp) {
                    2i16
                } else {
                    -2
                };
                let next =
                    (i16::from(self.current.volume.unwrap_or(0)) + delta).clamp(0, 100) as u8;
                send_cmd(&self.client, http, Command::Volume(next));
                true
            }
            Some(SurfHit::Speaker)
                if now.saturating_sub(press.from) >= MUTE_HOLD && !press.moved && !press.acted =>
            {
                press.acted = true;
                let cmd = if self.current.is_muted() {
                    Command::Unmute
                } else {
                    Command::Mute
                };
                send_cmd(&self.client, http, cmd);
                true
            }
            _ => false,
        }
    }

    fn handle_touch<H: HttpClient>(&mut self, ev: TouchEv, now: Duration, http: &mut H) {
        match ev {
            TouchEv::Down(p) => {
                self.press = Some(if let Some(kind) = self.open {
                    self.surface_until = Some(now + SURFACE_HOLD);
                    Press {
                        at: p,
                        from: now,
                        face: None,
                        surf: Some(surface::hit(&self.layout, kind, p)),
                        moved: false,
                        acted: false,
                    }
                } else {
                    Press {
                        at: p,
                        from: now,
                        face: face_hit(&self.layout, p),
                        surf: None,
                        moved: false,
                        acted: false,
                    }
                });
                self.last_step = now;
            }
            TouchEv::Move(p) => {
                let Some(pr) = self.press.as_mut() else {
                    return;
                };
                if (p.x - pr.at.x).abs() > TAP_SLOP || (p.y - pr.at.y).abs() > TAP_SLOP {
                    pr.moved = true;
                }
                let seeking = pr.face == Some(Hotspot::Seek) || pr.surf == Some(SurfHit::Seek);
                let moved = pr.moved;
                let on_slider = pr.surf == Some(SurfHit::VolSlider);
                if seeking && moved {
                    self.scrub = Some(ui::seek_fraction(self.layout.progress, p));
                    self.shown = None;
                }
                if on_slider {
                    send_cmd(
                        &self.client,
                        http,
                        Command::Volume(surface::volume_at(&self.layout, p)),
                    );
                    self.next_poll = now;
                    self.shown = None;
                }
            }
            TouchEv::Up(p) => {
                let Some(pr) = self.press.take() else {
                    return;
                };
                if self.open.is_some() {
                    self.surface_up(pr, p, now, http);
                    return;
                }
                if pr.moved && pr.face == Some(Hotspot::Seek) {
                    self.seek_to(p, now, http);
                    return;
                }
                if pr.moved {
                    return;
                }
                match pr.face {
                    Some(Hotspot::Art) => {
                        let kind = if self.caps.can_decode_art() && self.current.album_art.is_some()
                        {
                            Surface::Artwork
                        } else {
                            Surface::Metadata
                        };
                        self.open_surface_at(kind, now);
                    }
                    Some(Hotspot::Info) => {
                        self.next_net = now;
                        self.open_surface_at(Surface::Status, now);
                    }
                    Some(Hotspot::Title) | Some(Hotspot::DockMeta) => {
                        self.open_surface_at(Surface::Metadata, now);
                    }
                    Some(Hotspot::DockControls) => {
                        self.open_surface_at(Surface::Controls, now);
                    }
                    Some(Hotspot::DockVolume) => {
                        self.open_surface_at(Surface::Volume, now);
                    }
                    Some(Hotspot::Seek) => {
                        self.strip_view = match self.strip_view {
                            Strip::Progress => Strip::Stream,
                            Strip::Stream => Strip::Progress,
                            Strip::Off => Strip::Off,
                        };
                        self.shown = None;
                    }
                    None => {}
                }
            }
        }
    }

    /// A lift on an open surface.
    fn surface_up<H: HttpClient>(&mut self, pr: Press, p: Point, now: Duration, http: &mut H) {
        self.surface_until = Some(now + SURFACE_HOLD);
        let tap = !pr.moved;
        match pr.surf {
            Some(SurfHit::Close) => self.close_surface(),
            Some(SurfHit::Prev) if tap => self.command(Command::Prev, now, http),
            Some(SurfHit::PlayPause) if tap => self.command(Command::Toggle, now, http),
            Some(SurfHit::Next) if tap => self.command(Command::Next, now, http),
            Some(SurfHit::Seek) => self.seek_to(p, now, http),
            Some(SurfHit::Shuffle) if tap => self.command(Command::Random, now, http),
            Some(SurfHit::Repeat) if tap => self.command(Command::Repeat, now, http),
            Some(SurfHit::VolUp) if tap && !pr.acted => {
                let next = self.current.volume.unwrap_or(0).saturating_add(2).min(100);
                self.command(Command::Volume(next), now, http);
            }
            Some(SurfHit::VolDown) if tap && !pr.acted => {
                let next = self.current.volume.unwrap_or(0).saturating_sub(2);
                self.command(Command::Volume(next), now, http);
            }
            Some(SurfHit::VolSlider) => {
                self.command(
                    Command::Volume(surface::volume_at(&self.layout, p)),
                    now,
                    http,
                );
                self.shown = None;
            }
            _ => {}
        }
    }

    /// Send the seek the scrub or the lift point asks for, if the track
    /// has a length to seek within.
    fn seek_to<H: HttpClient>(&mut self, p: Point, now: Duration, http: &mut H) {
        let duration = self.current.duration.unwrap_or(0);
        if duration > 0 {
            let frac = self
                .scrub
                .unwrap_or_else(|| ui::seek_fraction(self.layout.progress, p));
            let secs = (frac * duration as f32) as u32;
            self.command(Command::Seek(secs), now, http);
        }
        self.scrub = None;
        self.shown = None;
    }

    fn command<H: HttpClient>(&mut self, cmd: Command, now: Duration, http: &mut H) {
        send_cmd(&self.client, http, cmd);
        self.next_poll = now;
    }
}

fn send_cmd<H: HttpClient>(client: &Client, http: &mut H, cmd: Command) {
    if let Err(e) = client.send(http, cmd) {
        log::warn!("command {cmd:?} failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use companion_hal::{HttpError, Resolution, WifiBands};
    use embedded_graphics::prelude::{OriginDimensions, Size};
    use embedded_graphics::Pixel;

    const CAPS: Capabilities = Capabilities {
        board: "test",
        resolution: Resolution::new(240, 320),
        psram_bytes: 8 << 20,
        flash_bytes: 16 << 20,
        wifi: WifiBands::Band2G4,
        microphone: false,
        speaker: false,
        imu: false,
        rtc: false,
        battery: false,
        removable_storage: false,
    };

    /// A player that answers from a script and records what it was asked.
    struct FakePlayer {
        status: &'static str,
        state: &'static str,
        asked: Vec<String>,
    }

    impl HttpClient for FakePlayer {
        fn get(&mut self, url: &str, _timeout: Duration) -> Result<Vec<u8>, HttpError> {
            self.asked.push(String::from(url));
            if url.ends_with("/status") {
                Ok(self.status.as_bytes().to_vec())
            } else if url.ends_with("/getState") {
                Ok(self.state.as_bytes().to_vec())
            } else if url.contains("/commands/") {
                Ok(Vec::new())
            } else {
                Err(HttpError::Status(404))
            }
        }
    }

    struct NoNet;

    impl NetworkStatus for NoNet {
        fn host_info(&mut self) -> HostInfo {
            HostInfo::default()
        }
    }

    /// A 240x320 target that counts frames cleared and pixels drawn.
    struct Counter {
        clears: u32,
        pixels: u64,
    }

    impl OriginDimensions for Counter {
        fn size(&self) -> Size {
            Size::new(240, 320)
        }
    }

    impl DrawTarget for Counter {
        type Color = Rgb565;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            self.pixels += pixels.into_iter().count() as u64;
            Ok(())
        }

        fn clear(&mut self, _color: Self::Color) -> Result<(), Self::Error> {
            self.clears += 1;
            Ok(())
        }
    }

    const PLAYING: &str = r#"{"status":"play","title":"Song","artist":"Band","album":"LP",
        "albumart":"/albumart?web=Band/LP/large","seek":1000,"duration":200,"volume":40}"#;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn rig(status: &'static str, state: &'static str) -> (App, FakePlayer, Counter) {
        let settings = Settings {
            player: String::from("http://p:3000"),
            ..Settings::default()
        };
        (
            App::new(&settings, CAPS, ms(0)),
            FakePlayer {
                status,
                state,
                asked: Vec::new(),
            },
            Counter {
                clears: 0,
                pixels: 0,
            },
        )
    }

    fn tap(app: &mut App, http: &mut FakePlayer, p: Point, now: Duration) {
        let sample = Some(TouchSample {
            x: p.x as u16,
            y: p.y as u16,
            pressure: 1,
            fingers: 1,
        });
        app.touch(sample, now, http);
        app.touch(None, now + ms(30), http);
    }

    fn centre(r: embedded_graphics::primitives::Rectangle) -> Point {
        r.center()
    }

    /// A point on `kind` that `surface::hit` reports as `want`.
    fn point_for(layout: &Layout, kind: Surface, want: SurfHit) -> Point {
        for y in (0..320).step_by(2) {
            for x in (0..240).step_by(2) {
                let p = Point::new(x, y);
                if surface::hit(layout, kind, p) == want {
                    return p;
                }
            }
        }
        panic!("no point on {kind:?} hits {want:?}");
    }

    #[test]
    fn status_screen_until_ready_then_the_face() {
        let (mut app, mut http, mut d) = rig("starting", PLAYING);
        let mut net = NoNet;
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        assert!(!app.on_player());
        assert!(http.asked.iter().any(|u| u.ends_with("/status")));
        assert!(
            !http.asked.iter().any(|u| u.ends_with("/getState")),
            "not before ready"
        );
        assert!(d.pixels > 0, "the status page was drawn");

        http.status = "ready";
        app.step(ms(1000), &mut http, &mut net, &mut d).unwrap();
        assert!(app.on_player());
        assert_eq!(app.state().title.as_deref(), Some("Song"));
        assert_eq!(d.clears, 1, "one full face paint");

        // The next poll with the same scene and a new seek repaints only the strip.
        http.state = r#"{"status":"play","title":"Song","artist":"Band","album":"LP",
            "albumart":"/albumart?web=Band/LP/large","seek":2000,"duration":200,"volume":40}"#;
        app.step(ms(1500), &mut http, &mut net, &mut d).unwrap();
        assert_eq!(d.clears, 1);
    }

    #[test]
    fn a_failed_poll_keeps_the_face() {
        let (mut app, mut http, mut d) = rig("ready", PLAYING);
        let mut net = NoNet;
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        assert!(app.on_player());
        http.state = "<html>503</html>";
        app.step(ms(500), &mut http, &mut net, &mut d).unwrap();
        assert!(app.on_player());
        assert_eq!(app.state().title.as_deref(), Some("Song"));
    }

    #[test]
    fn falls_through_to_getstate_after_the_cap() {
        let (mut app, mut http, mut d) = rig("starting", PLAYING);
        let mut net = NoNet;
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        app.step(STATUS_WAIT_CAP, &mut http, &mut net, &mut d)
            .unwrap();
        assert!(app.on_player());
    }

    #[test]
    fn art_is_asked_once_and_not_without_psram() {
        let (mut app, mut http, mut d) = rig("ready", PLAYING);
        let mut net = NoNet;
        assert!(
            app.art_request(ms(0)).is_none(),
            "nothing before the player"
        );
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        let req = app.art_request(ms(0)).unwrap();
        assert_eq!(req.url, "http://p:3000/albumart?web=Band/LP/large");
        assert_eq!(
            req.size,
            app.layout()
                .art
                .size
                .width
                .min(app.layout().art.size.height)
        );
        assert!(app.art_request(ms(10)).is_none(), "in flight");
        app.art_arrived(Some(Art::solid(10, 10, Rgb565::new(0, 0, 0))), ms(20));
        assert!(app.art_request(ms(30)).is_none(), "arrived");

        let settings = Settings {
            player: String::from("http://p:3000"),
            ..Settings::default()
        };
        let mut small = App::new(
            &settings,
            Capabilities {
                psram_bytes: 0,
                ..CAPS
            },
            ms(0),
        );
        small.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        assert!(small.on_player());
        assert!(small.art_request(ms(0)).is_none());
    }

    #[test]
    fn dock_tap_opens_controls_and_next_goes_out() {
        let (mut app, mut http, mut d) = rig("ready", PLAYING);
        let mut net = NoNet;
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        let layout = *app.layout();

        tap(&mut app, &mut http, centre(layout.dock_cell(0)), ms(100));
        assert_eq!(app.open_surface(), Some(Surface::Controls));

        let before = http.asked.len();
        tap(
            &mut app,
            &mut http,
            point_for(&layout, Surface::Controls, SurfHit::Next),
            ms(300),
        );
        let sent: Vec<_> = http.asked[before..]
            .iter()
            .filter(|u| u.contains("/commands/"))
            .collect();
        assert_eq!(sent, ["http://p:3000/api/v1/commands/?cmd=next"]);

        tap(
            &mut app,
            &mut http,
            point_for(&layout, Surface::Controls, SurfHit::Close),
            ms(500),
        );
        assert_eq!(app.open_surface(), None);
    }

    #[test]
    fn a_surface_closes_itself_after_the_hold() {
        let (mut app, mut http, mut d) = rig("ready", PLAYING);
        let mut net = NoNet;
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        let layout = *app.layout();
        tap(&mut app, &mut http, centre(layout.dock_cell(1)), ms(100));
        assert_eq!(app.open_surface(), Some(Surface::Volume));
        app.step(ms(130) + SURFACE_HOLD - ms(1), &mut http, &mut net, &mut d)
            .unwrap();
        assert_eq!(app.open_surface(), Some(Surface::Volume));
        app.step(ms(130) + SURFACE_HOLD, &mut http, &mut net, &mut d)
            .unwrap();
        assert_eq!(app.open_surface(), None);
    }

    #[test]
    fn touches_before_the_player_are_ignored() {
        let (mut app, mut http, mut d) = rig("starting", PLAYING);
        let mut net = NoNet;
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        let layout = *app.layout();
        tap(&mut app, &mut http, centre(layout.dock_cell(0)), ms(100));
        assert_eq!(app.open_surface(), None);
        assert!(!http.asked.iter().any(|u| u.contains("/commands/")));
    }

    #[test]
    fn holding_volume_up_repeats() {
        let (mut app, mut http, mut d) = rig("ready", PLAYING);
        let mut net = NoNet;
        app.step(ms(0), &mut http, &mut net, &mut d).unwrap();
        let layout = *app.layout();
        tap(&mut app, &mut http, centre(layout.dock_cell(1)), ms(100));
        let up = point_for(&layout, Surface::Volume, SurfHit::VolUp);
        let sample = Some(TouchSample {
            x: up.x as u16,
            y: up.y as u16,
            pressure: 1,
            fingers: 1,
        });
        app.touch(sample, ms(1000), &mut http);
        let before = http.asked.len();
        let commands = |asked: &[String]| asked.iter().filter(|u| u.contains("/commands/")).count();
        app.step(ms(1000) + HOLD_DELAY - ms(1), &mut http, &mut net, &mut d)
            .unwrap();
        assert_eq!(
            commands(&http.asked[before..]),
            0,
            "not before the hold delay"
        );
        app.step(ms(1000) + HOLD_DELAY, &mut http, &mut net, &mut d)
            .unwrap();
        assert!(http.asked[before..]
            .iter()
            .any(|u| u.ends_with("cmd=volume&volume=42")));
        app.touch(None, ms(1000) + HOLD_DELAY + ms(10), &mut http);
    }
}
