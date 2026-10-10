//! Single player's NPCs as the map spawns them (`code/game/NPC_spawn.cpp`: the
//! `SP_NPC_*` spawn functions, `SP_NPC_spawner`, `NPC_Spawn_Do`) and how they look
//! (`NPC_stats.cpp`: `NPC_ParseParms` reading the `ext_data/npcs/*.npc` blocks;
//! `g_client.cpp`: `G_SetG2PlayerModel`), drawn by cgame as players
//! (`code/cgame/cg_players.cpp`: `CG_Player`).
//!
//! Only the NPCs a map spawns by itself are here: a spawner with a "targetname"
//! waits to be used, and the ones it spawns depend on scripts. Each stands where
//! its spawner is, as it spawned: the game then runs its AI and scripts.

use swjkja_bsp::Entity;
use swjkja_shader::text::Cursor;

use crate::Vec3;
use crate::static_model::{
    Ghoul2, Rules, StaticModel, angles_to_axis, origin_and_angles, spawns_at,
};
use crate::style::atoi;

/// An NPC spawner's type, as its spawn function picks it.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcSpawn {
    /// The `.npc` block it reads, as named (looked up ignoring case).
    pub npc_type: String,
    pub origin: Vec3,
    pub angles: Vec3,
    /// Whether the spawner has a "message" (a key the NPC carries).
    pub message: bool,
}

