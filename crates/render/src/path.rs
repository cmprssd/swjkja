//! Camera paths: the views and game times of recorded frames, with a timestamp for
//! each, one frame per line as `usec time_ms x y z pitch yaw roll fov_x`. The
//! reference engine writes them (`cg_swjkjaPath`, bench/path-reference.sh), and the
//! benchmark and the map viewer replay them and write their own timestamps.

use crate::Camera;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathFrame {
    /// When the frame was drawn, in µs from any start; 0 when not timed.
    pub usec: i64,
    /// Game time (shader animation, light styles), in ms.
    pub time_ms: i32,
    pub origin: [f32; 3],
    /// Pitch, yaw, roll in degrees.
    pub angles: [f32; 3],
    pub fov_x: f32,
}

impl PathFrame {
    /// The frame's view at a size in pixels.
    pub fn camera(&self, width: u32, height: u32) -> Camera {
        let mut camera = Camera::new(self.origin, self.angles, width, height);
        camera.fov_x = self.fov_x;
        camera
    }
}

pub fn parse(text: &str) -> Result<Vec<PathFrame>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|(n, l)| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let bad = || format!("line {}: expected 9 numbers: {l}", n + 1);
            if f.len() != 9 {
                return Err(bad());
            }
            let num = |i: usize| f[i].parse::<f32>().map_err(|_| bad());
            Ok(PathFrame {
                usec: f[0].parse().map_err(|_| bad())?,
                time_ms: f[1].parse().map_err(|_| bad())?,
                origin: [num(2)?, num(3)?, num(4)?],
                angles: [num(5)?, num(6)?, num(7)?],
                fov_x: num(8)?,
            })
        })
        .collect()
}

/// The path with each frame's timestamp replaced by `usec[i]`, in the same format.
pub fn write(path: &[PathFrame], usec: &[i64]) -> String {
    let mut text = String::from("# usec time_ms x y z pitch yaw roll fov_x\n");
    for (f, us) in path.iter().zip(usec) {
        let ([x, y, z], [p, w, r]) = (f.origin, f.angles);
        text += &format!("{us} {} {x} {y} {z} {p} {w} {r} {}\n", f.time_ms, f.fov_x);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recorded_frames() {
        let text = "# usec time_ms x y z pitch yaw roll fov_x\n\
                    1000 5050 -1.5 2 3.25 10 -90.5 0 80\n\n\
                    3500 5100 0 0 0 0 0 0 80.000\n";
        let path = parse(text).unwrap();
        assert_eq!(
            path[0],
            PathFrame {
                usec: 1000,
                time_ms: 5050,
                origin: [-1.5, 2.0, 3.25],
                angles: [10.0, -90.5, 0.0],
                fov_x: 80.0,
            }
        );
        assert_eq!(path.len(), 2);
        assert_eq!(
            write(&path, &[7, 9]),
            "# usec time_ms x y z pitch yaw roll fov_x\n\
             7 5050 -1.5 2 3.25 10 -90.5 0 80\n\
             9 5100 0 0 0 0 0 0 80\n"
        );
        assert_eq!(parse(&write(&path, &[1000, 3500])).unwrap(), path);
    }

    #[test]
    fn rejects_short_lines() {
        assert_eq!(
            parse("1 2 3\n"),
            Err("line 1: expected 9 numbers: 1 2 3".to_string())
        );
    }
}
