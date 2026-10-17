//! Transient surfaces.
//!
//! Carried over from rpi-waveshare28 `runtime/src/surface.rs` at `61ecf6d`.
//!
//! One surface at a time. Close on 10 s inactivity or an outside tap.
//! Token map is the same as the face: accent is volume fill only.
//!
//! Status and metadata fill leftover body with `FONT_10X20` — the biggest
//! stock face. A.3's ~11/~9/~8 map wasted the column on glass. Line caps
//! come from leftover height so the same scale fits both orientations.
//! The resting face keeps `TITLE_FONT` / `META_FONT`.

use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoFont, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle, Triangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::time::Duration;

use companion_hal::HostInfo;

use crate::art::Art;
use crate::state::PlayerState;
use crate::ui::{self, Layout, Palette, META_FONT, TITLE_FONT};

/// How long a surface stays up without a touch, in seconds. Painted as a
/// countdown in the IP box.
pub const HOLD_SECS: u8 = 10;
/// Biggest stock mono face. Status + metadata only; IPv6 wraps.
const SURF_FONT: &MonoFont = &FONT_10X20;
const SURF_LINE_PAD: i32 = 2;
const SURF_FIELD_GAP: i32 = 8;
/// After title, before the format block.
const META_GAPS: i32 = SURF_FIELD_GAP * 2;
/// Title · artist · album · stream · service.
const META_WEIGHTS: [u32; 5] = [3, 2, 2, 1, 1];
const META_MINS: [usize; 5] = [2, 1, 1, 1, 1];
const META_MAXS: [usize; 5] = [6, 4, 4, 3, 2];

/// A transient surface over the resting face. One at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Transport: previous, play/pause, next, seek, shuffle, repeat.
    Controls,
    /// The volume slider, steps and mute.
    Volume,
    /// Title, artist, album, stream and service in full.
    Metadata,
    /// Hostname, addresses and the hotspot.
    Status,
    /// The cover alone, as large as the frame allows.
    Artwork,
}

/// What a touch on a surface landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfHit {
    /// The header or outside the frame: dismiss the surface.
    Close,
    /// Previous track.
    Prev,
    /// Toggle play and pause.
    PlayPause,
    /// Next track.
    Next,
    /// The seek strip; the caller reads the fraction.
    Seek,
    /// Toggle shuffle.
    Shuffle,
    /// Toggle repeat.
    Repeat,
    /// Volume up one step.
    VolUp,
    /// Volume down one step.
    VolDown,
    /// The volume slider; the caller reads the fraction.
    VolSlider,
    /// The speaker glyph: toggle mute.
    Speaker,
}

impl Surface {
    /// The word in the surface's header.
    pub fn label(self) -> &'static str {
        match self {
            Self::Controls => "CONTROLS",
            Self::Volume => "VOLUME",
            Self::Metadata => "TRACK",
            Self::Status => "STATUS",
            Self::Artwork => "ART",
        }
    }
}

fn header(layout: &Layout) -> Rectangle {
    let h = if layout.frame.size.width > layout.frame.size.height {
        24
    } else {
        28
    };
    Rectangle::new(layout.frame.top_left, Size::new(layout.frame.size.width, h))
}

fn body(layout: &Layout) -> Rectangle {
    let h = header(layout);
    Rectangle::new(
        Point::new(h.top_left.x, h.top_left.y + h.size.height as i32),
        Size::new(
            layout.frame.size.width,
            layout.frame.size.height - h.size.height,
        ),
    )
}

fn thirds_wide(band: Rectangle, play_wide: bool) -> [Rectangle; 3] {
    let w = band.size.width as i32;
    let (a, b, c) = if play_wide {
        let play = w * 128 / 320;
        let side = (w - play) / 2;
        (side, play, w - side - play)
    } else {
        let t = w / 3;
        (t, w - 2 * t, t)
    };
    let y = band.top_left.y;
    let h = band.size.height;
    let x = band.top_left.x;
    [
        Rectangle::new(Point::new(x, y), Size::new(a as u32, h)),
        Rectangle::new(Point::new(x + a, y), Size::new(b as u32, h)),
        Rectangle::new(Point::new(x + a + b, y), Size::new(c as u32, h)),
    ]
}

