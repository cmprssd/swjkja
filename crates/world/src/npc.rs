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
    Attached, Ghoul2, Pose, Rules, StaticModel, angles_to_axis, origin_and_angles, spawns_at,
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
    /// The spawner's "spawnflags", which the NPC keeps.
    pub flags: i64,
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
                flags,
            })
        })
        .collect()
}

/// The `.npc` files' texts as `NPC_LoadParms` joins them, in the order given (the
/// game's file listing of `ext_data/npcs`): each up to its first NUL, a line break
/// between. The `.sab` files join the same way for their tokens
/// (`WP_SaberLoadParms`).
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
    /// "class", lowercase ("class_stormtrooper"), or "" for none.
    pub class: String,
    pub team: Team,
    /// "rank", in the order of `rank_t` (0 a civilian, the default).
    pub rank: usize,
    /// "weapon", an index into [`WEAPONS`] (0 for none or an unknown name).
    pub weapon: usize,
    /// "saber" and "saber2": the names of their `.sab` blocks.
    pub sabers: [Option<String>; 2],
}

/// "playerTeam" (`team_t`): any other name is no team the game knows, which acts
/// as [`Team::Free`] does here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Team {
    Free,
    Player,
    Enemy,
    Neutral,
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
    let mut team = Team::Free;
    let mut rank = 0;
    let mut weapon = 0;
    let mut sabers = [None, None];
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
            // GetIDForString(TeamTable): -1 for an unknown name.
            "playerteam" if !value.is_empty() => {
                team = match value.to_ascii_lowercase().as_str() {
                    "player" | "team_player" => Team::Player,
                    "enemy" | "team_enemy" => Team::Enemy,
                    "neutral" | "team_neutral" => Team::Neutral,
                    _ => Team::Free,
                }
            }
            // TranslateRankName: a civilian for an unknown name.
            "rank" if !value.is_empty() => {
                rank = RANKS
                    .iter()
                    .position(|r| r.eq_ignore_ascii_case(&value))
                    .unwrap_or(0)
            }
            // GetIDForString(WPTable): an unknown name changes nothing.
            "weapon" if !value.is_empty() => {
                if let Some(w) = weapon_id(&value) {
                    weapon = w;
                }
            }
            "saber" if !value.is_empty() => sabers[0] = Some(value),
            "saber2" if !value.is_empty() => sabers[1] = Some(value),
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
        class,
        team,
        rank,
        weapon,
        sabers,
    })
}

/// The weapons (`weapon_t`) in order, each with its model in `weapons.dat` ("" for
/// none): a weapon's number is its index.
pub const WEAPONS: [(&str, &str); 29] = [
    ("WP_NONE", ""),
    ("WP_SABER", "models/weapons2/saber/saber_w.md3"),
    (
        "WP_BLASTER_PISTOL",
        "models/weapons2/blaster_pistol/blaster_pistol.md3",
    ),
    ("WP_BLASTER", "models/weapons2/blaster_r/blaster.md3"),
    ("WP_DISRUPTOR", "models/weapons2/disruptor/disruptor.md3"),
    ("WP_BOWCASTER", "models/weapons2/bowcaster/bowcaster.md3"),
    (
        "WP_REPEATER",
        "models/weapons2/heavy_repeater/heavy_repeater.md3",
    ),
    ("WP_DEMP2", "models/weapons2/demp2/demp2.md3"),
    ("WP_FLECHETTE", "models/weapons2/golan_arms/golan_arms.md3"),
    (
        "WP_ROCKET_LAUNCHER",
        "models/weapons2/merr_sonn/merr_sonn.md3",
    ),
    ("WP_THERMAL", "models/weapons2/thermal/thermal.md3"),
    ("WP_TRIP_MINE", "models/weapons2/laser_trap/laser_trap.md3"),
    ("WP_DET_PACK", "models/weapons2/detpack/det_pack.md3"),
    ("WP_CONCUSSION", "models/weapons2/concussion/c_rifle.md3"),
    ("WP_MELEE", "models/weapons2/noweap/noweap.md3"),
    ("WP_ATST_MAIN", "models/weapons2/noweap/noweap.md3"),
    ("WP_ATST_SIDE", "models/weapons2/noweap/noweap.md3"),
    ("WP_STUN_BATON", "models/weapons2/stun_baton/baton.md3"),
    (
        "WP_BRYAR_PISTOL",
        "models/weapons2/briar_pistol/briar_pistol.md3",
    ),
    ("WP_EMPLACED_GUN", "models/weapons2/noweap/noweap.md3"),
    ("WP_BOT_LASER", "models/weapons2/noweap/noweap.md3"),
    ("WP_TURRET", "models/weapons2/noweap/noweap.md3"),
    ("WP_TIE_FIGHTER", "models/weapons2/noweap/noweap.md3"),
    ("WP_RAPID_FIRE_CONC", "models/weapons2/noweap/noweap.md3"),
    ("WP_JAWA", "models/weapons2/jawa/jawa_gun.md3"),
    (
        "WP_TUSKEN_RIFLE",
        "models/weapons2/tusken_rifle/tusken_rifle.md3",
    ),
    (
        "WP_TUSKEN_STAFF",
        "models/weapons2/tusken_staff/tusken_staff.md3",
    ),
    (
        "WP_SCEPTER",
        "models/weapons2/sith_scepter/sith_scepter.md3",
    ),
    (
        "WP_NOGHRI_STICK",
        "models/weapons2/noghri_stick/noghri_stick.md3",
    ),
];

