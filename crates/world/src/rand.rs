//! The game module's random numbers (`shared/qcommon/q_math.c`: `irand` and
//! `flrand`, which `Q_irand` and `Q_flrand` call). Single player never seeds them
//! (`Rand_Init` has no caller), so every run draws the same sequence from the
//! module's first state: what the
//! game picks "at random" as a map loads is the same each time, given every draw
//! before it. The game and cgame share the one module, and so the sequence.

/// The generator's state (`holdrand`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rand(u32);

impl Default for Rand {
    /// The state the module loads with.
    fn default() -> Self {
        Self(0x89ab_cdef)
    }
}

impl Rand {
    /// `Q_irand`: an integer from `min` to `max`, both included.
    pub fn irand(&mut self, min: i32, max: i32) -> i32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        (((self.0 >> 17) as i32 * (max + 1 - min)) >> 15) + min
    }

    /// `Q_flrand`: a float from `min` up to `max` (not included).
    pub fn flrand(&mut self, min: f32, max: f32) -> f32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.0 >> 17) as f32 * (max - min) / 32768.0 + min
    }

    /// `Q_irand(0, 1)` taken as a condition: `heads` if it draws 1.
    pub fn flip<T>(&mut self, heads: T, tails: T) -> T {
        if self.irand(0, 1) != 0 { heads } else { tails }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_the_unseeded_sequence() {
        // The first draws of the academies' random Jedi (Q_irand(0, 11)), logged
        // by the reference capture with the generator's state after each.
        let mut r = Rand::default();
        assert_eq!(r.irand(0, 11), 5);
        assert_eq!(r, Rand(0x6fa3_b0f6));
        assert_eq!(r.irand(0, 11), 2);
        assert_eq!(r, Rand(0x3f1e_e3e1));
        assert_eq!(
            [1, 8, 1, 10, 7, 10].map(|_| r.irand(0, 11)),
            [1, 8, 1, 10, 7, 10]
        );
    }

    #[test]
    fn draws_floats_from_the_same_sequence() {
        // t2_dpred's first gun rack's draws (Q_flrand(-1, 1)), logged by the
        // reference capture after the map's seventh draw.
        let mut r = Rand(0x9b0d_11e4);
        let draws = [0.729797, -0.989746, -0.704834, 0.658997, 0.620544];
        for want in draws {
            let got = r.flrand(-1.0, 1.0);
            assert!((got - want).abs() < 1e-6, "{got} != {want}");
        }
        assert_eq!(r, Rand(0xcf6f_210b));
    }
}