/// Map a point on `surface` to what it landed on. Artwork closes on any
/// touch; every surface closes on its header or outside the frame.
pub fn hit(layout: &Layout, surface: Surface, p: Point) -> SurfHit {
    if surface == Surface::Artwork {
        return SurfHit::Close;
    }
    let h = header(layout);
    if h.contains(p) || !layout.frame.contains(p) {
        return SurfHit::Close;
    }
    match surface {
        Surface::Artwork => SurfHit::Close,
        Surface::Controls => {
            let rows = controls_bands(layout);
            let transport = thirds_wide(rows[0], true);
            if transport[0].contains(p) {
                return SurfHit::Prev;
            }
            if transport[1].contains(p) {
                return SurfHit::PlayPause;
            }
            if transport[2].contains(p) {
                return SurfHit::Next;
            }
            if rows[1].contains(p) {
                return SurfHit::Seek;
            }
            let modes = thirds_wide(rows[2], false);
            if modes[0].contains(p) {
                return SurfHit::Shuffle;
            }
            SurfHit::Repeat
        }
        Surface::Volume => {
            let v = volume_chrome(layout);
            if v.up.contains(p) {
                return SurfHit::VolUp;
            }
            if v.down.contains(p) {
                return SurfHit::VolDown;
            }
            if v.slider.contains(p) {
                return SurfHit::VolSlider;
            }
            SurfHit::Speaker
        }
        Surface::Metadata | Surface::Status => SurfHit::Close,
    }
}

/// Transport, seek, modes. Seek is a fixed 40 px so it stays a hit in
/// landscape; modes take the 120/292 share so the three lamps are the
/// floor, not a leftover strip.
fn controls_bands(layout: &Layout) -> [Rectangle; 3] {
    let b = body(layout);
    let seek_h = 40u32;
    let modes_h = (b.size.height * 120 / 292).max(80);
    let trans_h = b.size.height.saturating_sub(seek_h + modes_h);
    let x = b.top_left.x;
    let y = b.top_left.y;
    let w = b.size.width;
    [
        Rectangle::new(Point::new(x, y), Size::new(w, trans_h)),
        Rectangle::new(Point::new(x, y + trans_h as i32), Size::new(w, seek_h)),
        Rectangle::new(
            Point::new(x, y + trans_h as i32 + seek_h as i32),
            Size::new(w, modes_h),
        ),
    ]
}

/// Seek band on the Controls surface. Same box draw and hit use.
pub fn controls_seek_slot(layout: &Layout) -> Rectangle {
    controls_bands(layout)[1]
}

/// Repaint only that band. A full Controls redraw on every seek tick
/// clears the transport and flickers while playing.
pub fn draw_controls_seek<D>(
    target: &mut D,
    layout: &Layout,
    state: &PlayerState,
    scrub: Option<f32>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    ui::draw_seek_strip(
        target,
        controls_seek_slot(layout),
        state,
        layout.pal(),
        scrub,
        false,
    )
}

fn draw_header<D>(
    target: &mut D,
    layout: &Layout,
    pal: Palette,
    label: &str,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let h = header(layout);
    h.into_styled(PrimitiveStyle::with_fill(pal.bg))
        .draw(target)?;
    let style = MonoTextStyle::new(META_FONT, pal.dim);
    Text::new(
        label,
        Point::new(h.top_left.x + 12, h.top_left.y + 8),
        style,
    )
    .draw(target)?;
    Ok(())
}

/// Ceil remaining hold into 0..=10. `10.0` is 10; `9.1` is still 10.
pub fn hold_secs(left: Duration) -> u8 {
    let ceil = left
        .as_secs()
        .saturating_add(u64::from(left.subsec_nanos() > 0));
    ceil.min(u64::from(HOLD_SECS)) as u8
}

/// `{n}s` in the IP box so the clock stays off the body.
pub fn draw_hold<D>(target: &mut D, layout: &Layout, remain: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let pal = layout.pal();
    let box_ = ui::info_ring_box(layout);
    box_.into_styled(PrimitiveStyle::with_fill(pal.bg))
        .draw(target)?;
    let style = MonoTextStyle::new(META_FONT, pal.dim);
    let centred = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let label = format!("{remain}s");
    Text::with_text_style(&label, box_.center(), style, centred).draw(target)?;
    Ok(())
}

fn play_icon<D>(target: &mut D, c: Point, colour: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Triangle::new(
        Point::new(c.x - 8, c.y - 12),
        Point::new(c.x - 8, c.y + 12),
        Point::new(c.x + 12, c.y),
    )
    .into_styled(PrimitiveStyle::with_fill(colour))
    .draw(target)?;
    Ok(())
}

