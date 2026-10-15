//! The map's static models (`misc_model_static`), drawn the way multiplayer draws
//! them: cgame's `CG_DrawMiscStaticModels` culls each one by a sphere, the PVS and
//! the map's `distanceCull`, then the renderer's `R_AddMD3Surfaces` picks a level of
//! detail (`R_ComputeLOD`), culls its box (`R_CullModel`, which skips the sphere for
//! these scaled entities) and picks a fog (`R_ComputeFogNum`).
//!
//! Single player differs (`code/cgame/cg_main.cpp`: `CG_DrawMiscEnts`; `code/rd-vanilla`):
//! cgame culls by the PVS (drawing nothing from a view in solid, nor a model whose
//! cull point is in solid) and a fixed distance only, the renderer also culls by the
//! frame's sphere when the model has no scale, picks levels with `r_lodscale` 10,
//! prefers a fog that holds the whole model, and lights the model at its cull point.
//!
//! Single player's breakable models (`misc_model_breakable`) are game entities: the
//! server sends them when their box touches a cluster the view can see
//! (`SV_AddEntitiesToPacket`), cgame adds them before the static models with no cull
//! of its own (`CG_General`), and the renderer culls and lights them like single
//! player's static models, lit at their origin.
//!
//! The weapons on single player's racks are Ghoul2 models, items drawn by the
//! renderer's `R_AddGhoulSurfaces`: culled by a sphere of the entity's radius (20 for
//! an item, [`swjkja_world::static_model::ITEM_RADIUS`]) around its origin, their
//! level of detail and fog picked from the same sphere, and their meshes posed at
//! frame 0 of their skeleton.
//!
//! Single player's NPCs are Ghoul2 entities too, drawn by cgame as players
//! (`CG_Player`): their skin on, their surfaces switched, posed as they stand at the
//! time drawn, culled and leveled by their radius times their largest scale, and
//! lit at their origin.
//!
//! The models never move, so each placement's surfaces are baked into world space at
//! load; their normals carry the axes' scale, which is what the original's lighting
//! sees (`R_SetupEntityLighting` turns the light into the model's scaled axes). An
//! NPC's pose changes as time passes: its [`Rig`] gives its bones at a time, in
//! world space, for the renderer to skin its surfaces by
//! ([`StaticModels::bones`]), its head turned toward what it looks at
//! ([`crate::look`]).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use swjkja_bsp::{Bsp, DrawVert};
use swjkja_fs::Vfs;
use swjkja_model::animation;
use swjkja_model::gla::{self, Gla, Mat34};
use swjkja_model::glm::{self, Ghoul2Model};
use swjkja_model::md3::{Md3, Md3Model};
use swjkja_model::skin::Skin;
use swjkja_world::Geometry;
use swjkja_world::collision::Collision;
use swjkja_world::npc;
use swjkja_world::script::{self, Timeline};
use swjkja_world::static_model::{Pose, Rules, StaticModel};

use crate::camera::Camera;
use crate::fog::FogVolume;
use crate::look::{self, Head};
use crate::vis::{EntityLink, Frustum, WorldVis};

pub type Vec3 = [f32; 3];

/// `r_lodscale`, the original's default: multiplayer's, then single player's.
const LOD_SCALE: f32 = 5.0;
const SP_LOD_SCALE: f32 = 10.0;
/// Single player's cull distance, squared (`CG_DrawMiscEnts`: 8192 * 8192).
const SP_DISTANCE_CULL_SQ: f32 = 8192.0 * 8192.0;
/// The worldspawn's "distanceCull" when it has none (`DEFAULT_DISTANCE_CULL`).
const DEFAULT_DISTANCE_CULL: f32 = 6000.0;

/// One surface of one level of one placed model, in world space.
pub struct Piece {
    /// The surface's shader (skin 0), or "" for none (the default shader).
    pub shader: String,
    /// Fog number (0 none, else the map's fog + 1), as for world surfaces.
    pub fog: usize,
    /// A triangle soup whose vertices hold the entity number in `lightmap[3][1]`.
    pub geometry: Geometry,
    /// For a posed model's surface, which of its rig's surfaces it is: it is posed
    /// again as time passes ([`StaticModels::animate`]).
    pub source: Option<Source>,
}

/// A surface of instance `instance`'s rig: part `part` (0 the body), level `lod`
/// of its mesh, surface `surface`.
#[derive(Debug, Clone, Copy)]
pub struct Source {
    pub instance: usize,
    pub part: usize,
    pub lod: usize,
    pub surface: usize,
}

pub struct ModelLod {
    /// Frame 0's bounds in the model's own space (a Ghoul2 mesh's, posed).
    pub bounds: [Vec3; 2],
    /// Frame 0's sphere in the model's own space (centre, radius).
    pub sphere: (Vec3, f32),
    /// Indexes into [`StaticModels::pieces`].
    pub pieces: Range<usize>,
}

pub struct Instance {
    pub origin: Vec3,
    /// Scaled axes.
    pub axes: [Vec3; 3],
    /// The point cgame culls by: the origin, 1 + zoffset units up.
    pub cull_origin: Vec3,
    /// The point the model is lit from: the origin, or in single player the cull
    /// point (`RF_LIGHTING_ORIGIN`).
    pub light_origin: Vec3,
    /// Whether the renderer culls by the frame's sphere too (unit axes).
    pub sphere_cull: bool,
    pub cull_cluster: i32,
    /// A game entity's linked box (see [`StaticModel::entity_box`]), and where the
    /// server links it once located.
    pub entity_box: Option<[Vec3; 2]>,
    pub link: Option<EntityLink>,
    /// cgame's culling radius: the scaled bounds' (`R_ModelBounds`, level 0). In
    /// single player, the squared distance between the scaled bounds' corners.
    pub radius: f32,
    /// `R_ComputeLOD`'s radius: level 0's frame 0 bounds, unscaled; for a Ghoul2
    /// model, three quarters of its radius (`G2_ComputeLOD`).
    pub lod_radius: f32,
    /// A Ghoul2 model's radius: the renderer culls it by this sphere around its
    /// origin (`R_GCullModel`), not by its levels' spheres and boxes.
    pub ghoul2_radius: Option<f32>,
    /// Most detailed first.
    pub lods: Vec<ModelLod>,
    /// What poses it as time passes: an NPC's skeleton and animations.
    pub rig: Option<Rig>,
}

