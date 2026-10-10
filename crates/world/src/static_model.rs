//! The map's static models (`misc_model_static`), as multiplayer cgame spawns them
//! (OpenJK `codemp/cgame/cg_spawn.c`: `CG_ParseEntityFromSpawnVars` and
//! `SP_misc_model_static`). The renderer loads the models and draws them each frame
//! like `CG_DrawMiscStaticModels`.

use swjkja_bsp::Entity;

use crate::Vec3;
use crate::style::atoi;

/// Multiplayer game types, as cgame names them in a spawn's "gametype" key.
const GAMETYPE_NAMES: [&str; 10] = [
    "ffa",
    "holocron",
    "jedimaster",
    "duel",
    "powerduel",
    "single",
    "team",
    "siege",
    "ctf",
    "cty",
];
/// `GT_SINGLE_PLAYER` and `GT_TEAM`: the first game types with "notsingle" and
/// "notteam" filtering.
const GT_SINGLE_PLAYER: usize = 5;
const GT_TEAM: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct StaticModel {
    /// The model's file, as the "model" key names it.
    pub model: String,
    pub origin: Vec3,
    /// `AnglesToAxis(angles)` (forward, left, up), each axis scaled by its scale.
    pub axes: [Vec3; 3],
    /// Per-axis scale ("modelscale_vec", or "modelscale" on all three).
    pub scale: Vec3,
    /// "zoffset": raises the point the model is culled by.
    pub zoffset: f32,
}

/// `atof`: the longest leading number, 0 for none.
fn atof(s: &str) -> f32 {
    let s = s.trim_start();
    (1..=s.len())
        .rev()
        .filter(|&n| s.is_char_boundary(n))
        .find_map(|n| s[..n].parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// `CG_SpawnVector`'s `sscanf("%f %f %f")`: three numbers, or `None`.
fn vector(s: &str) -> Option<Vec3> {
    let mut it = s.split_ascii_whitespace();
    let mut v = [0.0; 3];
    for x in &mut v {
        *x = it.next()?.parse().ok()?;
    }
    Some(v)
}

/// `AnglesToAxis`: forward, left and up for pitch, yaw, roll in degrees.
pub fn angles_to_axis(angles: Vec3) -> [Vec3; 3] {
    let rad = |deg: f32| deg * (std::f32::consts::PI * 2.0 / 360.0);
    let (sp, cp) = rad(angles[0]).sin_cos();
    let (sy, cy) = rad(angles[1]).sin_cos();
    let (sr, cr) = rad(angles[2]).sin_cos();
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    [
        [cp * cy, cp * sy, -sp],
        right.map(|x| -x),
        [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp],
    ]
}

/// `CG_ParseEntityFromSpawnVars`'s filters: whether `e` spawns in `gametype` (an
/// index into the game type names, 0 for free-for-all).
fn spawns_in(e: &Entity, gametype: usize) -> bool {
    let flag = |key| e.get(key).map_or(0, atoi) != 0;
    if gametype == GT_SINGLE_PLAYER && flag("notsingle") {
        return false;
    }
    if if gametype >= GT_TEAM {
        flag("notteam")
    } else {
        flag("notfree")
    } {
        return false;
    }
    match e.get("gametype") {
        Some(names) => GAMETYPE_NAMES
            .get(gametype)
            .is_none_or(|name| names.contains(name)),
        None => true,
    }
}

/// The static models `entities` spawn in `gametype` (0 for free-for-all), in spawn
/// order. One without a model stops the original's map load; here it is skipped.
pub fn static_models(entities: &[Entity], gametype: usize) -> Vec<StaticModel> {
    let mut out = Vec::new();
    for e in entities {
        if !e
            .get("classname")
            .is_some_and(|c| c.eq_ignore_ascii_case("misc_model_static"))
            || !spawns_in(e, gametype)
        {
            continue;
        }
        let Some(model) = e.get("model").filter(|m| !m.is_empty()) else {
            continue;
        };
        // A key that is present but malformed reads as zero, like a missing one.
        let origin = e.get("origin").and_then(vector).unwrap_or_default();
        let zoffset = e.get("zoffset").map_or(0.0, atof);
        let angles = match e.get("angles").and_then(vector) {
            Some(a) => a,
            None => [0.0, e.get("angle").map_or(0.0, atof), 0.0],
        };
        // A malformed "modelscale_vec" reads as zero unless "modelscale" is there.
        let modelscale = e.get("modelscale").map(|s| [atof(s); 3]);
        let scale = match e.get("modelscale_vec") {
            Some(v) => vector(v).or(modelscale).unwrap_or([0.0; 3]),
            None => modelscale.unwrap_or([1.0; 3]),
        };
        let axis = angles_to_axis(angles);
        out.push(StaticModel {
            model: model.to_owned(),
            origin,
            axes: [0, 1, 2].map(|i| axis[i].map(|x| x * scale[i])),
            scale,
            zoffset,
        });
    }
    out
}
