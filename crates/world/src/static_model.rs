//! The map's static models (`misc_model_static`), as the game spawns them: in
//! multiplayer, cgame (OpenJK `codemp/cgame/cg_spawn.c`: `CG_ParseEntityFromSpawnVars`
//! and `SP_misc_model_static`); in single player, the game module (`code/game/g_spawn.cpp`:
//! `G_SpawnGEntityFromSpawnVars`; `g_misc.cpp`: `SP_misc_model_static`, then cgame's
//! `CG_CreateMiscEntFromGent`). The renderer loads the models and draws them each frame
//! like `CG_DrawMiscStaticModels` (multiplayer) or `CG_DrawMiscEnts` (single player).
//!
//! Single player's breakable models, racks, beacons and planted bombs ([`entity_models`])
//! are game entities instead (`code/game/g_breakable.cpp`: `SP_misc_model_breakable`;
//! `g_misc_model.cpp`: the racks; `g_misc.cpp`: beacons and bombs): the server sends
//! them to the client by the PVS of their box, and cgame draws them like any entity
//! (`code/cgame/cg_ents.cpp`: `CG_General`, and `CG_Item` for the items on a rack).
//! Multiplayer maps place none.

use swjkja_bsp::Entity;

use crate::Vec3;
use crate::collision::{
    CONTENTS_BODY, CONTENTS_BOTCLIP, CONTENTS_MONSTERCLIP, CONTENTS_OPAQUE, CONTENTS_SOLID,
    Collision, MASK_NPCSOLID,
};
use crate::npc;
use crate::rand::Rand;
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
    /// For a game entity, its linked box in world space (`absmin`, `absmax`): the
    /// server sends it when the box touches a cluster the view can see.
    pub entity_box: Option<[Vec3; 2]>,
    /// For a Ghoul2 model (`.glm`), how it is drawn.
    pub ghoul2: Option<Ghoul2>,
    /// For a breakable a script can move (one with a "script_targetname"), how it
    /// spawns as one.
    pub mover: Option<crate::script::Mover>,
}

/// How a Ghoul2 entity is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Ghoul2 {
    /// The skin file (`RE_RegisterSkin`'s name), or `None` for the mesh's own shaders.
    pub skin: Option<String>,
    /// Surfaces turned off (`true`) or on, in order, after the skin's.
    pub switches: Vec<(String, bool)>,
    /// The entity's radius (`s.radius`): the renderer culls it by a sphere of this
    /// radius around its origin, times its largest scale.
    pub radius: f32,
    /// The animations it stands in, or `None` for frame 0 of its skeleton.
    pub pose: Option<Pose>,
    /// Models bolted to it, in the order the original attaches them.
    pub attached: Vec<Attached>,
    /// The entity's colour (`shaderRGBA`), which its models and those bolted to it
    /// are drawn with: an NPC's `customRGBA`, else zero (cgame leaves it zeroed).
    pub color: [u8; 4],
}

/// The animations (named as in `animation.cfg`) an NPC's legs and torso stand
/// in: the legs' on bone "model_root", the torso's on "lower_lumbar" and "Motion"
/// (`PM_SetAnimFinal`), every other bone playing its parent's. A part whose
/// animation the skeleton lacks stays in "BOTH_STAND1", as it started. What it
/// plays over time is [`crate::script::timeline`]'s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pose {
    pub legs: &'static str,
    pub torso: &'static str,
    /// The level time (ms) it last moved at, as its `idleTime` keeps it: 0 when it
    /// stands still from the start.
    pub still_since: i32,
    /// The spawnscript it runs, if any.
    pub script: Option<std::sync::Arc<Vec<swjkja_icarus::Node>>>,
    /// The name scripts find it by (its `targetname`), if any.
    pub name: Option<String>,
    /// Whether its class's bones are too odd for cgame to turn its spine against
    /// its "Motion" bone (`G_ClassHasBadBones`).
    pub bad_bones: bool,
}