/// The spawn function's choice of type for spawner class `class` (lowercase) with
/// `flags`, `key` its "NPC_type" and `message` whether it has one. `None` for the
/// spawners that make no NPC here: vehicles (another model, chosen by
/// `vehicles.dat`), `NPC_Galak` (spawns nothing), and an `NPC_spawner` without a
/// type (a random one).
///
/// A random choice takes the branch written first: a coin flip its "if" side, a
/// pick among several its first case.
fn npc_type(class: &str, flags: i64, key: Option<&str>, message: bool) -> Option<String> {
    let f = |bit: i64| flags & bit != 0;
    // The spawn functions that keep a type the spawner names.
    let keeps_key = matches!(
        class,
        "npc_spawner"
            | "npc_jedi"
            | "npc_prisoner"
            | "npc_rebel"
            | "npc_human_merc"
            | "npc_rockettrooper"
            | "npc_hazardtrooper"
            | "npc_ugnaught"
            | "npc_jawa"
            | "npc_gran"
            | "npc_rodian"
            | "npc_weequay"
            | "npc_trandoshan"
            | "npc_tusken"
            | "npc_noghri"
            | "npc_swamptrooper"
            | "npc_imperial"
            | "npc_bespincop"
            | "npc_reborn"
            | "npc_colombian_soldier"
            | "npc_colombian_rebel"
            | "npc_reborn_new"
            | "npc_cultist_saber"
            | "npc_cultist_saber_powers"
            | "npc_cultist"
            | "npc_cultist_commando"
            | "npc_shadowtrooper"
            | "npc_colombian_emplacedgunner"
            | "npc_saboteur"
            | "npc_droid_assassin"
            | "npc_droid_saber"
    );
    if keeps_key && let Some(key) = key {
        return Some(key.to_owned());
    }
    // SP_NPC_Cultist_Saber(_Powers): medium (1), strong (2), or neither; throwing
    // (8). Its "all" branch tests 2 again, so 4 is never reached, and the
    // throwing plain one has no "2".
    let cultist_saber = |powers: bool| {
        let name = match (f(1), f(2), f(8)) {
            (true, _, true) => "cultist_saber_med_throw",
            (true, _, false) => "cultist_saber_med",
            (false, true, true) => "cultist_saber_strong_throw",
            (false, true, false) => "cultist_saber_strong",
            (false, false, true) => return "cultist_saber_throw".to_owned(),
            (false, false, false) => "cultist_saber",
        };
        if powers {
            format!("{name}2")
        } else {
            name.to_owned()
        }
    };
    let name = match class {
        "npc_player" => "Player",
        "npc_kyle" if f(1) => "Kyle_boss",
        "npc_kyle" => "Kyle",
        "npc_lando" => "Lando",
        "npc_jan" => "Jan",
        "npc_luke" => "Luke",
        "npc_monmothma" => "MonMothma",
        "npc_rosh_penin" if f(1) => "rosh_dark",
        "npc_rosh_penin" if f(2) => "rosh_penin_noforce",
        "npc_rosh_penin" => "rosh_penin",
        "npc_tavion" => "Tavion",
        "npc_tavion_new" if f(1) => "tavion_scepter",
        "npc_tavion_new" if f(2) => "tavion_sith_sword",
        "npc_tavion_new" => "tavion_new",
        "npc_alora" if f(1) => "alora_dual",
        "npc_alora" => "alora",
        "npc_reelo" => "Reelo",
        "npc_desann" | "npc_manuel_vergara_rmg" => "Desann",
        "npc_rax" => "Rax",
        "npc_bobafett" => "Boba_Fett",
        "npc_ragnos" => "Ragnos",
        "npc_lannik_racto" => "lannik_racto",
        "npc_kothos" if f(1) => "VKothos",
        "npc_kothos" => "DKothos",
        "npc_chewbacca" => "Chewie",
        "npc_bartender" => "Bartender",
        "npc_morgankatarn" => "MorganKatarn",
        // A random Jedi (4): the first of twelve.
        "npc_jedi" if f(4) => "jedi_hf1",
        "npc_jedi" if f(2) => "jedimaster",
        "npc_jedi" if f(1) => "jeditrainer",
        "npc_jedi" => "Jedi",
        "npc_prisoner" if f(1) => "elder",
        "npc_prisoner" => "Prisoner",
        "npc_merchant" => "merchant",
        "npc_rebel" => "Rebel",
        "npc_human_merc" if message => "human_merc_key",
        "npc_human_merc" if f(1) => "human_merc_bow",
        "npc_human_merc" if f(2) => "human_merc_rep",
        "npc_human_merc" if f(4) => "human_merc_flc",
        "npc_human_merc" if f(8) => "human_merc_cnc",
        "npc_human_merc" => "human_merc",
        "npc_stormtrooper" if f(8) => "rockettrooper",
        "npc_stormtrooper" if f(4) => "stofficeralt",
        "npc_stormtrooper" if f(2) => "stcommander",
        "npc_stormtrooper" | "npc_stormtrooperofficer" if f(1) => "stofficer",
        "npc_stormtrooperofficer" => "stofficer",
        "npc_stormtrooper" => "StormTrooper",
        "npc_snowtrooper" => "snowtrooper",
        "npc_tie_pilot" => "stormpilot",
        "npc_rockettrooper" if f(1) => "rockettrooper2Officer",
        "npc_rockettrooper" => "rockettrooper2",
        "npc_hazardtrooper" if f(1) => "hazardtrooperofficer",
        "npc_hazardtrooper" if f(2) => "hazardtrooperconcussion",
        "npc_hazardtrooper" => "hazardtrooper",
        "npc_ugnaught" => "Ugnaught",
        "npc_jawa" if f(1) => "jawa_armed",
        "npc_jawa" => "jawa",
        "npc_gran" if f(1) => "granshooter",
        "npc_gran" if f(2) => "granboxer",
        "npc_gran" => "gran",
        "npc_rodian" if f(1) => "rodian2",
        "npc_rodian" => "rodian",
        "npc_weequay" => "Weequay",
        "npc_trandoshan" => "Trandoshan",
        "npc_tusken" if f(1) => "tuskensniper",
        "npc_tusken" => "tusken",
        "npc_noghri" => "noghri",
        "npc_swamptrooper" if f(1) => "SwampTrooper2",
        "npc_swamptrooper" => "SwampTrooper",
        "npc_imperial" if f(1) => "ImpOfficer",
        "npc_imperial" if f(2) => "ImpCommander",
        "npc_imperial" => "Imperial",
        "npc_impworker" => "ImpWorker",
        "npc_bespincop" => "BespinCop",
        "npc_reborn" | "npc_colombian_soldier" | "npc_colombian_rebel" => match () {
            _ if f(1) => "rebornforceuser",
            _ if f(2) => "rebornfencer",
            _ if f(4) => "rebornacrobat",
            _ if f(8) => "rebornboss",
            _ => "reborn",
        },
        "npc_reborn_new" => match (f(8), f(4), f(1), f(2)) {
            (true, _, true, _) => "RebornMasterDual",
            (true, _, _, true) => "RebornMasterStaff",
            (true, ..) => "RebornMaster",
            (_, true, true, _) => "reborn_dual2",
            (_, true, _, true) => "reborn_staff2",
            (_, true, ..) => "reborn_new2",
            (_, _, true, _) => "reborn_dual",
            (_, _, _, true) => "reborn_staff",
            _ => "reborn_new",
        },
        "npc_cultist_saber" => return Some(cultist_saber(false)),
        "npc_cultist_saber_powers" => return Some(cultist_saber(true)),
        // A random saber cultist (1): the first strength (medium), throwing.
        "npc_cultist" if f(1) => "cultist_saber_med_throw",
        "npc_cultist" if f(2) => "cultist_grip",
        "npc_cultist" if f(4) => "cultist_lightning",
        "npc_cultist" if f(8) => "cultist_drain",
        "npc_cultist" => "cultist",
        "npc_cultist_commando" => "cultistcommando",
        "npc_cultist_destroyer" => "cultist_destroyer",
        "npc_shadowtrooper" | "npc_colombian_emplacedgunner" => "ShadowTrooper",
        "npc_saboteur" if f(1) => "saboteursniper",
        "npc_saboteur" if f(2) => "saboteurpistol",
        "npc_saboteur" if f(4) => "saboteurcommando",
        "npc_saboteur" => "saboteur",
        "npc_monster_murjj" => "Murjj",
        "npc_monster_swamp" => "Swamp",
        "npc_monster_howler" => "howler",
        "npc_monster_rancor" if f(1) => "mutant_rancor",
        "npc_monster_rancor" => "rancor",
        "npc_monster_mutant_rancor" => "mutant_rancor",
        "npc_monster_wampa" => "wampa",
        "npc_minemonster" => "minemonster",
        "npc_monster_claw" => "Claw",
        "npc_monster_glider" => "Glider",
        "npc_monster_flier2" => "Flier2",
        "npc_monster_lizard" => "Lizard",
        "npc_monster_fish" => "Fish",
        "npc_monster_sand_creature" if f(1) => "sand_creature_fast",
        "npc_monster_sand_creature" => "sand_creature",
        "npc_droid_interrogator" => "interrogator",
        "npc_droid_probe" => "probe",
        "npc_droid_mark1" => "mark1",
        "npc_droid_mark2" => "mark2",
        "npc_droid_atst" => "atst",
        "npc_droid_remote" => "remote_sp",
        "npc_droid_seeker" => "seeker",
        "npc_droid_sentry" => "sentry",
        "npc_droid_gonk" => "gonk",
        "npc_droid_mouse" => "mouse",
        "npc_droid_r2d2" if f(1) => "r2d2_imp",
        "npc_droid_r2d2" => "r2d2",
        "npc_droid_r5d2" if f(1) => "r5d2_imp",
        "npc_droid_r5d2" => "r5d2",
        "npc_droid_protocol" if f(1) => "protocol_imp",
        "npc_droid_protocol" => "protocol",
        "npc_droid_assassin" => "assassin_droid",
        "npc_droid_saber" if f(1) => "saber_droid_training",
        "npc_droid_saber" => "saber_droid",
        _ => return None,
    };
    Some(name.to_owned())
}

