//! The settings store and the touch mapping for a desktop.

use std::collections::BTreeMap;

use companion_core::ui::{NATIVE_H, NATIVE_W};
use companion_hal::{ConfigStore, TouchSample};
use embedded_graphics::prelude::Point;

/// A settings store that lives for the run: the command line fills it.
#[derive(Debug, Default, Clone)]
pub struct ArgStore(pub BTreeMap<String, String>);

impl ConfigStore for ArgStore {
    type Error = core::convert::Infallible;

    fn get(&self, key: &str) -> Result<Option<String>, Self::Error> {
        Ok(self.0.get(key).cloned())
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
        self.0.insert(key.into(), value.into());
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), Self::Error> {
        self.0.remove(key);
        Ok(())
    }
}

/// A point in the rotated window frame as the controller would report it.
///
/// The inverse of `Layout::map`: the application maps every sample through
/// the rotation, so a mouse point on a landscape window has to be turned
/// back into the portrait frame the controller lives in.
#[cfg_attr(not(feature = "window"), allow(dead_code))]
pub fn unmap(rotation: u16, p: Point) -> TouchSample {
    let (nw, nh) = (i32::from(NATIVE_W), i32::from(NATIVE_H));
    let (x, y) = match rotation {
        90 => (nw - 1 - p.y, p.x),
        180 => (nw - 1 - p.x, nh - 1 - p.y),
        270 => (p.y, nh - 1 - p.x),
        _ => (p.x, p.y),
    };
    TouchSample {
        x: x.clamp(0, nw - 1) as u16,
        y: y.clamp(0, nh - 1) as u16,
        pressure: 1,
        fingers: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion_core::Layout;

    #[test]
    fn unmap_inverts_map_in_every_rotation() {
        for rotation in [0, 90, 180, 270] {
            let layout = Layout::for_rotation(rotation);
            let frame = layout.frame.size;
            for (x, y) in [
                (0, 0),
                (17, 5),
                (frame.width as i32 - 1, frame.height as i32 - 1),
            ] {
                let p = Point::new(x, y);
                assert_eq!(
                    layout.map(unmap(rotation, p)),
                    p,
                    "rotation {rotation}, {p:?}"
                );
            }
        }
    }

    #[test]
    fn points_off_the_frame_are_clamped() {
        let s = unmap(0, Point::new(-5, 999));
        assert_eq!((s.x, s.y), (0, NATIVE_H - 1));
    }

    #[test]
    fn the_store_round_trips() {
        let mut s = ArgStore::default();
        assert_eq!(s.get("theme").unwrap(), None);
        s.set("theme", "night").unwrap();
        assert_eq!(s.get("theme").unwrap().as_deref(), Some("night"));
        s.remove("theme").unwrap();
        assert_eq!(s.get("theme").unwrap(), None);
    }
}