fn pause_icon<D>(target: &mut D, c: Point, colour: Rgb565) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Rectangle::new(Point::new(c.x - 8, c.y - 12), Size::new(5, 24))
        .into_styled(PrimitiveStyle::with_fill(colour))
        .draw(target)?;
    Rectangle::new(Point::new(c.x + 3, c.y - 12), Size::new(5, 24))
        .into_styled(PrimitiveStyle::with_fill(colour))
        .draw(target)?;
    Ok(())
}

fn skip_icon<D>(target: &mut D, c: Point, colour: Rgb565, next: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let dir = if next { 1 } else { -1 };
    Triangle::new(
        Point::new(c.x - 8 * dir, c.y - 10),
        Point::new(c.x - 8 * dir, c.y + 10),
        Point::new(c.x + 6 * dir, c.y),
    )
    .into_styled(PrimitiveStyle::with_fill(colour))
    .draw(target)?;
    Rectangle::new(Point::new(c.x + 7 * dir - 1, c.y - 10), Size::new(3, 20))
        .into_styled(PrimitiveStyle::with_fill(colour))
        .draw(target)?;
    Ok(())
}

const VOL_PAD: i32 = 12;
const VOL_ARM: u32 = 32;
const VOL_ARM_T: u32 = 3;
/// Visual fill. Lives inside the 44 px hit, not the whole band.
const VOL_TROUGH_H: u32 = 12;
const VOL_SLIDER_HIT: u32 = 44;
const VOL_FACTS_H: u32 = 40;
const VOL_SPEAKER_SCALE: i32 = 2;
const VOL_GAP: i32 = 8;

#[derive(Debug, Clone, Copy)]
struct VolumeChrome {
    up: Rectangle,
    down: Rectangle,
    readout: Rectangle,
    slider: Rectangle,
}

/// Same three rows in both orientations: plus, readout, minus.
/// Plus and minus share the leftover height after the readout (facts +
/// 44 px slider). The 146/60/86 weights made minus smaller than plus
/// and put a 4 px trough on the minus band.
fn volume_chrome(layout: &Layout) -> VolumeChrome {
    let b = body(layout);
    let x = b.top_left.x;
    let y = b.top_left.y;
    let w = b.size.width;
    let h = b.size.height;
    let read_h = VOL_SLIDER_HIT + VOL_FACTS_H;
    let rest = h.saturating_sub(read_h);
    let side = rest / 2;
    let down_h = rest - side;
    let up = Rectangle::new(Point::new(x, y), Size::new(w, side));
    let readout = Rectangle::new(Point::new(x, y + side as i32), Size::new(w, read_h));
    let down = Rectangle::new(
        Point::new(x, y + side as i32 + read_h as i32),
        Size::new(w, down_h),
    );
    let slider = Rectangle::new(
        Point::new(
            x,
            readout.top_left.y + readout.size.height as i32 - VOL_SLIDER_HIT as i32,
        ),
        Size::new(w, VOL_SLIDER_HIT),
    );
    VolumeChrome {
        up,
        down,
        readout,
        slider,
    }
}

fn volume_facts(v: VolumeChrome) -> Rectangle {
    let h = (v.slider.top_left.y - v.readout.top_left.y).max(0) as u32;
    Rectangle::new(v.readout.top_left, Size::new(v.readout.size.width, h))
}

fn volume_trough(slider: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(
            slider.top_left.x + VOL_PAD,
            slider.center().y - VOL_TROUGH_H as i32 / 2,
        ),
        Size::new(
            slider.size.width.saturating_sub((VOL_PAD * 2) as u32),
            VOL_TROUGH_H,
        ),
    )
}

/// x in the slider maps 0–100. Same pad as the seek strip.
pub fn volume_at(layout: &Layout, p: Point) -> u8 {
    let trough = volume_trough(volume_chrome(layout).slider);
    let w = trough.size.width.max(1) as i32;
    let dx = (p.x - trough.top_left.x).clamp(0, w);
    ((dx * 100) / w) as u8
}

fn plus_minus<D>(
    target: &mut D,
    cell: Rectangle,
    plus: bool,
    colour: Rgb565,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let c = cell.center();
    Rectangle::with_center(c, Size::new(VOL_ARM, VOL_ARM_T))
        .into_styled(PrimitiveStyle::with_fill(colour))
        .draw(target)?;
    if plus {
        Rectangle::with_center(c, Size::new(VOL_ARM_T, VOL_ARM))
            .into_styled(PrimitiveStyle::with_fill(colour))
            .draw(target)?;
    }
    Ok(())
}

