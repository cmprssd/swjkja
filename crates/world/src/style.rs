//! Light styles: the brightness patterns of flickering and pulsing lights, as
//! multiplayer sets them up and animates them (OpenJK `codemp`).
//!
//! The server fills the style config strings: `SP_worldspawn` sets styles 0-31 to the
//! built-in patterns, which the map can override with `ls_<n>r`/`g`/`b` keys; each
//! switchable `light` entity (`SP_light`) then sets its style to on (`switch_style`'s
//! pattern, or "z") or off (`style_off`'s, or "a"). The client converts each letter
//! to a brightness (`CG_SetLightstyle`) and steps through the pattern every 50 ms
//! (`CG_RunLightStyles`).

use swjkja_bsp::Entity;

pub const MAX_LIGHT_STYLES: usize = 64;
const NUM_STYLES: usize = 32;

/// `defaultStyles` in `g_spawn.c`: (red, green, blue) patterns for styles 0-13;
/// 14-31 are empty.
const DEFAULT_STYLES: [[&str; 3]; 14] = [
    ["z", "z", "z"],
    ["mmnmmommommnonmmonqnmmo"; 3],
    ["abcdefghijklmnopqrstuvwxyzyxwvutsrqponmlkjihgfedcb"; 3],
    ["mmmmmaaaaammmmmaaaaaabcdefgabcdefg"; 3],
    ["mamamamamama"; 3],
    ["jklmnopqrstuvwxyzyxwvutsrqponmlkj"; 3],
    ["nmonqnmomnmomomno"; 3],
    ["mmmaaaabcdefgmmmmaaaammmaamm"; 3],
    ["mmmaaammmaaammmabcdefaaaammmmabcdefmmmaaaa"; 3],
    ["aaaaaaaazzzzzzzz"; 3],
    ["mmamammmmammamamaaamammma"; 3],
    ["abcdefghijklmnopqrrqponmlkjihgfedcba"; 3],
    ["mkigegik"; 3],
    [
        "abcdefghijklmqrstuvwxyz",
        "zyxwvutsrqmlkjihgfedcba",
        "aammbbzzccllcckkffyyggp",
    ],
];

/// The 64 styles' (red, green, blue) pattern strings after the map has spawned.
#[derive(Debug, Clone, PartialEq)]
pub struct LightStyles {
    pub patterns: Vec<[String; 3]>,
}

fn atoi(s: &str) -> i64 {
    let s = s.trim_start();
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    sign * digits[..end].parse::<i64>().unwrap_or(0)
}

impl LightStyles {
    /// The config strings as the server sets them while spawning `entities` (the
    /// first one is the worldspawn).
    pub fn spawn(entities: &[Entity]) -> Self {
        let mut patterns = vec![[String::new(), String::new(), String::new()]; MAX_LIGHT_STYLES];
        let world = entities.first();
        for (i, p) in patterns.iter_mut().enumerate().take(NUM_STYLES) {
            for (c, channel) in ["r", "g", "b"].iter().enumerate() {
                let default = DEFAULT_STYLES.get(i).map_or("", |d| d[c]);
                // Style 0 is always the default; the others take the map's override.
                let key = format!("ls_{i}{channel}");
                p[c] = match world.and_then(|w| w.get(&key)) {
                    Some(v) if i > 0 => v.to_string(),
                    _ => default.to_string(),
                };
            }
        }
        for e in entities.iter().skip(1) {
            if e.get("classname") != Some("light") || e.get("targetname").is_none() {
                continue;
            }
            let int = |k: &str| e.get(k).map_or(0, atoi);
            let style = int("style");
            if !(0..MAX_LIGHT_STYLES as i64).contains(&style) {
                continue;
            }
            let on = int("spawnflags") & 4 == 0;
            let source = if on {
                int("switch_style")
            } else {
                int("style_off")
            };
            let fallback = if on { "z" } else { "a" };
            patterns[style as usize] =
                if source != 0 && (0..MAX_LIGHT_STYLES as i64).contains(&source) {
                    // trap->GetConfigstring into a 32-byte buffer keeps 31 characters.
                    patterns[source as usize]
                        .clone()
                        .map(|s| s.chars().take(31).collect())
                } else {
                    [
                        fallback.to_string(),
                        fallback.to_string(),
                        fallback.to_string(),
                    ]
                };
        }
        Self { patterns }
    }