pub struct StaticModels {
    /// Entity `n + 1` is instance `n` (entity 0 is the world): the game entities, then
    /// the static models, in the order the original adds them to the scene.
    pub instances: Vec<Instance>,
    pub pieces: Vec<Piece>,
    /// Every shader the models register, in the order the original registers them
    /// (`R_LoadMD3`, levels from least detailed to most).
    pub shaders: Vec<String>,
    distance_cull: f32,
    rules: Rules,
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(v: Vec3) -> f32 {
    dot(v, v).sqrt()
}

/// `RadiusFromBounds`.
pub fn radius_from_bounds([mins, maxs]: [Vec3; 2]) -> f32 {
    length([0, 1, 2].map(|i| mins[i].abs().max(maxs[i].abs())))
}

/// `origin + Σ v[k] * axes[k]`.
fn transform(origin: Vec3, axes: &[Vec3; 3], v: Vec3) -> Vec3 {
    [0, 1, 2].map(|i| origin[i] + v[0] * axes[0][i] + v[1] * axes[1][i] + v[2] * axes[2][i])
}

/// The sphere an MD3 at `origin` picks its fog by: frame 0's, its local origin
/// unrotated and unscaled.
pub fn md3_fog_sphere(origin: Vec3, md3: &Md3) -> (Vec3, f32) {
    let frame = &md3.frames[0];
    let center = [0, 1, 2].map(|i| origin[i] + frame.local_origin[i]);
    (center, frame.radius)
}

/// `R_ComputeFogNum`: the first fog whose bounds the frame's sphere overlaps.
pub fn fog_num(fogs: &[FogVolume], origin: Vec3, md3: &Md3) -> usize {
    let (center, radius) = md3_fog_sphere(origin, md3);
    fogs.iter()
        .position(|f| {
            (0..3)
                .all(|j| center[j] - radius < f.bounds[1][j] && center[j] + radius > f.bounds[0][j])
        })
        .map_or(0, |i| i + 1)
}

/// Single player's `R_ComputeFogNum` (and `R_GComputeFogNum`, for a Ghoul2 model's
/// sphere), for a sphere seen from fog `view_fog` (0 for none): the first fog that
/// holds the sphere's box (centre ± radius) whole; before it, a fog that holds one of
/// the box's two extreme corners and matches the view's fog (`R_FogParmsMatch`: the
/// same red and green, fog 0's being zero); failing both, the first fog holding a
/// corner.
pub fn sp_fog_num(fogs: &[FogVolume], center: Vec3, radius: f32, view_fog: usize) -> usize {
    let lo = center.map(|c| c - radius);
    let hi = center.map(|c| c + radius);
    let inside =
        |p: Vec3, f: &FogVolume| (0..3).all(|j| p[j] >= f.bounds[0][j] && p[j] <= f.bounds[1][j]);
    let parms = |n: usize| match n {
        0 => [0.0; 2],
        n => [fogs[n - 1].parms_color[0], fogs[n - 1].parms_color[1]],
    };
    let mut partial = 0;
    for (i, f) in fogs.iter().enumerate() {
        let n = i + 1;
        if inside(lo, f) && inside(hi, f) {
            return n;
        }
        if inside(lo, f) || inside(hi, f) {
            if view_fog == n || parms(view_fog) == parms(n) {
                return n;
            }
            if partial == 0 {
                partial = n;
            }
        }
    }
    partial
}

/// `R_ComputeLOD` with `r_lodbias` 0: the level for a model of `lod_radius` at
/// `origin`, from its projected size.
pub fn lod(count: usize, lod_radius: f32, origin: Vec3, camera: &Camera, scale: f32) -> usize {
    if count < 2 {
        return 0;
    }
    // ProjectRadius: the radius over the distance along the view axis, in units of
    // the half screen height.
    let forward = camera.axes()[0];
    let dist = dot(forward, origin) - dot(forward, camera.origin);
    let projected = if dist <= 0.0 {
        0.0
    } else {
        (lod_radius.abs() / (dist * camera.tan_half_fov().1)).min(1.0)
    };
    // Projected to nothing (the model crosses the view plane): the most detail.
    let flod = if projected != 0.0 {
        1.0 - projected * scale
    } else {
        0.0
    };
    ((flod * count as f32) as i32).clamp(0, count as i32 - 1) as usize
}

impl StaticModels {
    /// Loads the models the map places under `rules` (its game entities in their initial
    /// state, the secondary models of its brush entities, its NPCs, then its static
    /// models) and bakes each placement (call
    /// [`StaticModels::locate`] before drawing). A model that does not load stops the
    /// original's map load; here its placements are skipped with a warning.
    ///
    /// A single-player model's fog is the one it has seen from outside any fog. One
    /// retail model's fog depends on where the view is (taspir1's Raven's Claw, from
    /// inside fog 1); a retail-data test checks there are no others.
    ///
    /// The map is the one at `map` (as "maps/academy2.bsp"), which gives the rules
    /// ([`Rules::for_map`]) and, in single player, the humanoids' cinematic skeleton
    /// ([`animation::cinematic_skeleton`]). NPCs are baked as they stand at game time
    /// `time` (ms).
    pub fn load(vfs: &Vfs, bsp: &Bsp, fogs: &[FogVolume], map: &str, time: i32) -> Self {
        let rules = Rules::for_map(map);
        let sp = matches!(rules, Rules::SinglePlayer { .. });
        let cinematic = sp.then(|| Cinematic::load(vfs, map)).flatten();
        let mut world = Collision::at_spawn(bsp, rules);
        let mut placements =
            swjkja_world::static_model::entity_models(&bsp.entities, rules, &mut world);
        placements.extend(
            swjkja_world::brush_entity::brush_entities(&bsp.entities, &bsp.models, rules)
                .into_iter()
                .filter_map(|e| e.model2),
        );
        if sp {
            let text = |folder: &str, ext: &str| {
                let files: Vec<Vec<u8>> = vfs
                    .list(folder, ext)
                    .iter()
                    .filter_map(|f| vfs.read(&format!("{folder}/{f}")).ok())
                    .collect();
                npc::join_parms(files.iter().map(Vec::as_slice))
            };
            let (parms, sabers) = (
                text("ext_data/npcs", ".npc"),
                text("ext_data/sabers", ".sab"),
            );
            let scripts = |name: &str| {
                let bytes = vfs.read(&script::script_path(name)?).ok()?;
                swjkja_icarus::parse(&bytes)
                    .inspect_err(|e| eprintln!("warning: script {name}: {e}"))
                    .ok()
            };
            placements.extend(npc::npc_models(
                &bsp.entities,
                rules,
                &parms,
                &sabers,
                &world,
                &scripts,
            ));
        }
        placements.extend(swjkja_world::static_model::static_models(
            &bsp.entities,
            rules,
        ));
        let mut models: HashMap<String, Option<Model>> = HashMap::new();
        let mut skins: HashMap<String, Option<Skin>> = HashMap::new();
        let mut shaders = Vec::new();
        let mut instances = Vec::new();
        let mut pieces = Vec::new();
        for p in &placements {
            register_model(vfs, &mut models, &mut shaders, &p.model);
            if let Some(g) = &p.ghoul2 {
                // RE_RegisterSkin registers the skin's shaders after the mesh's; the
                // models attached later come after.
                if let Some(name) = &g.skin {
                    register_skin(vfs, &mut skins, &mut shaders, name);
                }
                for a in &g.attached {
                    register_model(vfs, &mut models, &mut shaders, &a.model);
                    if let Some(name) = &a.skin {
                        register_skin(vfs, &mut skins, &mut shaders, name);
                    }
                }
            }
            let skin = |name: &Option<String>| {
                name.as_ref()
                    .and_then(|n| skins[&n.to_ascii_lowercase()].as_ref())
            };
            let model = match &models[&p.model.to_ascii_lowercase()] {
                Some(Model::Md3(m)) => m,
                Some(Model::Ghoul2(m)) => {
                    let Some(g) = &p.ghoul2 else {
                        eprintln!("warning: model {}: a Ghoul2 mesh placed as an MD3", p.model);
                        continue;
                    };
                    let rig = g
                        .pose
                        .as_ref()
                        .map(|pose| Rig::new(vfs, m, pose, cinematic.as_ref()));
                    let pose = match &rig {
                        Some(rig) => rig.body_pose(time, &[]),
                        None => m.gla.pose(0),
                    };
                    let body = Part::new(m, pose, skin(&g.skin), &g.switches);
                    let mut parts = Vec::new();
                    let mut attached = Vec::new();
                    for a in &g.attached {
                        // G_CreateG2AttachedWeaponModel: nothing without the model or
                        // the bolt.
                        let Some(Model::Ghoul2(w)) = &models[&a.model.to_ascii_lowercase()] else {
                            continue;
                        };
                        let Some((s, root)) = m
                            .glm
                            .find_surface(a.bolt)
                            .and_then(|s| glm::bolt(&m.glm, s, &body.pose).map(|root| (s, root)))
                        else {
                            continue;
                        };
                        parts.push(Part::new(w, attached_pose(w, &root), skin(&a.skin), &[]));
                        attached.push((w.clone(), s));
                    }
                    parts.insert(0, body);
                    let entity = instances.len() + 1;
                    let mut instance = ghoul2_instance(
                        &parts,
                        p,
                        g.radius,
                        entity,
                        fogs,
                        rig.is_some(),
                        &mut pieces,
                    );
                    instance.rig = rig.map(|rig| Rig { attached, ..rig });
                    instances.push(instance);
                    continue;
                }
                None => continue,
            };
            let entity = instances.len() + 1;
            let lods = model
                .lods
                .iter()
                .map(|md3| {
                    let first = pieces.len();
                    let fog = if sp {
                        let (center, radius) = md3_fog_sphere(p.origin, md3);
                        sp_fog_num(fogs, center, radius, 0)
                    } else {
                        fog_num(fogs, p.origin, md3)
                    };
                    for s in &md3.surfaces {
                        let (xyz, normals) = s.frame(0);
                        let shader = s.shaders.first().map_or("", String::as_str);
                        let mesh = (xyz, normals, &s.st[..], &s.indexes[..]);
                        pieces.push(bake(mesh, shader, p, entity, fog, None));
                    }
                    ModelLod {
                        bounds: md3.frames[0].bounds,
                        sphere: (md3.frames[0].local_origin, md3.frames[0].radius),
                        pieces: first..pieces.len(),
                    }
                })
                .collect();
            let bounds = model.lods[0].frames[0].bounds;
            let scaled = bounds.map(|b| [0, 1, 2].map(|i| b[i] * p.scale[i]));
            let cull_origin = [p.origin[0], p.origin[1], p.origin[2] + 1.0 + p.zoffset];
            // Single player lights a static model at its cull point
            // (RF_LIGHTING_ORIGIN), an entity at its origin.
            let light_origin = if sp && p.entity_box.is_none() {
                cull_origin
            } else {
                p.origin
            };
            let radius = if sp {
                // CG_CreateMiscEnts: DistanceSquared(mins, maxs), squared on purpose.
                let d = [0, 1, 2].map(|i| scaled[1][i] - scaled[0][i]);
                dot(d, d)
            } else {
                radius_from_bounds(scaled)
            };
            instances.push(Instance {
                origin: p.origin,
                axes: p.axes,
                cull_origin,
                light_origin,
                sphere_cull: sp && p.normalized,
                cull_cluster: -1,
                entity_box: p.entity_box,
                link: None,
                radius,
                lod_radius: radius_from_bounds(bounds),
                ghoul2_radius: None,
                lods,
                rig: None,
            });
        }
        resolve_looks(&mut instances);
        let distance_cull = bsp
            .entities
            .first()
            .and_then(|e| e.get("distanceCull"))
            .and_then(|v| v.split_ascii_whitespace().next()?.parse().ok())
            .unwrap_or(DEFAULT_DISTANCE_CULL);
        Self {
            instances,
            pieces,
            shaders,
            distance_cull,
            rules,
        }
    }