/// A Ghoul2 model bolted to another (`G_CreateG2AttachedWeaponModel`), posed at
/// frame 0 of its skeleton from where the bolt is, with its own shaders or a skin.
#[derive(Debug, Clone, PartialEq)]
pub struct Attached {
    pub model: String,
    /// The surface of the carrier it is bolted to, as "*r_hand".
    pub bolt: &'static str,
    pub skin: Option<String>,
}

/// `atof`: the longest leading number, 0 for none.
pub(crate) fn atof(s: &str) -> f32 {
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
pub(crate) fn vector_prefix(s: &str) -> Vec3 {
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
pub(crate) fn spawns_in(e: &Entity, gametype: usize) -> bool {
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
pub(crate) fn spawns_at(e: &Entity, skill: u32) -> bool {
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
        entity_box: None,
        ghoul2: None,
        mover: None,
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
        entity_box: None,
        ghoul2: None,
        mover: None,
    })
}

/// An item's radius (`FinishSpawningItem`: `s.radius`).
pub const ITEM_RADIUS: f32 = 20.0;

/// `misc_model_breakable`'s START_OFF: hidden until something uses it.
const START_OFF: i64 = 4096;

/// The models of the game entities `entities` spawn under `rules`, in their initial
/// state: none in multiplayer (no retail multiplayer map places one); in single
/// player, of those not left out by skill, in entity order, the breakables not
/// started off, the racks (a gun rack followed by its weapons), and the beacons and
/// planted bombs not waiting to be used, then the goods on the ammo racks, which the
/// original spawns 100 ms into the level.
///
/// The racks' goods are spread with the game's random numbers ([`Rand`]),
/// replayed through the spawn: the other spawn functions' draws come from
/// [`npc::spawn_draws`], reading the saber blocks in `sabers`.
///
/// A breakable without a model stops the original's map load; here it is skipped.
/// Each model draws its first frame: an animated breakable (AUTOANIMATE) or a beacon
/// cycles its frames in the original. A breakable with "gravity" drops onto the
/// floor of `world` as it spawns, and each solid breakable joins `world` where it
/// stands, for those spawned after it. The weapons on racks are Ghoul2 models
/// (`.glm`).
pub fn entity_models(
    entities: &[Entity],
    rules: Rules,
    sabers: &[u8],
    world: &mut Collision,
) -> Vec<StaticModel> {
    let Rules::SinglePlayer { skill } = rules else {
        return Vec::new();
    };
    let mut rand = Rand::default();
    let (mut models, mut ammo_racks) = (Vec::new(), Vec::new());
    for e in entities.iter().filter(|e| spawns_at(e, skill)) {
        let class = e.get("classname").unwrap_or("").to_ascii_lowercase();
        match class.as_str() {
            "misc_model_breakable" => models.extend(breakable(e, world)),
            "misc_model_gun_rack" => {
                models.push(rack(e, GUN_RACK));
                models.extend(gun_rack_goods(e, &mut rand));
            }
            "misc_model_ammo_rack" => {
                models.push(rack(e, AMMO_RACK));
                ammo_racks.push(e);
            }
            "misc_model_beacon" => models.extend(usable(e, BEACON, [16.0, 16.0, 24.0])),
            "misc_model_bomb_planted" => models.extend(usable(e, BOMB, [16.0, 16.0, 70.0])),
            _ => {
                npc::spawn_draws(e, &class, sabers, &mut rand);
            }
        }
    }
    for e in ammo_racks {
        models.extend(ammo_rack_goods(e, &mut rand));
    }
    models
}

/// The racks' models, whatever their "model" key says.
const GUN_RACK: &str = "models/map_objects/kejim/weaponsrack.md3";
const AMMO_RACK: &str = "models/map_objects/kejim/weaponsrung.md3";
/// The beacon's and the planted bomb's models, whatever their "model" key says.
const BEACON: &str = "models/map_objects/wedge/beacon.md3";
const BOMB: &str = "models/map_objects/factory/bomb_new_deact.md3";

/// The origin and angles spawn keys, in the order they appear (`G_ParseField`).
pub(crate) fn origin_and_angles(e: &Entity) -> (Vec3, Vec3) {
    let (mut origin, mut angles) = ([0.0; 3], [0.0; 3]);
    for (key, value) in &e.pairs {
        match key.to_ascii_lowercase().as_str() {
            "origin" => origin = vector_prefix(value),
            "angles" => angles = vector_prefix(value),
            "angle" => angles = [0.0, atof(value), 0.0],
            _ => {}
        }
    }
    (origin, angles)
}

/// An unscaled entity model linked with the box `mins`..`maxs` around its origin
/// (`SV_LinkEntity`: an epsilon larger).
fn entity(model: &str, origin: Vec3, angles: Vec3, mins: Vec3, maxs: Vec3) -> StaticModel {
    StaticModel {
        model: model.to_owned(),
        origin,
        axes: angles_to_axis(angles),
        scale: [1.0; 3],
        normalized: true,
        zoffset: 0.0,
        entity_box: Some([
            [0, 1, 2].map(|i| origin[i] + mins[i] - 1.0),
            [0, 1, 2].map(|i| origin[i] + maxs[i] + 1.0),
        ]),
        ghoul2: None,
        mover: None,
    }
}

/// `SP_misc_model_gun_rack` and `SP_misc_model_ammo_rack`: the rack, linked with no
/// box of its own.
fn rack(e: &Entity, model: &str) -> StaticModel {
    let (origin, angles) = origin_and_angles(e);
    entity(model, origin, angles, [0.0; 3], [0.0; 3])
}

/// `SP_misc_model_beacon` and `SP_misc_model_bomb_planted`: one with a "targetname"
/// is not drawn until something uses it.
fn usable(e: &Entity, model: &str, maxs: Vec3) -> Option<StaticModel> {
    if e.get("targetname").is_some() {
        return None;
    }
    let (origin, angles) = origin_and_angles(e);
    Some(entity(model, origin, angles, [-16.0, -16.0, 0.0], maxs))
}

/// The racks' spawnflags: both racks' blaster, then the gun rack's...
const RACK_BLASTER: i64 = 1;
const RACK_REPEATER: i64 = 2;
const RACK_ROCKET: i64 = 4;
/// ...and the ammo rack's.
const RACK_METAL_BOLTS: i64 = 2;
const RACK_ROCKETS: i64 = 4;
const RACK_WEAPONS: i64 = 8;
const RACK_HEALTH: i64 = 16;
const RACK_PWR_CELL: i64 = 32;
const RACK_NO_FILL: i64 = 64;

/// An item a rack places: its world model and box (`ext_data/items.dat`), and
/// whether it is a weapon (turned to stand on the rack).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Item {
    model: &'static str,
    mins: Vec3,
    maxs: Vec3,
    weapon: bool,
}