/// The NPCs `entities` spawn as the map loads under `rules` (none in multiplayer), in
/// entity order: the spawners not left out by skill and not waiting to be used (no
/// "targetname").
pub fn npc_spawns(entities: &[Entity], rules: Rules) -> Vec<NpcSpawn> {
    let Rules::SinglePlayer { skill } = rules else {
        return Vec::new();
    };
    entities
        .iter()
        .filter(|e| spawns_at(e, skill) && e.get("targetname").is_none())
        .filter_map(|e| {
            let class = e.get("classname")?.to_ascii_lowercase();
            let flags = e.get("spawnflags").map_or(0, atoi);
            let message = e.get("message").is_some();
            let npc_type = npc_type(&class, flags, e.get("NPC_type"), message)?;
            let (origin, angles) = origin_and_angles(e);
            Some(NpcSpawn {
                npc_type,
                origin,
                angles,
                message,
            })
        })
        .collect()
}

/// The `.npc` files' texts as `NPC_LoadParms` joins them, in the order given (the
/// game's file listing of `ext_data/npcs`): each up to its first NUL, a line break
/// between.
pub fn join_parms<'a>(files: impl IntoIterator<Item = &'a [u8]>) -> Vec<u8> {
    let mut parms = Vec::new();
    for file in files {
        parms.extend_from_slice(&file[..file.iter().position(|&b| b == 0).unwrap_or(file.len())]);
        parms.push(b'\n');
    }
    parms
}