/// The number of the weapon named `name`, ignoring case ("NULL" is none).
fn weapon_id(name: &str) -> Option<usize> {
    if name.eq_ignore_ascii_case("NULL") {
        return Some(0);
    }
    WEAPONS
        .iter()
        .position(|(w, _)| w.eq_ignore_ascii_case(name))
}

fn weapon(name: &str) -> usize {
    weapon_id(name).expect("a weapon of the table")
}

/// The ranks (`rank_t`) by their names in `.npc` files, lowest first.
const RANKS: [&str; 8] = [
    "civilian",
    "crewman",
    "ensign",
    "ltjg",
    "lt",
    "ltcomm",
    "commander",
    "captain",
];

/// `NPC_WeaponsForTeam` and `NPC_SetWeapons`: the weapon an NPC of `team` and
/// type `npc_type` with spawn flags `flags` gets when its block gives none. Of a
/// pair, the later weapon wins unless it is melee.
fn team_weapon(team: Team, flags: i64, npc_type: &str) -> usize {
    let is = |name: &str| npc_type.eq_ignore_ascii_case(name);
    let starts = |prefix: &str| {
        npc_type
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    };
    let name = match team {
        Team::Enemy => match () {
            _ if is("tavion") || starts("reborn") || is("desann") || starts("shadowtrooper") => {
                "WP_SABER"
            }
            _ if starts("stofficer") => "WP_FLECHETTE",
            _ if is("stcommander") => "WP_REPEATER",
            _ if is("swamptrooper") => "WP_FLECHETTE",
            _ if is("swamptrooper2") => "WP_REPEATER",
            _ if is("rockettrooper") => "WP_ROCKET_LAUNCHER",
            _ if is("imperial") || starts("impworker") || is("stormpilot") => "WP_BLASTER_PISTOL",
            _ if is("galak") => "WP_BLASTER",
            _ if is("galak_mech") => "WP_REPEATER",
            _ if starts("ugnaught") => "WP_NONE",
            _ if is("granshooter") => "WP_BLASTER",
            _ if is("granboxer") => "WP_MELEE",
            // Thermal detonators and melee: the detonators.
            _ if starts("gran") => "WP_THERMAL",
            _ if is("rodian") => "WP_DISRUPTOR",
            _ if is("rodian2") => "WP_BLASTER",
            _ if is("interrogator") || is("sentry") || starts("protocol") => "WP_NONE",
            _ if starts("weequay") => "WP_BOWCASTER",
            _ if is("impofficer") || is("impcommander") => "WP_BLASTER",
            _ if is("probe") || is("seeker") || starts("remote") => "WP_BOT_LASER",
            _ if is("trandoshan") => "WP_REPEATER",
            // The main gun and the side guns: the side guns.
            _ if is("atst") => "WP_ATST_SIDE",
            _ if is("mark1") || is("mark2") => "WP_BOT_LASER",
            _ if is("minemonster") || is("howler") => "WP_MELEE",
            _ => "WP_BLASTER",
        },
        // SFB_RIFLEMAN, SFB_PHASER.
        Team::Player if flags & 2 != 0 => "WP_REPEATER",
        Team::Player if flags & 4 != 0 => "WP_BLASTER_PISTOL",
        Team::Player => match () {
            _ if starts("jedi") || is("luke") => "WP_SABER",
            _ if starts("prisoner") || starts("elder") || is("MonMothma") => "WP_NONE",
            _ if starts("bespincop") => "WP_BLASTER_PISTOL",
            _ => "WP_BLASTER",
        },
        Team::Neutral | Team::Free => "WP_NONE",
    };
    weapon(name)
}