    /// Finds the cluster each model is culled by, and links each entity.
    pub fn locate(&mut self, vis: &WorldVis) {
        for m in &mut self.instances {
            m.cull_cluster = vis.cluster(m.cull_origin);
            m.link = m.entity_box.map(|b| vis.link(b));
        }
    }

    /// Instance `n`'s bones as it stands at game time `time` (ms), in world space:
    /// each part's skeleton (the body's, then each bolted model's) after the
    /// instance's origin and axes, so a vertex skinned by them (as
    /// [`glm::skin`] does) lands where [`Piece::geometry`] has it. None for a model
    /// without a rig.
    pub fn bones(&self, n: usize, time: i32) -> Vec<Mat34> {
        let m = &self.instances[n];
        let Some(rig) = &m.rig else {
            return Vec::new();
        };
        let poses = rig.poses(time, &self.look_overrides(n, time));
        poses.iter().flatten().map(|b| place(m, b)).collect()
    }

    /// Where instance `n`'s eyes are with its body posed by `body`, and the angles
    /// they face (`renderInfo.eyePoint`, `eyeAngles`): its "*head_eyes" tag, or its
    /// origin and angles without one.
    fn eyes(&self, n: usize, body: &[Mat34]) -> (Vec3, Vec3) {
        let m = &self.instances[n];
        let tag = m.rig.as_ref().and_then(|rig| {
            let eyes = rig.eyes?;
            glm::bolt(&rig.body.glm, eyes, body)
        });
        match tag {
            Some(tag) => {
                let tag = place(m, &tag);
                ([0, 1, 2].map(|r| tag[r][3]), look::eye_angles(&tag))
            }
            None => (m.origin, look::vectoangles(m.axes[0])),
        }
    }

