//! Where an NPC looks: single player's cgame turns the head, neck and upper spine of
//! an NPC with a look target (a script's `SET_LOOK_TARGET`) toward the target's eyes
//! (`code/cgame/cg_players.cpp`: `CG_CheckLookTarget`, `CG_UpdateLookAngles`,
//! `CG_G2ClientNeckAngles`).
//!
//! Each frame the original measures the angles to the target from the NPC's eyes as
//! last drawn (the "*head_eyes" tag, already turned by the look so far), clamps them,
//! and moves the look a part of the way there (15% scaled by the frame's
//! interpolation). The look then settles where the angles measured from the turned
//! eyes equal the look itself: about half the way to the target, since the three
//! bones' shares add up to the whole turn. That settled look is what is drawn here,
//! from the moment the script sets the target (the original takes about a second
//! to get there, at a speed that depends on its frame rate).

use swjkja_model::gla::{Gla, Mat34};
use swjkja_world::static_model::angles_to_axis;

pub type Vec3 = [f32; 3];

/// `CG_UpdateLookAngles`' clamp on the angles to the target (pitch, yaw, roll).
const LOOK_MIN: Vec3 = [-50.0, -70.0, -30.0];
const LOOK_MAX: Vec3 = [50.0, 70.0, 30.0];
/// `CG_G2PlayerAngles`' clamp on the look before the bones share it.
const HEAD_MIN: Vec3 = [-25.0, -55.0, -10.0];
const HEAD_MAX: Vec3 = [50.0, 50.0, 10.0];
/// Each bone's share of the look (pitch, yaw, roll), `CG_G2ClientNeckAngles` for a
/// humanoid whose spine is not turned.
const THORACIC_SHARE: Vec3 = [0.4, 0.1, 0.1];
const CERVICAL_SHARE: Vec3 = [0.2, 0.3, 0.3];
const CRANIUM_SHARE: Vec3 = [0.4, 0.6, 0.6];

/// How many steps toward the settled look at most, and the change (degrees) small
/// enough to stop at.
const STEPS: usize = 32;
const SETTLED: f32 = 0.001;

/// `AngleNormalize180`: `angle` within -180..180, through the 16-bit steps of
/// `AngleNormalize360`.
pub fn normalize180(angle: f32) -> f32 {
    let steps = (angle * (65536.0 / 360.0)) as i32 & 65535;
    let a = (360.0 / 65536.0) * steps as f32;
    if a > 180.0 { a - 360.0 } else { a }
}

/// `AngleSubtract`: `a - b` within -180..180.
fn subtract(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

/// `vectoangles`: the pitch and yaw `v` points along (roll 0).
pub fn vectoangles(v: Vec3) -> Vec3 {
    let (yaw, pitch) = if v[0] == 0.0 && v[1] == 0.0 {
        (0.0, if v[2] > 0.0 { 90.0 } else { 270.0 })
    } else {
        let mut yaw = if v[0] != 0.0 {
            v[1].atan2(v[0]).to_degrees()
        } else if v[1] > 0.0 {
            90.0
        } else {
            270.0
        };
        if yaw < 0.0 {
            yaw += 360.0;
        }
        let forward = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let mut pitch = v[2].atan2(forward).to_degrees();
        if pitch < 0.0 {
            pitch += 360.0;
        }
        (yaw, pitch)
    };
    [-pitch, yaw, 0.0]
}

fn clamp(v: Vec3, min: Vec3, max: Vec3) -> Vec3 {
    [0, 1, 2].map(|i| v[i].clamp(min[i], max[i]))
}

/// The rotation `BG_G2SetBoneAngles` gives a humanoid bone for `angles` (pitch,
/// yaw, roll) with `POSITIVE_X, NEGATIVE_Y, NEGATIVE_Z` (`G2_Generate_Matrix`: yaw
/// about the bone's x, pitch about its y, roll about its z, which `AnglesToAxis`
/// turns as yaw, pitch and roll), its columns the turned axes (`Create_Matrix`).
pub fn bone_rotation(angles: Vec3) -> Mat34 {
    let axis = angles_to_axis([angles[0], angles[2], angles[1]]);
    std::array::from_fn(|r| [axis[0][r], axis[1][r], axis[2][r], 0.0])
}

/// The bones a humanoid's look turns: "thoracic", "cervical" and "cranium".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    pub thoracic: usize,
    pub cervical: usize,
    pub cranium: usize,
}

impl Head {
    /// The three bones of `gla`, if it has them all.
    pub fn find(gla: &Gla) -> Option<Self> {
        Some(Self {
            thoracic: gla.find_bone("thoracic")?,
            cervical: gla.find_bone("cervical")?,
            cranium: gla.find_bone("cranium")?,
        })
    }

