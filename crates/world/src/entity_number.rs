//! The entity numbers a single-player map's entities take as it loads, which order
//! each server frame: `G_RunFrame` updates entities by number. `G_Spawn` gives
//! each new entity the lowest free number from 1 (0 is the player's; a number freed
//! before 2000 ms may be taken again at once), so the numbers follow from which
//! entities free themselves and when:
//!
//! - as they spawn, in map order: those left out by skill or "notsingle", static
//!   models, waypoints and navgoals (the nav graph and the tags keep them),
//!   combat points, `ref_tag`s with no "target", `info_null`s with spawn flag 1;
//!   a `misc_camera` spawns its base right after itself;
//! - 100 ms later (`START_TIME_LINK_ENTS`), in number order: the other `ref_tag`s
//!   free themselves, and each door a touch opens spawns its trigger
//!   (`Think_SpawnNewDoorTrigger`);
//! - 300 ms later, the other `info_null`s;
//! - 350 ms later, each NPC spawner not waiting to be used spawns its NPC and the
//!   NPC's goal (`NPC_Spawn_Do`: `tempGoal`), then frees itself unless it spawns
//!   more than once.
//!
//! This gives every NPC and script runner the number the original's trace logs on
//! the academies, kor1, kor2 and t1_sour. Other entities that spawn others (gun
//! and ammo racks' goods, plats' triggers, emplaced guns) or free themselves are
//! left out, so t2_dpred's and t1_danger's later numbers come out lower.

use swjkja_bsp::Entity;

use crate::static_model::spawns_at;
use crate::style::atoi;

/// The numbers of a map's entities ([`entity_numbers`]), by index in its entity
/// list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Numbers {
    /// The number each entity holds once the NPCs have spawned, if it still does.
    pub entity: Vec<Option<i32>>,
    /// The number of the NPC each spawner spawned as the map began.
    pub npc: Vec<Option<i32>>,
}

/// When something happens to an entity after the load (ms after it).
enum Later {
    Free,
    DoorTrigger,
    SpawnNpc { stays: bool },
}

/// The numbers `entities` (a single-player map's, the world first) take at skill
/// `skill`.
pub fn entity_numbers(entities: &[Entity], skill: u32) -> Numbers {
    let mut numbers = Numbers {
        entity: vec![None; entities.len()],
        npc: vec![None; entities.len()],
    };
    // Whether each number is taken; the player's is 0.
    let mut used = vec![true];
    let spawn = |used: &mut Vec<bool>| match used.iter().skip(1).position(|u| !u) {
        Some(i) => {
            used[i + 1] = true;
            i as i32 + 1
        }
        None => {
            used.push(true);
            used.len() as i32 - 1
        }
    };
    let mut later: Vec<(i32, i32, usize, Later)> = Vec::new();
    for (i, e) in entities.iter().enumerate() {
        let class = e.get("classname").map(str::to_ascii_lowercase);
        let Some(class) = class.filter(|c| c != "worldspawn") else {
            continue;
        };
        let n = spawn(&mut used);
        let flags = e.get("spawnflags").map_or(0, atoi);
        let frees = !spawns_at(e, skill)
            || matches!(class.as_str(), "misc_model_static" | "point_combat")
            || class.starts_with("waypoint")
            || class == "ref_tag" && e.get("target").is_none()
            || class == "info_null" && flags & 1 != 0;
        if frees {
            used[n as usize] = false;
            continue;
        }
        numbers.entity[i] = Some(n);
        match class.as_str() {
            "ref_tag" => later.push((100, n, i, Later::Free)),
            "info_null" => later.push((300, n, i, Later::Free)),
            "misc_camera" => {
                spawn(&mut used);
            }
            // MOVER_FORCE_ACTIVATE, MOVER_LOCKED, MOVER_PLAYER_USE.
            "func_door"
                if flags & 16 != 0
                    || e.get("targetname").is_none()
                        && e.get("health").map_or(0, atoi) == 0
                        && flags & (2 | 64) == 0 =>
            {
                later.push((100, n, i, Later::DoorTrigger));
            }
            c if c.starts_with("npc_") && e.get("targetname").is_none() => {
                let count = e.get("count").map_or(0, atoi);
                let stays = count == -1 || count > 1;
                later.push((350, n, i, Later::SpawnNpc { stays }));
            }
            _ => {}
        }
    }
    later.sort_by_key(|&(at, n, ..)| (at, n));
    for (_, n, i, what) in later {
        match what {
            Later::Free => {
                used[n as usize] = false;
                numbers.entity[i] = None;
            }
            Later::DoorTrigger => {
                spawn(&mut used);
            }
            Later::SpawnNpc { stays } => {
                numbers.npc[i] = Some(spawn(&mut used));
                spawn(&mut used);
                if !stays {
                    used[n as usize] = false;
                    numbers.entity[i] = None;
                }
            }
        }
    }
    numbers
}