/// What a saber's `.sab` block says about its hilt (`WP_SaberParseParms`).
#[derive(Debug, Clone, PartialEq)]
pub struct SaberLook {
    /// "saberModel": a Ghoul2 model.
    pub model: String,
    /// "customSkin", or `None` for the model's own shaders.
    pub skin: Option<String>,
    /// "twoHanded": no second saber goes with it.
    pub two_handed: bool,
    /// "boltToWrist": bolted to the forearm's cap, not the hand.
    pub to_wrist: bool,
}

/// `WP_SaberParseParms` for saber `name` in the `.sab` files' text `sabers` (all
/// of them, in order: the first block of a name wins), from `WP_SaberSetDefaults`'
/// hilt (saber_reborn's). A saber no block names keeps the defaults.
pub fn saber_look(sabers: &[u8], name: &str) -> SaberLook {
    let mut look = SaberLook {
        model: "models/weapons2/saber_reborn/saber_w.glm".to_owned(),
        skin: None,
        two_handed: false,
        to_wrist: false,
    };
    let mut p = Cursor::new(sabers);
    loop {
        let token = p.token(true);
        if token.is_empty() {
            return look;
        }
        if token.eq_ignore_ascii_case(name.as_bytes()) {
            break;
        }
        p.skip_braced_section(0);
    }
    if p.token(true) != b"{" {
        return look;
    }
    loop {
        let token = p.token(true);
        if token.is_empty() || token == b"}" {
            return look;
        }
        // Each keyword reads one value on its line, but "notInMP", which skips the
        // line (the original also skips the line of a keyword it does not know).
        let key = String::from_utf8_lossy(token).to_ascii_lowercase();
        if key == "notinmp" {
            p.skip_rest_of_line();
            continue;
        }
        let value = String::from_utf8_lossy(p.token(false)).into_owned();
        match key.as_str() {
            "sabermodel" if !value.is_empty() => look.model = value,
            "customskin" if !value.is_empty() => look.skin = Some(value),
            "twohanded" if !value.is_empty() => look.two_handed = atoi(&value) != 0,
            "bolttowrist" if !value.is_empty() => look.to_wrist = atoi(&value) != 0,
            _ => {}
        }
    }
}

/// `G_CreateG2AttachedWeaponModel`'s Ghoul2 model for a weapon model `name`: an
/// MD3's name loses ".md3" and what follows, gains "_w" unless it has one (or is
/// "noweap", which has no Ghoul2 model), and ends in ".glm".
pub fn attached_model(name: &str) -> String {
    let Some(at) = name.find(".md3") else {
        return name.to_owned();
    };
    let base = &name[..at];
    let w = if base.contains("_w") || base.contains("noweap") {
        ""
    } else {
        "_w"
    };
    format!("{base}{w}.glm")
}