fn hairline<D>(target: &mut D, band: Rectangle, y: i32, pal: Palette) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    Line::new(
        Point::new(band.top_left.x + VOL_PAD, y),
        Point::new(band.top_left.x + band.size.width as i32 - VOL_PAD, y),
    )
    .into_styled(PrimitiveStyle::with_stroke(pal.dim, 1))
    .draw(target)?;
    Ok(())
}

/// Speaker + number as one group, `with_center` on the facts band.
fn volume_readout_geom(band: Rectangle, label: &str, font: &MonoFont<'_>) -> (Rectangle, Point) {
    let speaker = ui::speaker_box(Point::zero(), VOL_SPEAKER_SCALE);
    let sw = speaker.size.width as i32;
    let sh = speaker.size.height as i32;
    let tw = label.chars().count() as i32 * font.character_size.width as i32;
    let th = font.character_size.height as i32;
    let group_w = sw + VOL_GAP + tw;
    let group_h = sh.max(th);
    let group = Rectangle::with_center(band.center(), Size::new(group_w as u32, group_h as u32));
    let speaker = Rectangle::new(
        Point::new(group.top_left.x, group.top_left.y + (group_h - sh) / 2),
        Size::new(sw as u32, sh as u32),
    );
    let text = Point::new(speaker.top_left.x + sw + VOL_GAP, speaker.center().y);
    (speaker, text)
}

fn mode_cell<D>(
    target: &mut D,
    cell: Rectangle,
    label: &str,
    on: bool,
    pal: Palette,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    // Accent so ON reads orange on Ink. Danger stays mute.
    let colour = if on { pal.accent } else { pal.dim };
    let style = MonoTextStyle::new(TITLE_FONT, colour);
    let label_h = TITLE_FONT.character_size.height as i32;
    let gap = 8;
    let dot = 10i32;
    let group_h = label_h + gap + dot;
    let group = Rectangle::with_center(cell.center(), Size::new(cell.size.width, group_h as u32));
    let cx = cell.center().x;
    let centred = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(label, Point::new(cx, group.top_left.y), style, centred).draw(target)?;
    let lamp = Point::new(cx, group.top_left.y + label_h + gap + dot / 2);
    if on {
        Circle::with_center(lamp, dot as u32)
            .into_styled(PrimitiveStyle::with_fill(colour))
            .draw(target)?;
    } else {
        Circle::with_center(lamp, dot as u32)
            .into_styled(PrimitiveStyle::with_stroke(pal.dim, 2))
            .draw(target)?;
    }
    Ok(())
}

/// Paint `surface` over the whole frame. `scrub` is the live seek fraction
/// while a finger is on the strip, else `None`.
pub fn draw<D>(
    target: &mut D,
    layout: &Layout,
    surface: Surface,
    state: &PlayerState,
    art: Option<&Art>,
    host: &HostInfo,
    scrub: Option<f32>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let pal = layout.pal();
    target.clear(pal.bg)?;
    if surface == Surface::Artwork {
        return draw_artwork(target, layout, art);
    }
    draw_header(target, layout, pal, surface.label())?;
    match surface {
        Surface::Controls => draw_controls(target, layout, state, pal, scrub),
        Surface::Volume => draw_volume(target, layout, state, pal),
        Surface::Metadata => draw_metadata(target, layout, state, pal),
        Surface::Status => draw_status(target, layout, state, host, pal),
        Surface::Artwork => Ok(()),
    }
}

/// Contain: scale until the first frame edge is hit, then letterbox.
fn contain_rect(frame: Rectangle, art_w: u32, art_h: u32) -> Rectangle {
    if art_w == 0 || art_h == 0 {
        return Rectangle::new(frame.top_left, Size::zero());
    }
    let fw = frame.size.width;
    let fh = frame.size.height;
    let (w, h) = if art_w.saturating_mul(fh) <= art_h.saturating_mul(fw) {
        (art_w.saturating_mul(fh) / art_h, fh)
    } else {
        (fw, art_h.saturating_mul(fw) / art_w)
    };
    let w = w.max(1).min(fw);
    let h = h.max(1).min(fh);
    let x = frame.top_left.x + (fw as i32 - w as i32) / 2;
    let y = frame.top_left.y + (fh as i32 - h as i32) / 2;
    Rectangle::new(Point::new(x, y), Size::new(w, h))
}