    /// What instance `n` looks at at `time` and its settled look ([`look::settle`]:
    /// pitch, yaw, roll, `renderInfo.lastHeadAngles`) toward the target's eyes as
    /// the target stands (its own look left out); `None` when it looks at nothing or
    /// has no bones to turn.
    pub fn look(&self, n: usize, time: i32) -> Option<(usize, look::Vec3)> {
        let target = self.instances[n].rig.as_ref()?.looking_at(time)?;
        let goal = match &self.instances[target].rig {
            Some(t) => self.eyes(target, &t.body_pose(time, &[])).0,
            None => self.instances[target].origin,
        };
        Some((target, self.look_toward(n, time, goal)?))
    }

    /// Instance `n`'s settled look at `time` toward `goal` ([`StaticModels::look`]),
    /// whatever it looks at; `None` when it has no bones to turn.
    pub fn look_toward(&self, n: usize, time: i32, goal: Vec3) -> Option<look::Vec3> {
        let rig = self.instances[n].rig.as_ref()?;
        let head = rig.head?;
        let gla = rig.gla(time);
        let locals = gla.locals(&rig.body_frames(time));
        let settled = look::settle(goal, |l| {
            let overrides = head.overrides(gla, l);
            let body = gla.pose_chain(&gla::ROOT, &locals, &overrides, &rig.eye_chain);
            self.eyes(n, &body)
        });
        Some(settled)
    }

    /// The overrides that turn instance `n`'s head by its look at `time`
    /// ([`StaticModels::look`]); none when it looks at nothing.
    fn look_overrides(&self, n: usize, time: i32) -> Vec<(usize, Mat34)> {
        let (Some(rig), Some((_, settled))) = (&self.instances[n].rig, self.look(n, time)) else {
            return Vec::new();
        };
        let head = rig.head.expect("a look turns a head");
        head.overrides(rig.gla(time), settled).to_vec()
    }

    /// The surface posed piece `k` draws, and where its part's bones start in its
    /// instance's [`StaticModels::bones`]; `None` for a piece that is not posed.
    pub fn posed_surface(&self, k: usize) -> Option<(&glm::Surface, usize)> {
        let Source {
            instance,
            part,
            lod,
            surface,
        } = self.pieces[k].source?;
        let rig = self.instances[instance].rig.as_ref()?;
        let (model, first) = rig.part(part);
        Some((&model.glm.lods[lod][surface], first))
    }

    /// The level of detail instance `n` draws in this view, or `None` if culled.
    pub fn visible(
        &self,
        n: usize,
        camera: &Camera,
        frustum: &Frustum,
        view_cluster: i32,
        vis: &WorldVis,
    ) -> Option<usize> {
        let m = &self.instances[n];
        let to_view = [0, 1, 2].map(|i| m.origin[i] - camera.origin[i]);
        let scale = match (&m.link, self.rules) {
            // A game entity: the server's choice, then the renderer's.
            (Some(link), _) => {
                if !vis.sends(view_cluster, link) {
                    return None;
                }
                SP_LOD_SCALE
            }
            (None, Rules::Multiplayer { .. }) => {
                // CG_DrawMiscStaticModels.
                if m.radius != 0.0 && frustum.culls_sphere(m.cull_origin, m.radius) {
                    return None;
                }
                if !vis.can_see(view_cluster, m.cull_cluster) {
                    return None;
                }
                if length(to_view) - m.radius > self.distance_cull {
                    return None;
                }
                LOD_SCALE
            }
            (None, Rules::SinglePlayer { .. }) => {
                // CG_DrawMiscEnts. Its inPVS also needs both points' areas connected
                // (CM_AreasConnected): with every area portal open, as the world is
                // drawn, that is both points being inside the map (a solid leaf has
                // area -1, and these are the leafs without a cluster).
                if view_cluster < 0
                    || m.cull_cluster < 0
                    || !vis.can_see(view_cluster, m.cull_cluster)
                {
                    return None;
                }
                if dot(to_view, to_view) - m.radius > SP_DISTANCE_CULL_SQ {
                    return None;
                }
                SP_LOD_SCALE
            }
        };
        let level = lod(m.lods.len(), m.lod_radius, m.origin, camera, scale);
        // R_AddGhoulSurfaces: the entity's sphere (R_GCullModel).
        if let Some(radius) = m.ghoul2_radius {
            return (!frustum.culls_sphere(m.origin, radius)).then_some(level);
        }
        // R_AddMD3Surfaces: the level's sphere and box (R_CullModel).
        let (center, radius) = m.lods[level].sphere;
        if m.sphere_cull && frustum.culls_sphere(transform(m.origin, &m.axes, center), radius) {
            return None;
        }
        let [mins, maxs] = m.lods[level].bounds;
        let corners: [Vec3; 8] = std::array::from_fn(|i| {
            let v = [
                if i & 1 == 0 { mins[0] } else { maxs[0] },
                if i & 2 == 0 { mins[1] } else { maxs[1] },
                if i & 4 == 0 { mins[2] } else { maxs[2] },
            ];
            transform(m.origin, &m.axes, v)
        });
        (!frustum.culls_points(&corners)).then_some(level)
    }
}

/// A model of either kind, as `RE_RegisterModel` loads it.
enum Model {
    Md3(Md3Model),
    Ghoul2(Arc<Ghoul2Model>),
}

impl Model {
    /// A ".glm" is a Ghoul2 mesh, anything else an MD3.
    fn load(vfs: &Vfs, name: &str) -> Result<Self, swjkja_model::Error> {
        let glm = name
            .get(name.len().saturating_sub(4)..)
            .is_some_and(|ext| ext.eq_ignore_ascii_case(".glm"));
        Ok(if glm {
            Model::Ghoul2(Arc::new(Ghoul2Model::load(vfs, name)?))
        } else {
            Model::Md3(Md3Model::load(vfs, name)?)
        })
    }

