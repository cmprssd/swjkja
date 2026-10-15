//! A skeleton's animations (`animation.cfg`), read the way the single-player game
//! reads them (`code/game/NPC_stats.cpp`: `G_LoadAnimFileSet`, `G_ParseAnimationFile`),
//! and the frames a bone playing one reaches over time (ghoul2's `G2_TimingModel`).

use swjkja_fs::Vfs;

/// The animation an NPC starts in (`NPC_Begin`).
pub const STAND: &str = "BOTH_STAND1";

/// The skeleton humanoid meshes name, the one single player gives a cinematic
/// skeleton per map ([`cinematic_skeleton`]).
pub const HUMANOID: &str = "models/players/_humanoid/_humanoid";

/// The cinematic skeleton single player loads after [`HUMANOID`] on the map at `map`
/// (as "maps/academy2.bsp"), named as a mesh names its skeleton:
/// "models/players/_humanoid_academy2/_humanoid_academy2" (`G_ParseAnimFileSet`, from
/// `level.mapname` after its last folder). Its animation file's animations replace
/// [`HUMANOID`]'s of the same name and play on it.
pub fn cinematic_skeleton(map: &str) -> String {
    let map = map.replace('\\', "/");
    let file = map.rsplit('/').next().unwrap_or(&map);
    let name = file.strip_suffix(".bsp").unwrap_or(file);
    format!("models/players/_humanoid_{name}/_humanoid_{name}")
}

/// The animation file of the skeleton a mesh names (its `anim_name`, as
/// "models/players/_humanoid/_humanoid"): the skeleton's folder name, its
/// "<name>.cfg" there, or failing that its "animation.cfg". `None` if neither exists.
pub fn config_file(vfs: &Vfs, anim_name: &str) -> Option<String> {
    let folder = anim_name.rsplit_once('/').map_or(anim_name, |(dir, _)| dir);
    let skeleton = folder.rsplit('/').next().unwrap_or(folder);
    [
        format!("models/players/{skeleton}/{skeleton}.cfg"),
        format!("models/players/{skeleton}/animation.cfg"),
    ]
    .into_iter()
    .find(|f| vfs.contains(f))
}

/// An animation's line in an animation file, as `G_ParseAnimationFile` keeps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Animation {
    /// Its first frame in the skeleton.
    pub first: usize,
    /// How many frames it has (at least one).
    pub count: usize,
    /// Whether it loops (its loop frames are not -1).
    pub looping: bool,
    /// Milliseconds per frame: `ceil(1000 / fps)`, or `floor` of it, negative,
    /// when it plays backwards.
    pub frame_lerp: i32,
}

/// `animation`'s line in an animation file's `text` (lines of name, first frame,
/// frame count, loop frames, frames per second; `//` comments). The last line
/// naming it wins. `None` when no line names it or it has no frames, where the
/// original leaves the skeleton unanimated.
pub fn find(text: &str, animation: &str) -> Option<Animation> {
    let mut found = None;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("");
        let mut words = line.split_ascii_whitespace();
        if !words
            .next()
            .is_some_and(|w| w.eq_ignore_ascii_case(animation))
        {
            continue;
        }
        let mut number = || words.next().map_or(0.0, atof);
        let (first, count, loop_frames, fps) = (number(), number(), number(), number());
        // The file's fields are a short, a short and a signed char.
        let fps = if fps == 0.0 { 1.0 } else { fps };
        found = (count as u16 >= 1).then(|| Animation {
            first: first as i32 as u16 as usize,
            count: count as i32 as u16 as usize,
            looping: loop_frames as i32 as i8 != -1,
            frame_lerp: if fps < 0.0 {
                (1000.0 / fps).floor()
            } else {
                (1000.0 / fps).ceil()
            } as i32,
        });
    }
    found
}

impl Animation {
    /// The frame it starts at: its first, or its last when it plays backwards.
    pub fn start(&self) -> usize {
        if self.frame_lerp < 0 {
            self.first + self.count - 1
        } else {
            self.first
        }
    }

    /// The frames a bone playing it from its start shows `elapsed` milliseconds
    /// later (`G2_TimingModel`, as `PM_SetAnimFinal` sets it: 20 frames per second
    /// is speed 1): its frame, the next one and how far it has lerped toward that.
    /// A looping animation wraps around its frames, its last lerping into its
    /// first; one that does not stays on its last. One played backwards stays at
    /// its start (left out: no standing animation plays backwards).
    pub fn frame_at(&self, elapsed: i32) -> (usize, usize, f32) {
        if self.frame_lerp < 0 {
            return (self.start(), self.start(), 0.0);
        }
        let speed = 50.0 / self.frame_lerp as f32;
        let time = elapsed.max(0) as f32 / 50.0;
        let (first, size) = (self.first as f32, self.count as f32);
        let end = first + size;
        let last = self.first + self.count - 1;
        let mut g = first + time * speed;
        if g > end - 1.0 {
            if !self.looping {
                return (last, last, 0.0);
            }
            if g < end {
                return (g as usize, self.first, g - g.trunc());
            }
            g = end + (g - end) % size - size;
            let next = if g >= end - 1.0 {
                self.first
            } else {
                g as usize + 1
            };
            return (g as usize, next, g - g.trunc());
        }
        let frame = g as usize;
        let next = if frame + 1 > last {
            if self.looping { self.first } else { last }
        } else {
            frame + 1
        };
        (frame, next, g - g.trunc())
    }