fn scale_nearest(art: &Art, dw: u32, dh: u32) -> Vec<Rgb565> {
    if dw == art.w && dh == art.h {
        return art.px.clone();
    }
    let mut out = Vec::with_capacity((dw * dh) as usize);
    for y in 0..dh {
        let sy = ((y as u64 * art.h as u64) / dh as u64).min(art.h as u64 - 1) as u32;
        let row = sy * art.w;
        for x in 0..dw {
            let sx = ((x as u64 * art.w as u64) / dw as u64).min(art.w as u64 - 1) as u32;
            out.push(art.px[(row + sx) as usize]);
        }
    }
    out
}

fn draw_artwork<D>(target: &mut D, layout: &Layout, art: Option<&Art>) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let frame = layout.frame;
    if let Some(art) = art {
        let placed = contain_rect(frame, art.w, art.h);
        if placed.size.width > 0 && placed.size.height > 0 {
            let px = scale_nearest(art, placed.size.width, placed.size.height);
            target.fill_contiguous(&placed, px)?;
        }
    }
    Ok(())
}

fn draw_controls<D>(
    target: &mut D,
    layout: &Layout,
    state: &PlayerState,
    pal: Palette,
    scrub: Option<f32>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let rows = controls_bands(layout);
    let transport = thirds_wide(rows[0], true);
    skip_icon(target, transport[0].center(), pal.meta, false)?;
    if state.is_playing() {
        pause_icon(target, transport[1].center(), pal.title)?;
    } else {
        play_icon(target, transport[1].center(), pal.title)?;
    }
    skip_icon(target, transport[2].center(), pal.meta, true)?;
    ui::draw_seek_strip(target, rows[1], state, pal, scrub, false)?;
    let modes = thirds_wide(rows[2], false);
    mode_cell(target, modes[0], "SHUF", state.random.unwrap_or(false), pal)?;
    mode_cell(target, modes[1], "REP", state.repeat.unwrap_or(false), pal)?;
    mode_cell(
        target,
        modes[2],
        "ONE",
        state.repeat_single.unwrap_or(false),
        pal,
    )?;
    Ok(())
}

fn draw_volume<D>(
    target: &mut D,
    layout: &Layout,
    state: &PlayerState,
    pal: Palette,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let v = volume_chrome(layout);
    plus_minus(target, v.up, true, pal.title)?;
    let muted = state.is_muted();
    let vol = state.volume.unwrap_or(0);
    let silent = muted || vol == 0;
    let (label, font, ink) = if muted {
        ("MUTED".to_string(), META_FONT, pal.dim)
    } else {
        (format!("{vol}"), TITLE_FONT, pal.title)
    };
    let facts = volume_facts(v);
    let (speaker, text) = volume_readout_geom(facts, &label, font);
    ui::draw_speaker_at(target, speaker.center(), silent, pal, VOL_SPEAKER_SCALE)?;
    let style = MonoTextStyle::new(font, ink);
    let mid_left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();
    Text::with_text_style(&label, text, style, mid_left).draw(target)?;
    hairline(target, v.slider, v.slider.top_left.y, pal)?;
    let trough = volume_trough(v.slider);
    trough
        .into_styled(PrimitiveStyle::with_fill(pal.dim))
        .draw(target)?;
    let frac = if muted { 0.0 } else { f32::from(vol) / 100.0 };
    let filled = (trough.size.width as f32 * frac.clamp(0.0, 1.0)) as u32;
    if filled > 0 {
        Rectangle::new(trough.top_left, Size::new(filled, VOL_TROUGH_H))
            .into_styled(PrimitiveStyle::with_fill(pal.accent))
            .draw(target)?;
    }
    plus_minus(target, v.down, false, pal.title)?;
    Ok(())
}

fn text_col(layout: &Layout) -> MetaCol {
    let b = body(layout);
    MetaCol {
        x: b.top_left.x + 12,
        width: b.size.width.saturating_sub(24),
        top: b.top_left.y + 8,
        floor: b.top_left.y + b.size.height as i32 - 8,
    }
}

fn surf_line_h() -> i32 {
    SURF_FONT.character_size.height as i32 + SURF_LINE_PAD
}

/// Split leftover body lines across the five metadata fields.
/// Same weights both ways; the taller portrait body gets more title.
fn metadata_caps(col: MetaCol) -> [usize; 5] {
    let usable = (col.floor - col.top - META_GAPS).max(0);
    let budget = (usable / surf_line_h()) as usize;
    distribute(budget, &META_WEIGHTS, &META_MINS, &META_MAXS)
}

