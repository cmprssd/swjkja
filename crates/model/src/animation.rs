//! A skeleton's animations (`animation.cfg`), read the way the single-player game
//! reads them (`code/game/NPC_stats.cpp`: `G_LoadAnimFileSet`, `G_ParseAnimationFile`),
//! for the frame an animation starts at.

use swjkja_fs::Vfs;

/// The animation an NPC starts in (`NPC_Begin`).
pub const STAND: &str = "BOTH_STAND1";

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

/// The frame `animation` starts at in an animation file's `text` (lines of name,
/// first frame, frame count, loop frames, frames per second; `//` comments): its
/// first frame, or its last when it plays backwards (negative frames per second).
/// The last line naming it wins. `None` when no line names it or it has no frames,
/// where the original leaves the skeleton unanimated.
pub fn start_frame(text: &str, animation: &str) -> Option<usize> {
    let mut start = None;
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
        let (first, count, _loop, fps) = (number(), number(), number(), number());
        start = (count >= 1.0).then(|| {
            let first = first.max(0.0) as usize;
            if fps < 0.0 {
                first + count as usize - 1
            } else {
                first
            }
        });
    }
    start
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
        assert_eq!(start_frame(cfg, STAND), Some(14651));
        // Backwards: its last frame.
        assert_eq!(start_frame(cfg, "BOTH_WALK1"), Some(14));
        assert_eq!(
            start_frame(&format!("{cfg}both_stand1 7 1 -1 20"), STAND),
            Some(7)
        );
        // Missing, or listed with no frames: unanimated.
        assert_eq!(start_frame(cfg, "BOTH_RUN1"), None);
        assert_eq!(start_frame("BOTH_STAND1 5 0 0 20", STAND), None);
    }
}