    /// The shaders the model registers, in order: an MD3's surfaces' by level from
    /// least detailed to most (`R_LoadMD3`), a Ghoul2 mesh's surfaces' (`R_LoadMDXM`).
    fn shaders(&self) -> Vec<String> {
        match self {
            Model::Md3(m) => m
                .lods
                .iter()
                .rev()
                .flat_map(|md3| &md3.surfaces)
                .flat_map(|s| s.shaders.iter().cloned())
                .collect(),
            Model::Ghoul2(m) => m
                .glm
                .surfaces
                .iter()
                .filter(|s| !s.shader.is_empty())
                .map(|s| s.shader.clone())
                .collect(),
        }
    }
}

/// Loads model `name` once (by its lowercase name), adding its shaders to
/// `shaders` as it registers them; a model that does not load is `None`, with a
/// warning.
fn register_model(
    vfs: &Vfs,
    models: &mut HashMap<String, Option<Model>>,
    shaders: &mut Vec<String>,
    name: &str,
) {
    models
        .entry(name.to_ascii_lowercase())
        .or_insert_with(|| match Model::load(vfs, name) {
            Ok(m) => {
                shaders.extend(m.shaders());
                Some(m)
            }
            Err(e) => {
                eprintln!("warning: model {name}: {e}");
                None
            }
        });
}

/// Loads skin `name` once (by its lowercase name), adding its shaders to `shaders`.
fn register_skin(
    vfs: &Vfs,
    skins: &mut HashMap<String, Option<Skin>>,
    shaders: &mut Vec<String>,
    name: &str,
) {
    skins.entry(name.to_ascii_lowercase()).or_insert_with(|| {
        let skin = Skin::load(vfs, name);
        let surfaces = skin.iter().flat_map(|s| &s.surfaces);
        shaders.extend(surfaces.map(|(_, shader)| shader.clone()));
        skin
    });
}

/// A map's cinematic skeleton ([`animation::cinematic_skeleton`]) and its animation
/// file's text.
pub struct Cinematic {
    gla: Arc<Gla>,
    text: String,
}

impl Cinematic {
    /// The cinematic skeleton of the map at `map`, if it has one that loads.
    fn load(vfs: &Vfs, map: &str) -> Option<Self> {
        let name = animation::cinematic_skeleton(map);
        let file = format!("{name}.gla");
        let data = vfs.read(&file).ok()?;
        let gla = Gla::parse(&data)
            .inspect_err(|e| eprintln!("warning: {file}: {e}"))
            .ok()?;
        let text = animation::config_file(vfs, &name)
            .and_then(|file| vfs.read(&file).ok())
            .map(|text| String::from_utf8_lossy(&text).into_owned())
            .unwrap_or_default();
        Some(Self {
            gla: Arc::new(gla),
            text,
        })
    }
}

/// An NPC's skeleton, the models bolted to it, and what it plays over time.
pub struct Rig {
    body: Arc<Ghoul2Model>,
    /// The map's cinematic skeleton, for a humanoid: skeleton 1, the body's own 0.
    cinematic: Option<Arc<Gla>>,
    /// What its legs and torso play over time ([`script::timeline`]).
    timeline: Timeline,
    /// The animations it can play (those of its timeline, and `BOTH_STAND1`), by
    /// name, those its skeletons have, and the skeleton each plays on.
    anims: Vec<(String, animation::Animation, usize)>,
    /// From when (ms) the body plays on another skeleton, and which.
    switches: Vec<(i32, usize)>,
    /// When a switch of skeleton stops each change of the legs' and the torso's
    /// animation ([`Timeline`]'s), if one does.
    stopped: Stopped,
    /// Bones "model_root", "lower_lumbar" and "Motion", if the skeleton has them.
    bones: [Option<usize>; 3],
    /// The models bolted to it and the body's surfaces they are bolted to.
    attached: Vec<(Arc<Ghoul2Model>, usize)>,
    /// The bones its look turns, if its skeleton has them.
    head: Option<Head>,
    /// Its eyes' tag ("*head_eyes"), if its mesh has one.
    eyes: Option<usize>,
    /// The bones that place the eyes' tag ([`gla::Gla::chain`]), all a look
    /// needs posed.
    eye_chain: Vec<usize>,
    /// The name scripts find it by.
    name: Option<String>,
    /// What it looks at from when (ms): the instance, or `None` for nothing.
    look: Vec<(i32, Option<usize>)>,
}

impl Rig {
    /// `model` standing in `pose` and running its script, its animations from its
    /// animation file and, for a humanoid, `cinematic`'s, which replace those of the
    /// same name (`G_ParseAnimFileSet`). A cinematic skeleton whose bones are not
    /// the body's is left out.
    fn new(
        vfs: &Vfs,
        model: &Arc<Ghoul2Model>,
        pose: &Pose,
        cinematic: Option<&Cinematic>,
    ) -> Self {
        let gla = &model.gla;
        let cinematic = cinematic.filter(|c| {
            model
                .glm
                .anim_name
                .eq_ignore_ascii_case(animation::HUMANOID)
                && c.gla.bones.len() == gla.bones.len()
        });
        let text = animation::config_file(vfs, &model.glm.anim_name)
            .and_then(|file| vfs.read(&file).ok())
            .map(|text| String::from_utf8_lossy(&text).into_owned())
            .unwrap_or_default();
        let find_in = |text: &str, gla: &Gla, name: &str| {
            animation::find(text, name).filter(|a| a.first + a.count <= gla.frame_count)
        };
        // Each animation once: finding one reads the whole file.
        let found = std::cell::RefCell::new(HashMap::new());
        let find = |name: &str| {
            *found
                .borrow_mut()
                .entry(name.to_owned())
                .or_insert_with(|| {
                    let cine =
                        cinematic.and_then(|c| find_in(&c.text, &c.gla, name).map(|a| (a, 1)));
                    cine.or_else(|| find_in(&text, gla, name).map(|a| (a, 0)))
                })
        };
        let holds = |name: &str| {
            find(name).map(|(a, _): (animation::Animation, usize)| {
                let lerp = a.frame_lerp.abs();
                ((a.count as i32 - 1) * lerp, a.count as i32 * lerp)
            })
        };
        let timeline = script::timeline(pose, pose.script.as_deref().map(Vec::as_slice), &holds);
        let mut anims: Vec<(String, animation::Animation, usize)> = Vec::new();
        let changes = timeline.legs.iter().chain(&timeline.torso);
        let names =
            changes.flat_map(|c| [Some(c.anim.as_str()), c.from.as_ref().map(|f| f.0.as_str())]);
        for name in names.flatten().chain([animation::STAND]) {
            if !anims.iter().any(|(n, ..)| n == name)
                && let Some((a, skeleton)) = find(name)
            {
                anims.push((name.to_owned(), a, skeleton));
            }
        }
        let (switches, stopped) = switches(&timeline, |name| find(name).map(|(_, s)| s));
        let eyes = model.glm.find_surface("*head_eyes");
        let eye_chain = eyes.map_or_else(Vec::new, |s| {
            let tag = model.glm.lods[0][s].verts.iter().take(3);
            let bones: Vec<usize> = tag.flat_map(|v| v.bones.iter().map(|b| b.0)).collect();
            gla.chain(&bones)
        });
        Self {
            body: model.clone(),
            cinematic: cinematic.map(|c| c.gla.clone()),
            timeline,
            anims,
            switches,
            stopped,
            bones: ["model_root", "lower_lumbar", "Motion"].map(|b| gla.find_bone(b)),
            attached: Vec::new(),
            head: Head::find(gla),
            eyes,
            eye_chain,
            name: pose.name.clone(),
            look: Vec::new(),
        }
    }