/// `DEFAULT_MINS_2`: an NPC's box always starts 24 units below its origin.
const DEFAULT_MINS_2: f32 = -24.0;
/// `playerMins` and `playerMaxs`, the box an NPC has unless its block sets one.
const PLAYER_MINS: Vec3 = [-15.0, -15.0, DEFAULT_MINS_2];
const PLAYER_MAXS: Vec3 = [15.0, 15.0, 40.0];
/// The model and skin of the player's character, which an NPC of type "Player"
/// wears (`G_InitPlayerFromCvars`, with `g_char_model` and the `g_char_skin_*` at
/// their defaults).
const PLAYER_MODEL: &str = "jedi_tf";
const PLAYER_SKIN: &str = "head_a1|torso_a1|lower_a1";

/// What an NPC's `.npc` block says about how it looks.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcLook {
    /// The folder of its Ghoul2 model under `models/players/`.
    pub model: String,
    /// "default", another skin's name, or three parts "head|torso|lower".
    pub skin: String,
    /// Surface switches in the order the original applies them: (name, off).
    pub switches: Vec<(String, bool)>,
    /// `s.modelScale`.
    pub scale: Vec3,
    pub mins: Vec3,
    pub maxs: Vec3,
    /// `s.radius`, which the renderer culls and levels it by.
    pub radius: f32,
}

/// `NPC_ParseParms` for `npc_type`, reading the `.npc` files' text `parms` (all of
/// them, in order: the first block of a name wins), for an NPC whose spawner has a
/// "message" or not. `None` if no block has the name, or it gives no Ghoul2 model
/// ("playerModel"): the original's MD3 NPCs and vehicles are not drawn here.
pub fn npc_look(parms: &[u8], npc_type: &str, message: bool) -> Option<NpcLook> {
    let mut p = Cursor::new(parms);
    loop {
        let token = p.token(true);
        if token.is_empty() {
            return None;
        }
        if token.eq_ignore_ascii_case(npc_type.as_bytes()) {
            break;
        }
        p.skip_braced_section(0);
    }
    if p.token(true) != b"{" {
        return None;
    }
    let mut model = None;
    let mut skin = "default".to_owned();
    let (mut off, mut on) = (String::new(), String::new());
    let mut scale = [1.0; 3];
    let (mut mins, mut maxs) = (PLAYER_MINS, PLAYER_MAXS);
    let mut radius = 0;
    let mut class = String::new();
    loop {
        let token = p.token(true);
        if token.is_empty() {
            return None;
        }
        if token == b"}" {
            break;
        }
        let key = String::from_utf8_lossy(token).to_ascii_lowercase();
        // COM_ParseString and COM_ParseInt: a value on the same line, or the key
        // is dropped.
        let value = String::from_utf8_lossy(p.token(false)).into_owned();
        let number = || (!value.is_empty()).then(|| atoi(&value));
        match key.as_str() {
            "playermodel" if !value.is_empty() => model = Some(value),
            "customskin" if !value.is_empty() => skin = value,
            "surfoff" | "surfon" if !value.is_empty() => {
                let list = if key == "surfoff" { &mut off } else { &mut on };
                if !list.is_empty() {
                    list.push(',');
                }
                list.push_str(&value);
            }
            "scale" | "scalex" | "scaley" | "scalez" => {
                if let Some(n) = number().filter(|&n| n >= 0 && n != 100) {
                    let s = n as f32 / 100.0;
                    match key.as_str() {
                        "scalex" => scale[0] = s,
                        "scaley" => scale[1] = s,
                        "scalez" => scale[2] = s,
                        _ => scale = [s; 3],
                    }
                }
            }
            "width" => {
                if let Some(n) = number() {
                    (mins[0], mins[1], maxs[0], maxs[1]) =
                        (-n as f32, -n as f32, n as f32, n as f32);
                }
            }
            "height" => {
                if let Some(n) = number() {
                    maxs[2] = n as f32 + DEFAULT_MINS_2;
                    radius = n;
                }
            }
            "class" if !value.is_empty() => class = value.to_ascii_lowercase(),
            // Values on the line after the key, unread: the rest of the line.
            _ if value.is_empty() => {}
            _ => p.skip_rest_of_line(),
        }
    }
    let model = model?;
    if class == "class_vehicle" {
        return None;
    }
    // The player's character takes no switches (G_ChangePlayerModel passes none).
    let player = model.eq_ignore_ascii_case("player");
    if player {
        (off, on) = (String::new(), String::new());
    }
    let (model, skin) = if player {
        (PLAYER_MODEL.to_owned(), PLAYER_SKIN.to_owned())
    } else {
        (model, skin)
    };
    // G_SetG2PlayerModelInfo: surfOff, then surfOn, then an imperial's key sleeve.
    let mut switches = Vec::new();
    for (list, turn_off) in [(&off, true), (&on, false)] {
        let mut words = Cursor::new(list.as_bytes());
        loop {
            let word = words.token(true);
            if word.is_empty() {
                break;
            }
            switches.push((String::from_utf8_lossy(word).into_owned(), turn_off));
        }
    }
    if class == "class_imperial" && message {
        switches.push(("l_arm_key".to_owned(), false));
    }
    // The radius: fixed for some classes, else "height", else the box's largest
    // extent. The spawner's flags reach the NPC only later, so a rancor's is never
    // a mutant's.
    let radius = match class.as_str() {
        "class_sand_creature" => 256.0,
        "class_rancor" => 150.0,
        _ if radius > 0 => radius as f32,
        _ => {
            let extent = mins.iter().map(|m| -m).chain(maxs).fold(f32::MIN, f32::max) as i32;
            if extent == 0 { 60.0 } else { extent as f32 }
        }
    };
    Some(NpcLook {
        model,
        skin,
        switches,
        scale,
        mins,
        maxs,
        radius,
    })
}

