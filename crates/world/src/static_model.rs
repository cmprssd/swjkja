//! The map's static models (`misc_model_static`), as the game spawns them: in
//! multiplayer, cgame (OpenJK `codemp/cgame/cg_spawn.c`: `CG_ParseEntityFromSpawnVars`
//! and `SP_misc_model_static`); in single player, the game module (`code/game/g_spawn.cpp`:
//! `G_SpawnGEntityFromSpawnVars`; `g_misc.cpp`: `SP_misc_model_static`, then cgame's
//! `CG_CreateMiscEntFromGent`). The renderer loads the models and draws them each frame
//! like `CG_DrawMiscStaticModels` (multiplayer) or `CG_DrawMiscEnts` (single player).

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

/// Which game's spawn rules a map follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rules {
    /// `gametype` indexes the game type names (0 for free-for-all).
    Multiplayer { gametype: usize },
    /// `skill` is `g_spskill` (0 easiest, the cvar's default).
    SinglePlayer { skill: u32 },
}

impl Rules {
    /// The rules for the map at `path`: the multiplayer maps (under `maps/mp/`) spawn
    /// in free-for-all, every other map in single player at the default skill.
    pub fn for_map(path: &str) -> Self {
        let path = path.replace('\\', "/").to_ascii_lowercase();
        if path.starts_with("maps/mp/") {
            Rules::Multiplayer { gametype: 0 }
        } else {
            Rules::SinglePlayer { skill: 0 }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StaticModel {
    /// The model's file, as the "model" key names it.
    pub model: String,
    pub origin: Vec3,
    /// `AnglesToAxis(angles)` (forward, left, up), each axis scaled by its scale
    /// (in single player, only by a scale other than 0 and 1: `ScaleModelAxis`).
    pub axes: [Vec3; 3],
    /// Per-axis scale ("modelscale_vec", or "modelscale" on all three).
    pub scale: Vec3,
    /// Whether the axes are left unit length, so the renderer may cull the model by
    /// its sphere (`nonNormalizedAxes` unset): single-player models without a scale.
    pub normalized: bool,
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

/// `sscanf("%f %f %f")` into zeros: the numbers it reads before the first that is
/// not one.
fn vector_prefix(s: &str) -> Vec3 {
    let mut v = [0.0; 3];
    for (x, word) in v.iter_mut().zip(s.split_ascii_whitespace()) {
        match word.parse() {
            Ok(n) => *x = n,
            Err(_) => break,
        }
    }
    v
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

/// `G_SpawnGEntityFromSpawnVars`'s filters: "notsingle", and the spawnflags bit that
/// leaves an entity out at `skill` (256 easy, 512 medium, 1024 hard).
fn spawns_at(e: &Entity, skill: u32) -> bool {
    let flag = |key| e.get(key).map_or(0, atoi);
    flag("notsingle") == 0 && flag("spawnflags") & (1 << (8 + skill)) == 0
}

/// The static models `entities` spawn under `rules`, in spawn order. One without a
/// model stops the original's map load; here it is skipped.
pub fn static_models(entities: &[Entity], rules: Rules) -> Vec<StaticModel> {
    entities
        .iter()
        .filter(|e| {
            e.get("classname")
                .is_some_and(|c| c.eq_ignore_ascii_case("misc_model_static"))
        })
        .filter_map(|e| match rules {
            Rules::Multiplayer { gametype } => {
                spawns_in(e, gametype).then(|| multiplayer(e)).flatten()
            }
            Rules::SinglePlayer { skill } => {
                spawns_at(e, skill).then(|| single_player(e)).flatten()
            }
        })
        .collect()
}

/// cgame's `SP_misc_model_static`.
fn multiplayer(e: &Entity) -> Option<StaticModel> {
    let model = e.get("model").filter(|m| !m.is_empty())?;
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
    Some(StaticModel {
        model: model.to_owned(),
        origin,
        axes: [0, 1, 2].map(|i| axis[i].map(|x| x * scale[i])),
        scale,
        normalized: false,
        zoffset,
    })
}

/// The game's `SP_misc_model_static`. Spawn keys fill the entity's fields in the
/// order they appear (`G_ParseField`), so a later "angle" or "angles" replaces an
/// earlier one; `G_SpawnString` reads the first of a key.
fn single_player(e: &Entity) -> Option<StaticModel> {
    let (mut origin, mut angles, mut model) = ([0.0; 3], [0.0; 3], None);
    for (key, value) in &e.pairs {
        match key.to_ascii_lowercase().as_str() {
            // sscanf's missing numbers come from an unset local; read them as zero.
            "origin" => origin = vector_prefix(value),
            "angles" => angles = vector_prefix(value),
            "angle" => angles = [0.0, atof(value), 0.0],
            "model" => model = Some(value.as_str()),
            _ => {}
        }
    }
    // The original stops the map load on a missing or non-MD3 model.
    let model = model.filter(|m| {
        m.get(m.len().saturating_sub(4)..)
            .is_some_and(|ext| ext.eq_ignore_ascii_case(".md3"))
    })?;
    let mut scale = vector_prefix(e.get("modelscale_vec").unwrap_or("1 1 1"));
    let modelscale = e.get("modelscale").map_or(0.0, atof);
    if modelscale != 0.0 {
        scale = [modelscale; 3];
    }
    let axis = angles_to_axis(angles);
    let scaled = scale.map(|s| s != 0.0 && s != 1.0);
    Some(StaticModel {
        model: model.to_owned(),
        origin,
        axes: [0, 1, 2].map(|i| axis[i].map(|x| if scaled[i] { x * scale[i] } else { x })),
        scale,
        normalized: !scaled.contains(&true),
        zoffset: e.get("zoffset").map_or(0.0, atof),
    })
}