    /// The instance it looks at at `time`, if any.
    fn looking_at(&self, time: i32) -> Option<usize> {
        let after = self.look.partition_point(|(start, _)| *start <= time);
        self.look[..after].last().and_then(|(_, target)| *target)
    }

    /// The skeleton the body plays on at game time `time` (ms): its own, or the
    /// cinematic one from when it plays a cinematic animation.
    fn skeleton(&self, time: i32) -> usize {
        let after = self.switches.partition_point(|(start, _)| *start <= time);
        after.checked_sub(1).map_or(0, |i| self.switches[i].1)
    }

    /// The skeleton the body's bones are posed by at game time `time` (ms).
    fn gla(&self, time: i32) -> &Gla {
        match (self.skeleton(time), &self.cinematic) {
            (1, Some(cinematic)) => cinematic,
            _ => &self.body.gla,
        }
    }

    /// Each bone's frame at game time `time` (ms), each part at the frame its
    /// animation has reached ([`Timeline::at`], `G2_TimingModel`). A part whose
    /// animation the skeletons lack (or a skeleton with no animation file) plays
    /// `BOTH_STAND1` instead, as `NPC_Begin` started it, or stays at frame 0 without
    /// that too (`PM_SetAnimFinal` sets nothing then); so does a part whose bone the
    /// skeleton lacks. A part whose animation a switch of skeleton stopped, or that
    /// plays on the other skeleton, plays its parent bone's frames.
    ///
    /// For [`script::BLEND_MS`] after a part's animation changes, it blends from where
    /// the one before had reached ([`Timeline::blending_at`],
    /// [`animation::Animation::blend_from`]); not into an animation played
    /// backwards (`PM_SetAnimFinal`), nor into one the skeletons lack (nothing
    /// changes then), nor from one a switch of skeleton stopped.
    fn body_frames(&self, time: i32) -> Vec<gla::Blend> {
        let skeleton = self.skeleton(time);
        let find = |name: &str| {
            let found = self.anims.iter().find(|(n, ..)| n == name);
            found.map(|(_, a, s)| (*a, *s))
        };
        let stand = find(animation::STAND);
        let frame = |(frame, next, lerp)| gla::Frame { frame, next, lerp };
        let mut anims = Vec::new();
        for (bone, part) in self.bones.into_iter().zip([0, 1, 1]) {
            let changes = if part == 0 {
                &self.timeline.legs
            } else {
                &self.timeline.torso
            };
            let i = changes
                .partition_point(|c| c.start <= time)
                .saturating_sub(1);
            let (change, stopped) = (&changes[i], &self.stopped[part]);
            let (Some(bone), Some((anim, on))) = (bone, find(&change.anim).or(stand)) else {
                continue;
            };
            if on != skeleton || stopped[i].is_some_and(|s| s <= time) {
                continue;
            }
            let mut blend = gla::Blend::from(frame(anim.frame_at(time - change.start)));
            if let Some((old, old_start, weight)) = self.timeline.blending_at(part, time) {
                let blends = old != change.anim
                    && find(&change.anim).is_some_and(|(a, _)| a.frame_lerp >= 0)
                    && i.checked_sub(1)
                        .is_none_or(|j| stopped[j].is_none_or(|s| s > change.start));
                if let (true, Some((from, _))) = (blends, find(old).or(stand)) {
                    blend.from = frame(from.blend_from(change.start - old_start));
                    blend.weight = weight;
                }
            }
            anims.push((bone, blend));
        }
        self.body.gla.frames(&anims)
    }

    /// The body's pose at game time `time` ([`Rig::body_frames`]), its bones turned
    /// by `overrides`.
    fn body_pose(&self, time: i32, overrides: &[(usize, Mat34)]) -> Vec<Mat34> {
        let gla = self.gla(time);
        gla.pose_overridden(&gla::ROOT, &self.body_frames(time), overrides)
    }

    /// How many bones its parts have.
    pub fn bone_count(&self) -> usize {
        let attached = self.attached.iter().map(|(w, _)| w.gla.bones.len());
        self.body.gla.bones.len() + attached.sum::<usize>()
    }

    /// Part `part`'s model (0 the body, then the bolted models) and where its bones
    /// start among the parts'.
    fn part(&self, part: usize) -> (&Ghoul2Model, usize) {
        let mut first = self.body.gla.bones.len();
        for (w, _) in &self.attached[..part.saturating_sub(1)] {
            first += w.gla.bones.len();
        }
        match part {
            0 => (&self.body, 0),
            p => (&self.attached[p - 1].0, first),
        }
    }

    /// Each part's pose at game time `time` (ms): the body's, its bones turned by
    /// `overrides`, then each bolted model's from where its bolt is.
    fn poses(&self, time: i32, overrides: &[(usize, Mat34)]) -> Vec<Vec<Mat34>> {
        let body = self.body_pose(time, overrides);
        let attached: Vec<_> = self
            .attached
            .iter()
            .map(|(w, s)| {
                // A bolt that held at load holds: it needs only the surface's vertices.
                let root = glm::bolt(&self.body.glm, *s, &body).unwrap_or(gla::IDENTITY);
                attached_pose(w, &root)
            })
            .collect();
        std::iter::once(body).chain(attached).collect()
    }
}

/// When a switch of skeleton stops each of a timeline's changes, per part.
type Stopped = [Vec<Option<i32>>; 2];

/// When the body switches skeletons as `timeline` plays, `skeleton` giving the one
/// each animation plays on (`None` for one the skeletons lack, which sets nothing):
/// from when (ms) it plays on which, and when a switch stops each of the timeline's
/// changes, if one does (per part, legs first, as [`Rig::stopped`]).
///
/// `PM_SetAnimFinal` sets the animation's skeleton before the animation
/// (`G2API_SetAnimIndex`), and setting another skeleton stops every bone's animation;
/// a change on both parts sets the torso first. The body starts on its own skeleton.
fn switches(
    timeline: &Timeline,
    skeleton: impl Fn(&str) -> Option<usize>,
) -> (Vec<(i32, usize)>, Stopped) {
    let parts = [&timeline.legs, &timeline.torso];
    let mut order: Vec<(usize, usize)> = (0..2)
        .flat_map(|part| (0..parts[part].len()).map(move |i| (part, i)))
        .collect();
    order.sort_by_key(|&(part, i)| (parts[part][i].start, part == 0));
    let mut stopped = parts.map(|changes| vec![None; changes.len()]);
    let (mut switches, mut on, mut playing) = (Vec::new(), 0, Vec::<(usize, usize)>::new());
    for (part, i) in order {
        let change = &parts[part][i];
        if let Some(s) = skeleton(&change.anim)
            && s != on
        {
            for (p, j) in playing.drain(..) {
                stopped[p][j] = Some(change.start);
            }
            switches.push((change.start, s));
            on = s;
        }
        playing.push((part, i));
    }
    (switches, stopped)
}

/// A bolted model's pose: frame 0 of its skeleton, from its bolt's matrix `root`.
fn attached_pose(model: &Ghoul2Model, root: &Mat34) -> Vec<Mat34> {
    model.gla.pose_frames(root, &vec![0; model.gla.bones.len()])
}

/// One Ghoul2 model of an entity: its pose and the surfaces it draws.
struct Part<'a> {
    model: &'a Ghoul2Model,
    pose: Vec<Mat34>,
    skin: Option<&'a Skin>,
    drawn: Vec<usize>,
}