    /// Where a bone playing it from its start for `elapsed` milliseconds blends
    /// from when another animation replaces it, as [`frame_at`](Self::frame_at)
    /// gives it: the frame it reached (`G2_Set_Bone_Anim_Index`'s `blendFrame`)
    /// lerped toward the next (`blendLerpFrame`) by one less the fraction it had
    /// gone: `G2_TransformBone` weighs the reached frame by that fraction, the
    /// next by the rest. One played backwards blends from its frame alone.
    pub fn blend_from(&self, elapsed: i32) -> (usize, usize, f32) {
        let (frame, next, lerp) = self.frame_at(elapsed);
        if self.frame_lerp < 0 {
            return (frame, frame, 0.0);
        }
        (frame, next, 1.0 - lerp)
    }
}

/// C's `atof`: the longest leading number, 0 for none.
fn atof(s: &str) -> f32 {
    (1..=s.len())
        .rev()
        .filter(|&n| s.is_char_boundary(n))
        .find_map(|n| s[..n].parse::<f32>().ok())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_starts_at_its_first_frame_the_last_line_winning() {
        let cfg = "// name first count loop fps\n\
                   FACE_ALERT 0 2 -1 1\n\
                   BOTH_STAND1 14651 2 0 20\n\
                   BOTH_WALK1 10 5 0 -20\n";
        let start = |name| find(cfg, name).map(|a| a.start());
        assert_eq!(start(STAND), Some(14651));
        // Backwards: its last frame.
        assert_eq!(start("BOTH_WALK1"), Some(14));
        assert_eq!(
            find(&format!("{cfg}both_stand1 7 1 -1 20"), STAND).map(|a| a.start()),
            Some(7)
        );
        // Missing, or listed with no frames: unanimated.
        assert_eq!(start("BOTH_RUN1"), None);
        assert_eq!(find("BOTH_STAND1 5 0 0 20", STAND), None);
    }

    #[test]
    fn a_maps_cinematic_skeleton_is_named_after_it() {
        let academy2 = "models/players/_humanoid_academy2/_humanoid_academy2";
        assert_eq!(cinematic_skeleton("maps/academy2.bsp"), academy2);
        assert_eq!(cinematic_skeleton("maps\\academy2.bsp"), academy2);
        assert_eq!(cinematic_skeleton("academy2"), academy2);
    }

    #[test]
    fn frame_lerp_rounds_away_from_zero_and_zero_fps_is_one() {
        let lerp = |line: &str| find(line, "A").unwrap().frame_lerp;
        assert_eq!(lerp("A 0 1 0 15"), 67);
        assert_eq!(lerp("A 0 1 0 -15"), -67);
        assert_eq!(lerp("A 0 1 0 0"), 1000);
        assert!(!find("A 0 1 -1 20", "A").unwrap().looping);
    }

    #[test]
    fn a_looping_animation_wraps_its_last_frame_lerping_into_its_first() {
        // Frames 100..=103 at 20 per second: one frame each 50 ms.
        let anim = find("A 100 4 0 20", "A").unwrap();
        assert_eq!(anim.frame_at(0), (100, 101, 0.0));
        assert_eq!(anim.frame_at(-30), (100, 101, 0.0));
        assert_eq!(anim.frame_at(75), (101, 102, 0.5));
        assert_eq!(anim.frame_at(160), (103, 100, 0.19999695));
        // Past the end: 200 ms is frame 104, the first again.
        assert_eq!(anim.frame_at(200), (100, 101, 0.0));
        assert_eq!(anim.frame_at(1000 + 125), (102, 103, 0.5));
    }

    #[test]
    fn a_held_animation_stays_on_its_last_frame() {
        // 10 per second: 100 ms a frame.
        let anim = find("A 100 4 -1 10", "A").unwrap();
        assert_eq!(anim.frame_at(250), (102, 103, 0.5));
        assert_eq!(anim.frame_at(301), (103, 103, 0.0));
        assert_eq!(anim.frame_at(5000), (103, 103, 0.0));
    }

    #[test]
    fn a_replaced_animation_blends_from_its_frame_weighed_the_other_way() {
        // A quarter past frame 101: a quarter on 101, three quarters on 102.
        // 10 per second: 100 ms a frame.
        let anim = find("A 100 4 0 10", "A").unwrap();
        assert_eq!(anim.blend_from(125), (101, 102, 0.75));
        // On a frame exactly: the next one alone.
        assert_eq!(anim.blend_from(100), (101, 102, 1.0));
        // Looping past its last frame: toward the first.
        assert_eq!(anim.blend_from(350), (103, 100, 0.5));
        // Held on its last frame.
        let held = find("A 100 4 -1 20", "A").unwrap();
        assert_eq!(held.blend_from(1000), (103, 103, 1.0));
    }
}
