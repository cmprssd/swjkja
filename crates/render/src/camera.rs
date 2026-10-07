//! The view: the original's camera conventions (OpenJK `q_math.c` `AngleVectors`,
//! cgame `CalcFov`) mapped to Vulkan clip space with reversed, infinite depth.
//!
//! Map space: x forward (east), y left (north), z up. Angles are pitch (positive looks
//! down), yaw (positive turns left) and roll, in degrees.

pub type Vec3 = [f32; 3];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub origin: Vec3,
    /// Pitch, yaw, roll in degrees.
    pub angles: Vec3,
    /// Horizontal field of view in degrees (`cg_fov`, 80 by default); the vertical one
    /// follows from the aspect ratio, as in the original.
    pub fov_x: f32,
    pub width: u32,
    pub height: u32,
    /// `r_znear`, 4 by default.
    pub z_near: f32,
}

impl Camera {
    pub fn new(origin: Vec3, angles: Vec3, width: u32, height: u32) -> Self {
        Self {
            origin,
            angles,
            fov_x: 80.0,
            width,
            height,
            z_near: 4.0,
        }
    }

    /// `AngleVectors`: forward, right, up.
    pub fn axes(&self) -> [Vec3; 3] {
        let rad = |deg: f32| deg * (std::f32::consts::PI * 2.0 / 360.0);
        let (sp, cp) = rad(self.angles[0]).sin_cos();
        let (sy, cy) = rad(self.angles[1]).sin_cos();
        let (sr, cr) = rad(self.angles[2]).sin_cos();
        [
            [cp * cy, cp * sy, -sp],
            [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp],
            [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp],
        ]
    }

    /// tan of half the horizontal and vertical fields of view. The vertical one comes
    /// from cgame's `CalcFov`: `x = width / tan(fov_x / 2)`, `fov_y = 2 atan(height / x)`.
    pub fn tan_half_fov(&self) -> (f32, f32) {
        let tx = (self.fov_x / 360.0 * std::f32::consts::PI).tan();
        (tx, tx * self.height as f32 / self.width as f32)
    }

    /// The view-projection matrix, column-major (as WGSL `mat4x4` takes it): clip x
    /// right, y down, depth `z_near / distance` (1 at the near plane, 0 at infinity).
    pub fn view_proj(&self) -> [[f32; 4]; 4] {
        let [f, r, u] = self.axes();
        let (tx, ty) = self.tan_half_fov();
        let o = self.origin;
        let d = |a: Vec3| a[0] * o[0] + a[1] * o[1] + a[2] * o[2];
        // Rows of the matrix acting on (x, y, z, 1).
        let rows = [
            [r[0] / tx, r[1] / tx, r[2] / tx, -d(r) / tx],
            [-u[0] / ty, -u[1] / ty, -u[2] / ty, d(u) / ty],
            [0.0, 0.0, 0.0, self.z_near],
            [f[0], f[1], f[2], -d(f)],
        ];
        [0, 1, 2, 3].map(|c| [rows[0][c], rows[1][c], rows[2][c], rows[3][c]])
    }

    /// Where a map point lands: (x, y) in pixels from the top-left, and depth.
    pub fn project(&self, p: Vec3) -> Option<(f32, f32, f32)> {
        let m = self.view_proj();
        let clip = [0, 1, 2, 3]
            .map(|row| m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row]);
        if clip[3] <= 0.0 {
            return None;
        }
        let (x, y) = (clip[0] / clip[3], clip[1] / clip[3]);
        Some((
            (x + 1.0) * 0.5 * self.width as f32,
            (y + 1.0) * 0.5 * self.height as f32,
            clip[2] / clip[3],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn axes_follow_angle_vectors() {
        let c = Camera::new([0.0; 3], [0.0, 90.0, 0.0], 640, 480);
        let [f, r, u] = c.axes();
        // Yaw 90 looks along +y (north); its right-hand side is +x (east).
        assert!(close(f[0], 0.0) && close(f[1], 1.0) && close(f[2], 0.0));
        assert!(close(r[0], 1.0) && close(r[1], 0.0));
        assert!(close(u[2], 1.0));
        // Positive pitch looks down.
        let [f, ..] = Camera::new([0.0; 3], [30.0, 0.0, 0.0], 640, 480).axes();
        assert!(f[2] < 0.0 && close(f[2], -0.5));
    }

    #[test]
    fn projection_puts_points_where_the_original_would() {
        let c = Camera::new([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 1280, 720);
        // Straight ahead: the centre, depth near / distance.
        let (x, y, z) = c.project([100.0, 0.0, 0.0]).unwrap();
        assert!(close(x, 640.0) && close(y, 360.0) && close(z, 0.04));
        // At the edge of the 80 degree horizontal FOV, to the left (+y): x = 0.
        let t = (40.0f32).to_radians().tan();
        let (x, y, _) = c.project([100.0, 100.0 * t, 0.0]).unwrap();
        assert!(close(x, 0.0) && close(y, 360.0));
        // Up (+z) is towards the top of the image; the vertical FOV follows the aspect.
        let (_, y, _) = c.project([100.0, 0.0, 100.0 * t * 720.0 / 1280.0]).unwrap();
        assert!(close(y, 0.0));
        // Behind the camera: not on screen.
        assert!(c.project([-10.0, 0.0, 0.0]).is_none());
    }
}