impl<'a> Part<'a> {
    /// `model` posed by `pose`, with `skin` on and then `switches`
    /// (`G2API_SetSkin`, `G2_SetSurfaceOnOff`).
    fn new(
        model: &'a Ghoul2Model,
        pose: Vec<Mat34>,
        skin: Option<&'a Skin>,
        switches: &[(String, bool)],
    ) -> Self {
        let glm = &model.glm;
        let mut flags = skin.map_or_else(|| glm.surface_flags(), |skin| glm.skinned_flags(skin));
        for (name, off) in switches {
            glm.set_surface_off(&mut flags, name, *off);
        }
        Self {
            model,
            pose,
            skin,
            drawn: glm.drawn_with(&flags),
        }
    }
}

/// `R_AddGhoulSurfaces` for an entity placed by `p` made of `parts` (its own model
/// first, then the models attached to it): each part's drawn surfaces at its level
/// of detail, with the skin's shader for their name or their own
/// (`RenderSurfaces`). Culled by a sphere of `radius` times its largest scale,
/// leveled by three quarters of that, and fogged by `radius` as seen from outside
/// any fog. A `rigged` entity's pieces keep their [`Source`].
///
/// Each part picks its level from its own number of levels (`G2_ComputeLOD`): with
/// as many entity levels as the least common multiple of the parts' counts, entity
/// level `c` of `n` is level `c * count / n` of a part, the same level as picking
/// from the part's count, as the original does.
fn ghoul2_instance(
    parts: &[Part],
    p: &StaticModel,
    radius: f32,
    entity: usize,
    fogs: &[FogVolume],
    rigged: bool,
    pieces: &mut Vec<Piece>,
) -> Instance {
    let largest = p.scale.into_iter().fold(f32::MIN, f32::max);
    let scaled_radius = radius * if largest == 0.0 { 1.0 } else { largest };
    let fog = sp_fog_num(fogs, p.origin, radius, 0);
    let count = parts
        .iter()
        .map(|part| part.model.glm.lods.len())
        .fold(1, lcm);
    let lods = (0..count)
        .map(|level| {
            let first = pieces.len();
            let mut bounds: [Vec3; 2] = [[f32::MAX; 3], [f32::MIN; 3]];
            for (n, part) in parts.iter().enumerate() {
                let glm = &part.model.glm;
                let lod = level * glm.lods.len() / count;
                let surfaces = &glm.lods[lod];
                for &s in &part.drawn {
                    let surface = &surfaces[s];
                    let (xyz, normals) = glm::skin(surface, &part.pose);
                    for v in &xyz {
                        bounds[0] = [0, 1, 2].map(|k| bounds[0][k].min(v[k]));
                        bounds[1] = [0, 1, 2].map(|k| bounds[1][k].max(v[k]));
                    }
                    let mesh = (
                        &xyz[..],
                        &normals[..],
                        &surface.st[..],
                        &surface.indexes[..],
                    );
                    let info = &glm.surfaces[s];
                    let shader = part
                        .skin
                        .and_then(|skin| skin.shader(&info.name))
                        .unwrap_or(&info.shader);
                    let source = rigged.then_some(Source {
                        instance: entity - 1,
                        part: n,
                        lod,
                        surface: s,
                    });
                    pieces.push(bake(mesh, shader, p, entity, fog, source));
                }
            }
            ModelLod {
                bounds,
                sphere: ([0.0; 3], scaled_radius),
                pieces: first..pieces.len(),
            }
        })
        .collect();
    Instance {
        origin: p.origin,
        axes: p.axes,
        cull_origin: p.origin,
        light_origin: p.origin,
        sphere_cull: true,
        cull_cluster: -1,
        entity_box: p.entity_box,
        link: None,
        radius: scaled_radius,
        lod_radius: 0.75 * scaled_radius,
        ghoul2_radius: Some(scaled_radius),
        lods,
        rig: None,
    }
}

/// Bone or tag matrix `b` of instance `m`, from its model's space to the world's:
/// after the instance's axes and origin.
fn place(m: &Instance, b: &Mat34) -> Mat34 {
    std::array::from_fn(|i| {
        let t = [0, 1, 2].map(|k| m.axes[k][i]);
        let mut r: [f32; 4] =
            std::array::from_fn(|j| t[0] * b[0][j] + t[1] * b[1][j] + t[2] * b[2][j]);
        r[3] += m.origin[i];
        r
    })
}

/// Finds what each NPC's look targets name (`Q3_LookTarget`: `G_Find`, ignoring
/// case): the first NPC of that name. A name that finds none changes nothing, as
/// the original's, which only logs it.
fn resolve_looks(instances: &mut [Instance]) {
    let names: Vec<(usize, String)> = instances
        .iter()
        .enumerate()
        .filter_map(|(i, m)| Some((i, m.rig.as_ref()?.name.clone()?)))
        .collect();
    for m in instances.iter_mut() {
        let Some(rig) = &mut m.rig else { continue };
        let mut look: Vec<(i32, Option<usize>)> = Vec::new();
        for (start, name) in &rig.timeline.look {
            let target = match name {
                None => None,
                Some(name) => match names.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)) {
                    Some(&(i, _)) => Some(i),
                    None => continue,
                },
            };
            look.push((*start, target));
        }
        rig.look = look;
    }
}

fn lcm(a: usize, b: usize) -> usize {
    let gcd = |mut a: usize, mut b: usize| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    a / gcd(a, b) * b
}

/// A surface's positions, normals, texture coordinates and triangles.
type Mesh<'a> = (&'a [Vec3], &'a [Vec3], &'a [[f32; 2]], &'a [u32]);