const fn ammo(model: &'static str, mins: Vec3) -> Item {
    Item {
        model,
        mins,
        maxs: [8.0, 8.0, 16.0],
        weapon: false,
    }
}

const fn weapon(model: &'static str) -> Item {
    Item {
        model,
        mins: [-10.0, -10.0, -2.0],
        maxs: [10.0, 10.0, 12.0],
        weapon: true,
    }
}

const AMMO_BLASTER: Item = ammo("models/items/energy_cell.md3", [-8.0, -8.0, 0.0]);
const AMMO_METAL_BOLTS: Item = ammo("models/items/metallic_bolts.md3", [-8.0, -8.0, 0.0]);
const AMMO_POWERCELL: Item = ammo("models/items/power_cell.md3", [-8.0, -8.0, 0.0]);
const AMMO_ROCKETS: Item = ammo("models/items/rockets.md3", [-8.0, -8.0, 0.0]);
const MEDPAK: Item = ammo("models/items/medpac.md3", [-8.0, -8.0, -3.0]);
const BLASTER: Item = weapon("models/weapons2/blaster_r/blaster_w.glm");
const REPEATER: Item = weapon("models/weapons2/heavy_repeater/heavy_repeater_w.glm");
const ROCKET_LAUNCHER: Item = weapon("models/weapons2/merr_sonn/merr_sonn_w.glm");