fn distribute(budget: usize, weights: &[u32], mins: &[usize], maxs: &[usize]) -> [usize; 5] {
    let mut caps = [0usize; 5];
    let mut used = 0usize;
    for i in 0..5 {
        caps[i] = mins[i];
        used = used.saturating_add(mins[i]);
    }
    let extra = budget.saturating_sub(used);
    let wsum: u32 = weights.iter().sum();
    let mut given = 0usize;
    if wsum > 0 {
        for i in 0..5 {
            let share = extra * weights[i] as usize / wsum as usize;
            let room = maxs[i].saturating_sub(caps[i]);
            let add = share.min(room);
            caps[i] += add;
            given += add;
        }
    }
    let mut left = extra.saturating_sub(given);
    while left > 0 {
        let mut grew = false;
        let mut order: [usize; 5] = [0, 1, 2, 3, 4];
        order.sort_by(|a, b| weights[*b].cmp(&weights[*a]).then(a.cmp(b)));
        for i in order {
            if left == 0 {
                break;
            }
            if caps[i] < maxs[i] {
                caps[i] += 1;
                left -= 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    caps
}

fn draw_metadata<D>(
    target: &mut D,
    layout: &Layout,
    state: &PlayerState,
    pal: Palette,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let col = text_col(layout);
    let [title_n, artist_n, album_n, stream_n, service_n] = metadata_caps(col);
    let mut y = col.top;
    y = paint_wrapped(
        target,
        state.title.as_deref().unwrap_or(""),
        y,
        pal.title,
        title_n,
        col,
    )?;
    y += SURF_FIELD_GAP;
    if let Some(a) = state.artist.as_deref() {
        y = paint_wrapped(target, a, y, pal.meta, artist_n, col)?;
    }
    if let Some(a) = state.album.as_deref() {
        y = paint_wrapped(target, a, y, pal.dim, album_n, col)?;
    }
    y += SURF_FIELD_GAP;
    if let Some(info) = state.stream_info() {
        y = paint_wrapped(target, &info, y, pal.meta, stream_n, col)?;
    }
    if let Some(svc) = state.service.as_deref() {
        paint_wrapped(target, svc, y, pal.meta, service_n, col)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct MetaCol {
    x: i32,
    width: u32,
    top: i32,
    floor: i32,
}

fn paint_wrapped<D>(
    target: &mut D,
    text: &str,
    mut y: i32,
    colour: Rgb565,
    max_lines: usize,
    col: MetaCol,
) -> Result<i32, D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if text.is_empty() {
        return Ok(y);
    }
    let lines = ui::wrap(text, col.width, SURF_FONT);
    let line_h = surf_line_h();
    let face = MonoTextStyle::new(SURF_FONT, colour);
    let top = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Left)
        .build();
    let cap = lines.len().min(max_lines);
    for (i, line) in lines.iter().take(cap).enumerate() {
        if y + SURF_FONT.character_size.height as i32 > col.floor {
            break;
        }
        let t = if i + 1 == cap && lines.len() > cap {
            let mut s = line.clone();
            s.push('…');
            s
        } else {
            line.clone()
        };
        Text::with_text_style(&t, Point::new(col.x, y), face, top).draw(target)?;
        y += line_h;
    }
    Ok(y)
}

fn draw_status<D>(
    target: &mut D,
    layout: &Layout,
    state: &PlayerState,
    host: &HostInfo,
    pal: Palette,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let col = text_col(layout);
    let mut y = col.top;
    for a in &host.addrs {
        y = paint_wrapped(
            target,
            &format!("{}  {}", a.link.label(), a.addr),
            y,
            pal.title,
            4,
            col,
        )?;
        y += SURF_FIELD_GAP;
    }
    y = paint_wrapped(target, &host.hostname, y, pal.meta, 3, col)?;
    y += SURF_FIELD_GAP;
    if let Some(info) = state.stream_info() {
        y = paint_wrapped(target, &info, y, pal.meta, 3, col)?;
        y += SURF_FIELD_GAP;
    }
    paint_wrapped(
        target,
        &format!("waveshare28  {}", layout.theme.as_str()),
        y,
        pal.dim,
        2,
        col,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        contain_rect, metadata_caps, scale_nearest, surf_line_h, text_col, META_GAPS, META_MINS,
        SURF_FONT,
    };
    use crate::art::Art;
    use crate::ui::{self, Layout};
    use embedded_graphics::mono_font::ascii::FONT_10X20;
    use embedded_graphics::pixelcolor::Rgb565;

    fn origin(r: Rectangle) -> (i32, i32, u32, u32) {
        (r.top_left.x, r.top_left.y, r.size.width, r.size.height)
    }

    #[test]
    fn artwork_contains_to_the_nearest_edge() {
        let p = Layout::for_rotation(0).frame;
        assert_eq!(origin(contain_rect(p, 186, 186)), (0, 40, 240, 240));
        let l = Layout::for_rotation(270).frame;
        assert_eq!(origin(contain_rect(l, 186, 186)), (40, 0, 240, 240));
        assert_eq!(origin(contain_rect(p, 200, 100)), (0, 100, 240, 120));
        assert_eq!(origin(contain_rect(p, 100, 200)), (40, 0, 160, 320));
    }

    #[test]
    fn artwork_scale_keeps_corners() {
        let art = Art {
            w: 2,
            h: 2,
            px: vec![
                Rgb565::new(31, 0, 0),
                Rgb565::new(0, 63, 0),
                Rgb565::new(0, 0, 31),
                Rgb565::new(31, 63, 31),
            ],
        };
        let out = scale_nearest(&art, 4, 4);
        assert_eq!(out.len(), 16);
        assert_eq!(out[0], art.px[0]);
        assert_eq!(out[3], art.px[1]);
        assert_eq!(out[12], art.px[2]);
        assert_eq!(out[15], art.px[3]);
    }

    #[test]
    fn hold_secs_ceils_the_remaining_second() {
        assert_eq!(hold_secs(Duration::from_secs(10)), 10);
        assert_eq!(hold_secs(Duration::from_millis(9900)), 10);
        assert_eq!(hold_secs(Duration::from_secs(9)), 9);
        assert_eq!(hold_secs(Duration::from_millis(1)), 1);
        assert_eq!(hold_secs(Duration::ZERO), 0);
    }

    #[test]
    fn hold_clock_sits_on_the_ip_box() {
        let p = Layout::for_rotation(0);
        let l = Layout::for_rotation(270);
        assert_eq!(origin(ui::info_ring_box(&p)), (220, 2, 18, 18));
        assert_eq!(origin(ui::info_ring_box(&l)), (300, 2, 18, 18));
    }

    #[test]
    fn header_and_artwork_close() {
        let l = Layout::for_rotation(0);
        assert_eq!(
            hit(&l, Surface::Controls, Point::new(10, 10)),
            SurfHit::Close
        );
        assert_eq!(
            hit(&l, Surface::Artwork, Point::new(120, 160)),
            SurfHit::Close
        );
    }

    #[test]
    fn volume_surface_hits_stepper_and_speaker() {
        let l = Layout::for_rotation(0);
        let v = volume_chrome(&l);
        assert_eq!(hit(&l, Surface::Volume, v.up.center()), SurfHit::VolUp);
        assert_eq!(
            hit(&l, Surface::Volume, volume_facts(v).center()),
            SurfHit::Speaker
        );
        assert_eq!(
            hit(&l, Surface::Volume, v.slider.center()),
            SurfHit::VolSlider
        );
        assert_eq!(hit(&l, Surface::Volume, v.down.center()), SurfHit::VolDown);
    }

    #[test]
    fn volume_plus_and_minus_are_equal() {
        for rot in [0, 270] {
            let l = Layout::for_rotation(rot);
            let v = volume_chrome(&l);
            assert_eq!(v.up.size.height, v.down.size.height, "rot {rot}");
            assert_eq!(v.up.size.width, v.down.size.width);
            assert_eq!(v.up.center().x, v.down.center().x);
            assert!(v.slider.size.height >= 44);
            assert!(
                v.slider.top_left.y + v.slider.size.height as i32 <= v.down.top_left.y,
                "slider must not sit in minus"
            );
            assert_eq!(
                hit(&l, Surface::Volume, v.slider.center()),
                SurfHit::VolSlider
            );
            assert_eq!(hit(&l, Surface::Volume, v.down.center()), SurfHit::VolDown);
        }
    }

    #[test]
    fn volume_slider_maps_the_trough() {
        let layout = Layout::for_rotation(270);
        let trough = volume_trough(volume_chrome(&layout).slider);
        assert_eq!(volume_at(&layout, trough.top_left), 0);
        assert_eq!(
            volume_at(
                &layout,
                Point::new(
                    trough.top_left.x + trough.size.width as i32,
                    trough.center().y
                )
            ),
            100
        );
    }

    #[test]
    fn controls_seek_slot_is_the_middle_band() {
        let l = Layout::for_rotation(0);
        let slot = controls_seek_slot(&l);
        assert!(slot.top_left.y > 28);
        assert_eq!(slot.size.height, 40);
        assert_eq!(slot.size.width, 240);
        let rows = controls_bands(&l);
        assert!(
            slot.top_left.y + slot.size.height as i32 <= rows[2].top_left.y,
            "seek sits above the modes, not in them"
        );
    }

    #[test]
    fn controls_modes_are_equal_thirds() {
        for rot in [0, 270] {
            let l = Layout::for_rotation(rot);
            let modes = thirds_wide(controls_bands(&l)[2], false);
            assert_eq!(modes[0].size.width, modes[2].size.width);
            assert_eq!(modes[0].center().y, modes[1].center().y);
            assert_eq!(modes[1].center().y, modes[2].center().y);
            assert_eq!(
                hit(&l, Surface::Controls, modes[0].center()),
                SurfHit::Shuffle
            );
            assert_eq!(
                hit(&l, Surface::Controls, modes[1].center()),
                SurfHit::Repeat
            );
        }
    }

    #[test]
    fn controls_surface_hits_play_and_modes() {
        let l = Layout::for_rotation(0);
        assert_eq!(
            hit(&l, Surface::Controls, Point::new(120, 80)),
            SurfHit::PlayPause
        );
        assert_eq!(
            hit(&l, Surface::Controls, Point::new(40, 300)),
            SurfHit::Shuffle
        );
        assert_eq!(
            hit(&l, Surface::Controls, Point::new(160, 300)),
            SurfHit::Repeat
        );
        // ONE is a lamp. REST cannot set it; the tap is repeat.
        assert_eq!(
            hit(&l, Surface::Controls, Point::new(220, 300)),
            SurfHit::Repeat
        );
    }

    #[test]
    fn metadata_album_wraps_to_the_column() {
        let l = Layout::for_rotation(270);
        let width = text_col(&l).width;
        let album = "A Very Long Album Title That Would Drive Into The Neighbour Driveway";
        let parts = ui::wrap(album, width, SURF_FONT);
        assert!(parts.len() >= 2, "{parts:?}");
        let cols = (width / SURF_FONT.character_size.width) as usize;
        assert!(parts.iter().all(|p| p.chars().count() <= cols));
    }

    fn stack_px(caps: [usize; 5]) -> i32 {
        let lines: usize = caps.iter().sum();
        lines as i32 * surf_line_h() + META_GAPS
    }

    #[test]
    fn text_surfaces_use_the_largest_stock_face() {
        assert_eq!(SURF_FONT.character_size, FONT_10X20.character_size);
        for rot in [0, 270] {
            let l = Layout::for_rotation(rot);
            let col = text_col(&l);
            assert!(
                col.width >= SURF_FONT.character_size.width * 21,
                "rot {rot}: IPv4 must fit one line"
            );
            let caps = metadata_caps(col);
            assert!(
                stack_px(caps) <= col.floor - col.top,
                "rot {rot}: {caps:?} stack {} > {}",
                stack_px(caps),
                col.floor - col.top
            );
            assert!(caps[0] >= 2);
            assert!(caps.iter().sum::<usize>() >= META_MINS.iter().sum());
        }
        let p = metadata_caps(text_col(&Layout::for_rotation(0)));
        let l = metadata_caps(text_col(&Layout::for_rotation(270)));
        assert!(p[0] >= l[0], "taller body keeps the title lines");
    }

    #[test]
    fn status_ipv6_wraps_to_the_column() {
        let col = text_col(&Layout::for_rotation(0));
        let v6 = "wlan0  fe80::1a2b:3c4d:5e6f:7890";
        let parts = ui::wrap(v6, col.width, SURF_FONT);
        assert!(parts.len() >= 2, "{parts:?}");
        let cols = (col.width / SURF_FONT.character_size.width) as usize;
        assert!(parts.iter().all(|p| p.chars().count() <= cols));
    }

    #[test]
    fn landscape_controls_modes_are_the_bottom_band() {
        let l = Layout::for_rotation(270);
        assert_eq!(
            hit(&l, Surface::Controls, Point::new(40, 220)),
            SurfHit::Shuffle
        );
        assert_eq!(
            hit(&l, Surface::Controls, Point::new(240, 220)),
            SurfHit::Repeat
        );
    }
}