    /// The bones' overrides for look `look` (pitch, yaw, roll, relative to where
    /// the eyes face unturned): clamped, then shared out among the bones.
    pub fn overrides(&self, gla: &Gla, look: Vec3) -> [(usize, Mat34); 3] {
        let look = clamp(look, HEAD_MIN, HEAD_MAX);
        [
            (self.thoracic, THORACIC_SHARE),
            (self.cervical, CERVICAL_SHARE),
            (self.cranium, CRANIUM_SHARE),
        ]
        .map(|(bone, share)| {
            let angles = [0, 1, 2].map(|i| look[i] * share[i]);
            (bone, gla.angles_override(bone, &bone_rotation(angles)))
        })
    }
}

/// The look an NPC settles at when its eyes, turned by look `l`, are at `eyes(l)`
/// (where they are and the angles they face) and the target's eyes at `target`:
/// the look equal to the clamped angles from its eyes to the target, less the
/// angles its eyes face (`CG_CheckLookTarget`, `CG_UpdateLookAngles`). Found by
/// stepping half the way each time, from no look.
pub fn settle(target: Vec3, eyes: impl Fn(Vec3) -> (Vec3, Vec3)) -> Vec3 {
    let mut look = [0.0; 3];
    for _ in 0..STEPS {
        let (at, facing) = eyes(look);
        let to = vectoangles([0, 1, 2].map(|i| target[i] - at[i]));
        let wanted = [0, 1, 2].map(|i| subtract(normalize180(to[i]), normalize180(facing[i])));
        let wanted = clamp(wanted, LOOK_MIN, LOOK_MAX);
        let next = [0, 1, 2].map(|i| look[i] + 0.5 * subtract(wanted[i], look[i]));
        let change = (0..3)
            .map(|i| (next[i] - look[i]).abs())
            .fold(0.0, f32::max);
        look = next;
        if change < SETTLED {
            break;
        }
    }
    look
}

/// The angles an entity's eyes face from a bolt matrix (its columns the tag's axes,
/// in world space): along the tag's -y (`G2API_GiveMeVectorFromMatrix`'s
/// `NEGATIVE_Y`).
pub fn eye_angles(bolt: &Mat34) -> Vec3 {
    vectoangles([0, 1, 2].map(|r| -bolt[r][1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 0.01)
    }

    #[test]
    fn vectors_give_their_pitch_and_yaw() {
        assert!(close(vectoangles([1.0, 0.0, 0.0]), [0.0, 0.0, 0.0]));
        assert!(close(vectoangles([0.0, 1.0, 0.0]), [0.0, 90.0, 0.0]));
        assert!(close(vectoangles([-1.0, -1.0, 0.0]), [0.0, 225.0, 0.0]));
        // Up is pitch -90; down, -270 (left to AngleNormalize180).
        assert!(close(vectoangles([1.0, 0.0, 1.0]), [-45.0, 0.0, 0.0]));
        assert!(close(vectoangles([0.0, 0.0, -1.0]), [-270.0, 0.0, 0.0]));
        assert_eq!(normalize180(-270.0), 90.0);
        assert_eq!(normalize180(225.0), -135.0);
    }

    #[test]
    fn a_bone_rotation_yaws_about_its_x_and_pitches_about_its_y() {
        // Yaw 90 turns about the bone's x: y to z.
        let r = bone_rotation([0.0, 90.0, 0.0]);
        let col = |c: usize| [0, 1, 2].map(|i| r[i][c]);
        assert!(close(col(0), [1.0, 0.0, 0.0]), "{r:?}");
        assert!(close(col(1), [0.0, 0.0, 1.0]), "{r:?}");
        // Pitch 90 turns about its y: x to -z.
        let r = bone_rotation([90.0, 0.0, 0.0]);
        let col = |c: usize| [0, 1, 2].map(|i| r[i][c]);
        assert!(close(col(0), [0.0, 0.0, -1.0]), "{r:?}");
    }

    #[test]
    fn a_look_settles_half_way_to_its_target() {
        // Eyes at the origin that turn with the look, facing yaw 10 unturned; the
        // target 60 degrees left of x: the look settles at (60 - 10) / 2.
        let target = [0.5, 3.0f32.sqrt() / 2.0, 0.0];
        let eyes = |l: Vec3| ([0.0; 3], [l[0], 10.0 + l[1], 0.0]);
        let look = settle(target, eyes);
        assert!(close(look, [0.0, 25.0, 0.0]), "{look:?}");
        // Eyes that do not turn (a head clamped still): the look runs to the whole
        // angle, clamped to 70.
        let behind = [-1.0, 0.1, 0.0];
        let look = settle(behind, |_| ([0.0; 3], [0.0; 3]));
        assert!(close(look, [0.0, 70.0, 0.0]), "{look:?}");
        // Above: pitch is negative up.
        let look = settle([1.0, 0.0, 1.0], |l| ([0.0; 3], l));
        assert!(close(look, [-22.5, 0.0, 0.0]), "{look:?}");
    }

    #[test]
    fn eyes_face_along_their_tags_minus_y() {
        // A tag whose -y runs along world +x.
        let bolt = [
            [0.0, -1.0, 0.0, 5.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        assert!(close(eye_angles(&bolt), [0.0, 0.0, 0.0]));
    }
}