/// `SP_misc_model_gun_rack`: up to three weapons, 9 units apart, each at its own
/// height; the first fills the shelf. With no weapon checked, blasters. Each is
/// spread by up to 2 units aside and forward with draws from `rand` (aside drawn
/// first: the compiler evaluates `GunRackAddItem`'s arguments right to left).
pub(crate) fn gun_rack_goods(e: &Entity, rand: &mut Rand) -> Vec<StaticModel> {
    let flags = e.get("spawnflags").map_or(0, atoi);
    let none = flags & (RACK_BLASTER | RACK_REPEATER | RACK_ROCKET) == 0;
    let mut guns: Vec<(Item, f32)> = [
        (flags & RACK_BLASTER != 0 || none, BLASTER, 23.0),
        (flags & RACK_REPEATER != 0, REPEATER, 24.5),
        (flags & RACK_ROCKET != 0, ROCKET_LAUNCHER, 25.5),
    ]
    .into_iter()
    .filter_map(|(on, item, up)| on.then_some((item, up)))
    .collect();
    while guns.len() < 3 {
        guns.push(guns[0]);
    }
    let (origin, angles) = origin_and_angles(e);
    guns.iter()
        .enumerate()
        .map(|(i, &(item, up))| {
            let right = (i as f32 - 1.0) * 9.0 + rand.flrand(-1.0, 1.0) * 2.0;
            let fwd = rand.flrand(-1.0, 1.0) * 2.0;
            rack_item(item, origin, angles, fwd, right, up, rand)
        })
        .collect()
}

/// `spawn_rack_goods`: the ammo packs, the weapon and the medpack an ammo rack
/// places (`GunRackAddItem`), spread with draws from `rand`: each pack up to half
/// a unit forward, the weapon 4 to 10 units to a side picked at random, the
/// medpack 4 to 8 units to the other side (or to one picked at random).
pub(crate) fn ammo_rack_goods(e: &Entity, rand: &mut Rand) -> Vec<StaticModel> {
    let flags = e.get("spawnflags").map_or(0, atoi);
    let any_ammo = RACK_BLASTER | RACK_METAL_BOLTS | RACK_ROCKETS | RACK_PWR_CELL;
    let blaster = flags & RACK_BLASTER != 0 || flags & any_ammo == 0;
    let mut ammo: Vec<_> = [
        (blaster, AMMO_BLASTER),
        (flags & RACK_METAL_BOLTS != 0, AMMO_METAL_BOLTS),
        (flags & RACK_PWR_CELL != 0, AMMO_POWERCELL),
        (flags & RACK_ROCKETS != 0, AMMO_ROCKETS),
    ]
    .into_iter()
    .filter_map(|(on, item)| on.then_some(item))
    .collect();
    if flags & RACK_NO_FILL == 0 {
        // The first item fills the shelf.
        while ammo.len() < 3 {
            ammo.push(ammo[0]);
        }
    }
    let (origin, angles) = origin_and_angles(e);
    let mut goods: Vec<_> = ammo
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let fwd = rand.flrand(-1.0, 1.0) * 0.5;
            rack_item(item, origin, angles, fwd, (i as f32 - 1.0) * 8.0, 7.0, rand)
        })
        .collect();
    // A weapon: with no ammo type checked, a blaster at the rack's own height (the
    // original leaves its height at 0 then); otherwise the first of the blaster,
    // repeater and rocket launcher whose ammo is checked (not the power cell's).
    let weapon = if flags & RACK_WEAPONS == 0 {
        None
    } else if flags & any_ammo == 0 {
        Some((BLASTER, 0.0))
    } else if flags & RACK_BLASTER != 0 {
        Some((BLASTER, 25.5))
    } else if flags & RACK_METAL_BOLTS != 0 {
        Some((REPEATER, 27.0))
    } else if flags & RACK_ROCKETS != 0 {
        Some((ROCKET_LAUNCHER, 28.0))
    } else {
        None
    };
    // A side picked at random: -1 left, 1 right.
    let pick_side = |rand: &mut Rand| {
        if rand.flrand(0.0, 1.0) > 0.5 {
            -1.0
        } else {
            1.0
        }
    };
    let mut side = 0.0;
    if let Some((item, up)) = weapon {
        side = pick_side(rand);
        let right = (rand.flrand(0.0, 1.0) * 6.0 + 4.0) * side;
        let fwd = rand.flrand(-1.0, 1.0) * 2.0;
        goods.push(rack_item(item, origin, angles, fwd, right, up, rand));
    }
    if flags & RACK_HEALTH != 0 {
        side = if side == 0.0 { pick_side(rand) } else { -side };
        let right = (rand.flrand(0.0, 1.0) * 4.0 + 4.0) * side;
        let fwd = rand.flrand(-1.0, 1.0) * 0.5;
        goods.push(rack_item(MEDPAK, origin, angles, fwd, right, 24.0, rand));
    }
    goods
}

