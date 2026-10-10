//! Free-flying camera for the map viewer, moving like the original's noclip:
//! forward follows the view (pitch included), strafing stays level, up and down are
//! world up and down.

use swjkja_render::Camera;

/// Units per second (the original's `g_speed` default, 250) and the run multiplier.
pub const SPEED: f32 = 250.0;
pub const FAST: f32 = 3.0;
/// Degrees per mouse count: `m_yaw`/`m_pitch` 0.022 times the default `sensitivity` 5.
pub const DEGREES_PER_COUNT: f32 = 0.11;

/// Which movement keys are held.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub fast: bool,
}

/// Moves the camera for `dt` seconds of held keys.
pub fn fly(camera: &mut Camera, keys: Keys, dt: f32) {
    let axis = |plus: bool, minus: bool| f32::from(u8::from(plus)) - f32::from(u8::from(minus));
    let (f, r, u) = (
        axis(keys.forward, keys.back),
        axis(keys.right, keys.left),
        axis(keys.up, keys.down),
    );
    let [forward, right, _] = camera.axes();
    let mut v = [0, 1, 2].map(|k| f * forward[k] + r * right[k]);
    v[2] += u;
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len == 0.0 {
        return;
    }
    let step = SPEED * if keys.fast { FAST } else { 1.0 } * dt / len;
    for (o, v) in camera.origin.iter_mut().zip(v) {
        *o += v * step;
    }
}

/// Turns the camera by a mouse movement in counts (x right, y down): right turns
/// right, down looks down; pitch stops at straight up and down.
pub fn look(camera: &mut Camera, dx: f64, dy: f64) {
    let [pitch, yaw, _] = &mut camera.angles;
    *yaw = (*yaw - dx as f32 * DEGREES_PER_COUNT).rem_euclid(360.0);
    *pitch = (*pitch + dy as f32 * DEGREES_PER_COUNT).clamp(-89.0, 89.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(pitch: f32, yaw: f32) -> Camera {
        Camera::new([0.0; 3], [pitch, yaw, 0.0], 1280, 720)
    }

    fn rounded(v: [f32; 3]) -> [f32; 3] {
        v.map(|x| (x * 100.0).round() / 100.0)
    }

    #[test]
    fn moves_along_the_view() {
        // Yaw 90 faces +y (map north); one second forward is 250 units.
        let mut c = camera(0.0, 90.0);
        let forward = Keys {
            forward: true,
            ..Keys::default()
        };
        fly(&mut c, forward, 1.0);
        assert_eq!(rounded(c.origin), [0.0, 250.0, 0.0]);
        // Strafing right from facing +y goes +x.
        let mut c = camera(0.0, 90.0);
        let right = Keys {
            right: true,
            ..Keys::default()
        };
        fly(&mut c, right, 1.0);
        assert_eq!(rounded(c.origin), [250.0, 0.0, 0.0]);
        // Looking straight down, forward goes down.
        let mut c = camera(90.0, 0.0);
        fly(&mut c, forward, 0.5);
        assert_eq!(rounded(c.origin), [0.0, 0.0, -125.0]);
    }

    #[test]
    fn diagonals_and_running_keep_the_speed() {
        let mut c = camera(0.0, 0.0);
        let keys = Keys {
            forward: true,
            up: true,
            fast: true,
            ..Keys::default()
        };
        fly(&mut c, keys, 1.0);
        let d = 750.0 / 2f32.sqrt();
        assert_eq!(rounded(c.origin), rounded([d, 0.0, d]));
        // Opposite keys cancel.
        let mut c = camera(0.0, 0.0);
        let keys = Keys {
            forward: true,
            back: true,
            ..Keys::default()
        };
        fly(&mut c, keys, 1.0);
        assert_eq!(c.origin, [0.0; 3]);
    }

    #[test]
    fn mouse_turns_and_stops_at_vertical() {
        let mut c = camera(0.0, 10.0);
        look(&mut c, 100.0, 100.0);
        assert_eq!(rounded(c.angles), [11.0, 359.0, 0.0]);
        look(&mut c, 0.0, 10_000.0);
        assert_eq!(c.angles[0], 89.0);
        look(&mut c, 0.0, -20_000.0);
        assert_eq!(c.angles[0], -89.0);
    }
}