/// What an NPC of `look`, type `npc_type`, with spawn flags `flags` holds as it
/// spawns and the animations it then stands in, with `sabers` the `.sab` files'
/// text.
///
/// Its weapon is its block's, or one its team gives (`NPC_Begin`). A saber
/// wielder holds its saber's hilt, a second in the left hand when it has two
/// (`WP_SaberAddG2SaberModels`); others hold their weapon in the right hand, as
/// `NPC_SetMiscDefaultData` gives them by team: none for good Jedi, saber-wielding
/// enemies, flying droids or neutrals, and a reborn officer's blaster pistol twice.
///
/// It stands as its first movement frames leave it with no enemy, its weapon idle
/// (`PM_Footsteps`, `PM_TorsoAnimation`): legs apart (`BOTH_STAND3`) holding a gun,
/// an imperial with none in `BOTH_STAND4`, others in `BOTH_STAND1`; the torso in
/// its weapon's idle animation. A saber's blade is off, so its torso follows its
/// legs.
pub fn npc_gear(
    look: &NpcLook,
    npc_type: &str,
    flags: i64,
    sabers: &[u8],
) -> (Pose, Vec<Attached>) {
    let class = look.class.as_str();
    let weapon_number = match look.weapon {
        0 => team_weapon(look.team, flags, npc_type),
        w => w,
    };
    let held = |name: &str| weapon_number == weapon(name);
    let mut attached = Vec::new();
    // ps.saber[0].type is set once the block names a saber.
    if let (Some(first), true) = (&look.sabers[0], held("WP_SABER")) {
        let first = saber_look(sabers, first);
        let second = look.sabers[1]
            .as_ref()
            .map(|name| saber_look(sabers, name))
            .filter(|s| !first.two_handed && !s.two_handed);
        for (saber, hand) in [(Some(first), "r"), (second, "l")] {
            let Some(saber) = saber else { continue };
            let bolt = match (saber.to_wrist, hand) {
                (true, "r") => "*r_hand_cap_r_arm",
                (true, _) => "*l_hand_cap_l_arm",
                (false, "r") => "*r_hand",
                (false, _) => "*l_hand",
            };
            attached.push(Attached {
                model: saber.model,
                bolt,
                skin: saber.skin,
            });
        }
    }
    let gun = |bolt| Attached {
        model: attached_model(WEAPONS[weapon_number].1),
        bolt,
        skin: None,
    };
    let armed = weapon_number != 0 && !held("WP_SABER");
    match look.team {
        Team::Player
            if !matches!(
                class,
                "class_seeker" | "class_jedi" | "class_kyle" | "class_luke"
            ) =>
        {
            if armed {
                attached.push(gun("*r_hand"));
            }
        }
        Team::Enemy
            if !matches!(
                class,
                "class_tavion"
                    | "class_alora"
                    | "class_desann"
                    | "class_shadowtrooper"
                    | "class_probe"
                    | "class_remote"
                    | "class_interrogator"
                    | "class_sentry"
            ) && !(class == "class_reborn" && held("WP_SABER")) =>
        {
            if armed {
                attached.push(gun("*r_hand"));
                // Dual blaster pistols for a reborn of RANK_LT_COMM or above.
                if held("WP_BLASTER_PISTOL") && class == "class_reborn" && look.rank >= 5 {
                    attached.push(gun("*l_hand"));
                }
            }
        }
        Team::Free if armed && !held("WP_MELEE") => attached.push(gun("*r_hand")),
        _ => {}
    }

    // PM_Footsteps: a gun between the saber and the detonation pack (exclusive).
    let gun_held = (weapon("WP_SABER") + 1..weapon("WP_DET_PACK")).contains(&weapon_number);
    let legs = match class {
        "class_galakmech" => "BOTH_STAND2",
        "class_atst" => "BOTH_STAND1",
        _ if gun_held => "BOTH_STAND3",
        "class_imperial" => "BOTH_STAND4",
        _ => "BOTH_STAND1",
    };
    // PM_TorsoAnimation, weapon idle: the legs' guard idles first, then by weapon.
    let dual_pistols = attached.len() == 2 && held("WP_BLASTER_PISTOL");
    let torso = match WEAPONS[weapon_number].0 {
        _ if class == "class_galakmech" => "BOTH_STAND2",
        "WP_SABER" => legs,
        _ if legs == "BOTH_STAND4" => legs,
        "WP_NONE" => "BOTH_STAND1",
        "WP_BLASTER_PISTOL" if dual_pistols => "BOTH_STAND1",
        "WP_BRYAR_PISTOL" | "WP_BLASTER_PISTOL" | "WP_BOT_LASER" => "TORSO_WEAPONIDLE2",
        "WP_MELEE" if class == "class_rancor" => "BOTH_STAND1",
        "WP_MELEE"
            if matches!(
                class,
                "class_probe"
                    | "class_seeker"
                    | "class_interrogator"
                    | "class_sentry"
                    | "class_remote"
            ) =>
        {
            "BOTH_STAND1"
        }
        "WP_MELEE" => "BOTH_STAND6",
        "WP_TUSKEN_STAFF" => "BOTH_STAND3",
        "WP_DISRUPTOR" | "WP_TUSKEN_RIFLE" => "TORSO_WEAPONREADY3",
        "WP_THERMAL" | "WP_TRIP_MINE" | "WP_DET_PACK" => "BOTH_STAND1",
        _ => "TORSO_WEAPONIDLE3",
    };
    (Pose { legs, torso }, attached)
}

/// The models of the NPCs `entities` spawn under `rules`, with `parms` the `.npc`
/// files' text and `sabers` the `.sab` files', in spawn order: each at its
/// spawner's origin, turned to its yaw (`CG_G2PlayerAngles` keeps a standing NPC
/// upright), standing as its first movement frames leave it with what it holds
/// ([`npc_gear`]), and linked with its box. A scaled NPC is drawn 24 units higher
/// per unit of height scale above 1 (`CG_Player`), its axes scaled
/// (`ScaleModelAxis`). An NPC whose block is missing or gives no Ghoul2 model is
/// left out (the original logs an error for a missing one).
pub fn npc_models(
    entities: &[Entity],
    rules: Rules,
    parms: &[u8],
    sabers: &[u8],
) -> Vec<StaticModel> {
    npc_spawns(entities, rules)
        .into_iter()
        .filter_map(|n| {
            let look = npc_look(parms, &n.npc_type, n.message)?;
            let (pose, attached) = npc_gear(&look, &n.npc_type, n.flags, sabers);
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
                    pose: Some(pose),
                    attached,
                }),
            })
        })
        .collect()
}