/// One posed surface in world space (`RB_SurfaceMesh`, `RB_SurfaceGhoul`), with white
/// vertex colours and the entity number for the shader's entity table.
fn bake(
    (xyz, normals, st, indexes): Mesh,
    shader: &str,
    p: &StaticModel,
    entity: usize,
    fog: usize,
    source: Option<Source>,
) -> Piece {
    let verts: Vec<DrawVert> = xyz
        .iter()
        .zip(normals)
        .zip(st)
        .map(|((&v, &n), &st)| DrawVert {
            xyz: transform(p.origin, &p.axes, v),
            st,
            lightmap: [[0.0; 2], [0.0; 2], [0.0; 2], [0.0, entity as f32]],
            normal: transform([0.0; 3], &p.axes, n),
            color: [[255; 4]; 4],
        })
        .collect();
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    for v in &verts {
        bounds[0] = [0, 1, 2].map(|k| bounds[0][k].min(v.xyz[k]));
        bounds[1] = [0, 1, 2].map(|k| bounds[1][k].max(v.xyz[k]));
    }
    Piece {
        shader: shader.to_owned(),
        fog,
        geometry: Geometry::Triangles {
            verts,
            indexes: indexes.to_vec(),
            bounds,
        },
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fog(bounds: [Vec3; 2], color: [f32; 3]) -> FogVolume {
        FogVolume {
            color: [0; 4],
            parms_color: color,
            tc_scale: 0.0,
            surface: [0.0; 4],
            bounds,
        }
    }

    #[test]
    fn single_player_fog_prefers_a_whole_fit_then_the_view_fog() {
        // A sphere of radius 10 at the origin: its box is -10..10.
        let red = [1.0, 0.0, 0.0];
        let blue = [0.0, 0.0, 1.0];
        // Fog 1 holds the box's low corner only, fog 2 its high corner only, fog 3
        // the whole box.
        let low = fog([[-20.0; 3], [0.0; 3]], red);
        let high = fog([[0.0; 3], [20.0; 3]], red);
        let whole = fog([[-50.0; 3], [50.0; 3]], red);
        let fogs = [low, high, whole];
        // From outside any fog: neither partial fog matches fog 0's zero colour, so
        // the whole fit wins.
        assert_eq!(sp_fog_num(&fogs, [0.0; 3], 10.0, 0), 3);
        // From inside fog 2 (or one of the same colour): fog 1 already matches.
        assert_eq!(sp_fog_num(&fogs, [0.0; 3], 10.0, 2), 1);
        // A blue fog has fog 0's red and green (zero): it matches from outside.
        let fogs = [fog([[-20.0; 3], [0.0; 3]], blue), high];
        assert_eq!(sp_fog_num(&fogs, [0.0; 3], 10.0, 0), 1);
        // Only partial fits of other colours: the first of them.
        let fogs = [high, low];
        assert_eq!(sp_fog_num(&fogs, [0.0; 3], 10.0, 0), 1);
        // Out of every fog.
        assert_eq!(sp_fog_num(&fogs, [100.0; 3], 10.0, 0), 0);
    }

    fn changes(list: &[(&str, i32)]) -> Vec<script::Change> {
        list.iter()
            .map(|&(anim, start)| script::Change {
                start,
                anim: anim.to_owned(),
                from: None,
            })
            .collect()
    }

    #[test]
    fn playing_on_another_skeleton_stops_every_part() {
        let skeleton = |anim: &str| match anim {
            "BOTH_CIN_2" | "BOTH_CIN_4" => Some(1),
            "MISSING" => None,
            _ => Some(0),
        };
        // Both parts into a cinematic animation and back: the switch to it stops
        // both standings, the one back both cinematic animations.
        let timeline = Timeline {
            legs: changes(&[
                ("BOTH_STAND1", 0),
                ("BOTH_CIN_2", 1550),
                ("BOTH_STAND1", 9000),
            ]),
            torso: changes(&[
                ("BOTH_STAND1", 0),
                ("BOTH_CIN_2", 1550),
                ("BOTH_STAND1", 9000),
            ]),
            look: Vec::new(),
        };
        let (at, stopped) = switches(&timeline, skeleton);
        assert_eq!(at, [(1550, 1), (9000, 0)]);
        let both = vec![Some(1550), Some(9000), None];
        assert_eq!(stopped, [both.clone(), both]);
        // The torso alone: the legs' standing stops too; a cinematic animation
        // after another on the same skeleton switches nothing, nor does one the
        // skeletons lack.
        let timeline = Timeline {
            legs: changes(&[("BOTH_STAND1", 0)]),
            torso: changes(&[
                ("BOTH_STAND1", 0),
                ("BOTH_CIN_2", 1550),
                ("BOTH_CIN_4", 3000),
                ("MISSING", 4000),
            ]),
            look: Vec::new(),
        };
        let (at, stopped) = switches(&timeline, skeleton);
        assert_eq!(at, [(1550, 1)]);
        assert_eq!(
            stopped,
            [vec![Some(1550)], vec![Some(1550), None, None, None]]
        );
        // On both parts at once, the torso is set first: the legs' change of
        // skeleton stops the torso's.
        let timeline = Timeline {
            legs: changes(&[("BOTH_CIN_2", 0)]),
            torso: changes(&[("BOTH_STAND1", 0)]),
            look: Vec::new(),
        };
        let (at, stopped) = switches(&timeline, skeleton);
        assert_eq!(at, [(0, 1)]);
        assert_eq!(stopped, [vec![None], vec![Some(0)]]);
    }

    #[test]
    fn radius_reaches_the_farthest_corner() {
        assert_eq!(radius_from_bounds([[-3.0, 0.0, 0.0], [1.0, 4.0, 0.0]]), 5.0);
    }

    #[test]
    fn level_of_detail_follows_projected_size() {
        let camera = Camera::new([0.0; 3], [0.0; 3], 640, 480);
        // 80 degrees across at 4:3: tan(half vertical fov) = 0.629.
        let ty = camera.tan_half_fov().1;
        let at = |d: f32| lod(3, 10.0, [d, 0.0, 0.0], &camera, LOD_SCALE);
        // Projected radius 1/5 or more: flod <= 0, the most detail.
        assert_eq!(at(10.0 / (0.2 * ty)), 0);
        // Projected radius 0.1: flod 0.5, times 3 levels: level 1.
        assert_eq!(at(10.0 / (0.1 * ty)), 1);
        // Far away: the least detail.
        assert_eq!(at(1.0e6), 2);
        // Behind the viewer: projected to nothing, the most detail.
        assert_eq!(at(-100.0), 0);
        // One level: always it.
        assert_eq!(lod(1, 10.0, [1.0e6, 0.0, 0.0], &camera, LOD_SCALE), 0);
    }
}