/// The models of the NPCs `entities` spawn under `rules`, with `parms` the `.npc`
/// files' text, in spawn order: each at its spawner's origin, turned to its yaw
/// (`CG_G2PlayerAngles` keeps a standing NPC upright), posed as it starts
/// (`BOTH_STAND1`), and linked with its box. A scaled NPC is drawn 24 units higher
/// per unit of height scale above 1 (`CG_Player`), its axes scaled
/// (`ScaleModelAxis`). An NPC whose block is missing or gives no Ghoul2 model is
/// left out (the original logs an error for a missing one).
pub fn npc_models(entities: &[Entity], rules: Rules, parms: &[u8]) -> Vec<StaticModel> {
    npc_spawns(entities, rules)
        .into_iter()
        .filter_map(|n| {
            let look = npc_look(parms, &n.npc_type, n.message)?;
            let folder = format!("models/players/{}/", look.model);
            let skin = if look.skin.contains('|') {
                format!("{folder}|{}", look.skin)
            } else {
                format!("{folder}model_{}.skin", look.skin)
            };
            let scaled = look.scale.map(|s| s != 0.0 && s != 1.0);
            let mut origin = n.origin;
            if scaled[2] {
                origin[2] += 24.0 * (look.scale[2] - 1.0);
            }
            let axis = angles_to_axis([0.0, n.angles[1], 0.0]);
            Some(StaticModel {
                model: format!("{folder}model.glm"),
                origin,
                axes: [0, 1, 2]
                    .map(|i| axis[i].map(|x| if scaled[i] { x * look.scale[i] } else { x })),
                scale: look.scale,
                normalized: !scaled.contains(&true),
                zoffset: 0.0,
                entity_box: Some([
                    [0, 1, 2].map(|i| n.origin[i] + look.mins[i] - 1.0),
                    [0, 1, 2].map(|i| n.origin[i] + look.maxs[i] + 1.0),
                ]),
                ghoul2: Some(Ghoul2 {
                    skin: Some(skin),
                    switches: look.switches,
                    radius: look.radius,
                    stand: true,
                }),
            })
        })
        .collect()
}