/// `GunRackAddItem`, `fwd` units ahead of, `right` units to the right of and `up`
/// units above the rack's origin, linked with the item's box, turned with draws
/// from `rand`: an item turned 90 degrees from the rack, up to 4 more; a weapon
/// turned around, up to 14 degrees more, pitched and rolled up to 4, and stood up
/// (cgame's `CG_Item` pitches a VERTICAL item 75 degrees up).
fn rack_item(
    item: Item,
    origin: Vec3,
    angles: Vec3,
    fwd: f32,
    right: f32,
    up: f32,
    rand: &mut Rand,
) -> StaticModel {
    let axis = angles_to_axis(angles);
    let mut at = [0, 1, 2].map(|i| origin[i] - right * axis[1][i] + fwd * axis[0][i]);
    at[2] += up;
    let turned = if item.weapon {
        let pitch = angle_normalize180(angles[0] + rand.flrand(-1.0, 1.0) * 4.0);
        let yaw = angle_normalize180(angles[1] + 180.0 + rand.flrand(-1.0, 1.0) * 14.0);
        let roll = angle_normalize180(angles[2] + rand.flrand(-1.0, 1.0) * 4.0);
        [pitch - 75.0, yaw, roll]
    } else {
        let yaw = angle_normalize180(angles[1] + 90.0 + rand.flrand(-1.0, 1.0) * 4.0);
        [angles[0], yaw, angles[2]]
    };
    let mut m = entity(item.model, at, turned, item.mins, item.maxs);
    if item.weapon {
        m.ghoul2 = Some(Ghoul2 {
            skin: None,
            switches: Vec::new(),
            radius: ITEM_RADIUS,
            pose: None,
            attached: Vec::new(),
            color: [0; 4],
        });
    }
    m
}

/// `AngleNormalize180`: `angle` within -180..=180, in 65536ths of a turn.
fn angle_normalize180(angle: f32) -> f32 {
    let a = (360.0f32 / 65536.0) * ((angle * (65536.0f32 / 360.0)) as i32 & 65535) as f32;
    if a > 180.0 { a - 360.0 } else { a }
}

