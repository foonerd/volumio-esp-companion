//! Press, move and release from raw touch reports.
//!
//! Carried over from rpi-waveshare28 `runtime/src/input.rs` at `61ecf6d`,
//! with the thread and the channel removed: the device reads its
//! controller when the interrupt line says so and feeds the report here;
//! what comes out is the event the face understands, already mapped
//! through the layout's rotation.
//!
//! The rules the Pi settled on and this keeps: a new press within the
//! debounce of the last is dropped; moves are rated to one per 40 ms so a
//! scrub does not flood the loop; a lift reports the last point seen,
//! because the controller's release packet carries none.

use core::time::Duration;
use embedded_graphics::prelude::Point;

use companion_core::{Layout, TouchEv};
use companion_hal::TouchSample;

/// Minimum gap between two accepted move events.
const MOVE_RATE: Duration = Duration::from_millis(40);

/// The finger's state between reports.
#[derive(Debug, Clone)]
pub struct Gestures {
    debounce: Duration,
    finger: bool,
    last_down: Option<Duration>,
    last_move: Duration,
    last_pt: Point,
}

impl Gestures {
    /// A deriver that drops a press landing within `debounce` of the last.
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            finger: false,
            last_down: None,
            last_move: Duration::ZERO,
            last_pt: Point::zero(),
        }
    }

    /// True while a finger is down.
    pub fn is_down(&self) -> bool {
        self.finger
    }

    /// Feed one report. `None` is an empty report or a release.
    ///
    /// `now` is the device's monotonic clock. Returns the event the report
    /// amounts to, if any.
    pub fn feed(
        &mut self,
        report: Option<TouchSample>,
        now: Duration,
        layout: &Layout,
    ) -> Option<TouchEv> {
        match report {
            Some(t) => {
                let p = layout.map(t);
                if !self.finger {
                    if let Some(last) = self.last_down {
                        if now.saturating_sub(last) < self.debounce {
                            return None;
                        }
                    }
                    self.finger = true;
                    self.last_down = Some(now);
                    self.last_pt = p;
                    self.last_move = now;
                    Some(TouchEv::Down(p))
                } else if now.saturating_sub(self.last_move) >= MOVE_RATE {
                    self.last_pt = p;
                    self.last_move = now;
                    Some(TouchEv::Move(p))
                } else {
                    None
                }
            }
            None => {
                if self.finger {
                    self.finger = false;
                    Some(TouchEv::Up(self.last_pt))
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: u16, y: u16) -> Option<TouchSample> {
        Some(TouchSample {
            x,
            y,
            pressure: 10,
            fingers: 1,
        })
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn down_move_up_in_order() {
        let layout = Layout::for_rotation(0);
        let mut g = Gestures::new(ms(50));
        assert_eq!(
            g.feed(at(10, 20), ms(0), &layout),
            Some(TouchEv::Down(Point::new(10, 20)))
        );
        assert!(g.is_down());
        // Too soon for a move.
        assert_eq!(g.feed(at(12, 22), ms(10), &layout), None);
        assert_eq!(
            g.feed(at(14, 24), ms(40), &layout),
            Some(TouchEv::Move(Point::new(14, 24)))
        );
        // The lift reports where the finger last was.
        assert_eq!(
            g.feed(None, ms(60), &layout),
            Some(TouchEv::Up(Point::new(14, 24)))
        );
        assert!(!g.is_down());
    }

    #[test]
    fn a_bounce_is_dropped() {
        let layout = Layout::for_rotation(0);
        let mut g = Gestures::new(ms(50));
        assert!(g.feed(at(1, 1), ms(0), &layout).is_some());
        assert!(g.feed(None, ms(5), &layout).is_some());
        assert_eq!(g.feed(at(1, 1), ms(20), &layout), None, "within debounce");
        assert!(
            g.feed(at(1, 1), ms(50), &layout).is_some(),
            "at the debounce"
        );
    }

    #[test]
    fn an_empty_report_with_no_finger_is_nothing() {
        let layout = Layout::for_rotation(0);
        let mut g = Gestures::new(ms(50));
        assert_eq!(g.feed(None, ms(0), &layout), None);
    }

    #[test]
    fn points_come_out_rotated() {
        let layout = Layout::for_rotation(90);
        let mut g = Gestures::new(ms(0));
        let t = TouchSample {
            x: 10,
            y: 20,
            pressure: 1,
            fingers: 1,
        };
        assert_eq!(
            g.feed(Some(t), ms(0), &layout),
            Some(TouchEv::Down(layout.map(t)))
        );
    }
}