    /// Every style's colour (RGBA bytes) at `time_ms` of game time.
    pub fn colors(&self, time_ms: i64) -> [[u8; 4]; MAX_LIGHT_STYLES] {
        let ofs = time_ms / 50;
        let mut out = [[255u8; 4]; MAX_LIGHT_STYLES];
        for (style, p) in self.patterns.iter().enumerate() {
            // cl_lightstyle's length is the last channel's (the server keeps the
            // three equal).
            let length = p[2].len() as i64;
            if length == 0 {
                continue;
            }
            let k = if length == 1 {
                0
            } else {
                ofs.rem_euclid(length)
            } as usize;
            for c in 0..3 {
                // A channel shorter than the length reads what CG_SetLightstyle left
                // there: zero.
                out[style][c] = p[c].as_bytes().get(k).map_or(0, |&b| brightness(b));
            }
        }
        out
    }
}

/// `CG_SetLightstyle`: 'a' is off (0), 'z' full (255).
fn brightness(letter: u8) -> u8 {
    (f32::from(letter.wrapping_sub(b'a')) / f32::from(b'z' - b'a') * 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(pairs: &[(&str, &str)]) -> Entity {
        Entity {
            pairs: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn letters_map_to_brightness() {
        assert_eq!(brightness(b'a'), 0);
        assert_eq!(brightness(b'm'), 122); // 12 / 25 * 255 = 122.4
        assert_eq!(brightness(b'z'), 255);
    }

    #[test]
    fn styles_step_every_50_ms() {
        let styles = LightStyles::spawn(&[entity(&[("classname", "worldspawn")])]);
        // Style 4, fast strobe "mamamamamama": m at 0-49 ms, a at 50-99 ms.
        assert_eq!(styles.colors(49)[4], [122, 122, 122, 255]);
        assert_eq!(styles.colors(50)[4], [0, 0, 0, 255]);
        // Style 0 is steady full light; unused styles are white.
        assert_eq!(styles.colors(12345)[0], [255; 4]);
        assert_eq!(styles.colors(12345)[20], [255; 4]);
        // Style 13 differs per channel: at 0 ms 'a', 'z', 'a'.
        assert_eq!(styles.colors(0)[13], [0, 255, 0, 255]);
    }

    #[test]
    fn maps_override_styles_and_switchable_lights_set_theirs() {
        let styles = LightStyles::spawn(&[
            entity(&[
                ("classname", "worldspawn"),
                ("ls_1r", "a"),
                ("ls_1g", "z"),
                ("ls_1b", "a"),
                ("ls_0r", "a"),
            ]),
            // Switched on with style 2's pattern.
            entity(&[
                ("classname", "light"),
                ("targetname", "t1"),
                ("style", "32"),
                ("switch_style", "2"),
            ]),
            // Starts off (spawnflags 4) without an off style: "a".
            entity(&[
                ("classname", "light"),
                ("targetname", "t2"),
                ("style", "33"),
                ("spawnflags", "4"),
            ]),
            // Without a targetname the light is not spawned.
            entity(&[("classname", "light"), ("style", "34"), ("spawnflags", "4")]),
        ]);
        assert_eq!(styles.colors(0)[1], [0, 255, 0, 255]);
        // Style 0 ignores the map.
        assert_eq!(styles.colors(0)[0], [255; 4]);
        // Copied through a 32-byte config string buffer: 31 letters of style 2.
        assert_eq!(styles.patterns[32][0], "abcdefghijklmnopqrstuvwxyzyxwvu");
        assert_eq!(styles.colors(50)[32], [10, 10, 10, 255]); // 'b': 1 / 25 * 255
        assert_eq!(styles.colors(0)[33], [0, 0, 0, 255]);
        assert_eq!(styles.colors(0)[34], [255; 4]);
    }
}