/// `SP_misc_model_breakable`, for an entity that spawns.
fn breakable(e: &Entity, world: &mut Collision) -> Option<StaticModel> {
    let (mut origin, mut angles, mut model) = ([0.0; 3], [0.0; 3], None);
    let (mut mins, mut maxs) = ([0.0; 3], [0.0; 3]);
    for (key, value) in &e.pairs {
        match key.to_ascii_lowercase().as_str() {
            "origin" => origin = vector_prefix(value),
            "angles" => angles = vector_prefix(value),
            "angle" => angles = [0.0, atof(value), 0.0],
            "model" => model = Some(value.as_str()),
            "mins" => mins = vector_prefix(value),
            "maxs" => maxs = vector_prefix(value),
            _ => {}
        }
    }
    if e.get("spawnflags").map_or(0, atoi) & START_OFF != 0 {
        return None;
    }
    let model = model.filter(|m| {
        m.get(m.len().saturating_sub(4)..)
            .is_some_and(|ext| ext.eq_ignore_ascii_case(".md3"))
    })?;
    let mut scale = [0.0; 3];
    let mut has_scale = false;
    if let Some(v) = e.get("modelscale_vec") {
        scale = vector_prefix(v);
        has_scale = true;
    } else {
        let s = e.get("modelscale").map_or(0.0, atof);
        if s != 0.0 {
            scale = [s; 3];
            has_scale = true;
        }
    }
    if mins == [0.0; 3] {
        mins = [-16.0; 3];
    }
    if maxs == [0.0; 3] {
        maxs = [16.0; 3];
    }
    if model.eq_ignore_ascii_case("models/map_objects/ships/tie_bomber.md3") {
        (mins, maxs) = ([-80.0; 3], [80.0; 3]);
    }
    if has_scale {
        // The box scales with the model, and the origin rises by as much as the
        // box's bottom sinks.
        let old_bottom = mins[2];
        mins = [0, 1, 2].map(|i| mins[i] * scale[i]);
        maxs = [0, 1, 2].map(|i| maxs[i] * scale[i]);
        origin[2] += old_bottom - mins[2];
    }
    // misc_model_breakable_gravity_init: traced down from a unit above its origin
    // with what blocks NPCs; it then stays where it lands.
    if e.get("gravity").map_or(0.0, atof) != 0.0 {
        let top = [origin[0], origin[1], origin[2] + 1.0];
        if let Some(floor) = world.drop_to_floor(top, mins, maxs, MASK_NPCSOLID) {
            origin = floor;
        }
    }
    // SV_LinkEntity: the box around the origin, an epsilon larger.
    let entity_box = [
        [0, 1, 2].map(|i| origin[i] + mins[i] - 1.0),
        [0, 1, 2].map(|i| origin[i] + maxs[i] + 1.0),
    ];
    // misc_model_breakable_init: SOLID (spawnflag 1) blocks NPCs and shots, else
    // shots alone.
    if e.get("spawnflags").map_or(0, atoi) & 1 != 0 {
        world.add_box(
            [0, 1, 2].map(|i| origin[i] + mins[i]),
            [0, 1, 2].map(|i| origin[i] + maxs[i]),
            CONTENTS_SOLID
                | CONTENTS_OPAQUE
                | CONTENTS_BODY
                | CONTENTS_MONSTERCLIP
                | CONTENTS_BOTCLIP,
        );
    }
    // CG_General's ScaleModelAxis, as for single player's static models.
    let axis = angles_to_axis(angles);
    let scaled = scale.map(|s| s != 0.0 && s != 1.0);
    Some(StaticModel {
        model: model.to_owned(),
        origin,
        axes: [0, 1, 2].map(|i| axis[i].map(|x| if scaled[i] { x * scale[i] } else { x })),
        scale,
        normalized: !scaled.contains(&true),
        zoffset: 0.0,
        entity_box: Some(entity_box),
        ghoul2: None,
        // A script finds it by its "script_targetname" (`G_SpawnGEntityFromSpawnVars`
        // registers it with ICARUS), and moves it from where it stands.
        mover: e.get("script_targetname").map(|name| crate::script::Mover {
            name: name.to_owned(),
            origin,
            angles,
            linear: e.get("linear").map_or(0, atoi) != 0,
        }),
    })
}
