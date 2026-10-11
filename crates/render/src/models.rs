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
//! (`CG_Player`): their skin on, their surfaces switched, posed as they start
//! (`BOTH_STAND1`), culled and leveled by their radius times their largest scale, and
//! lit at their origin.
//!
//! The models never move, so each placement's surfaces are baked into world space at
//! load; their normals carry the axes' scale, which is what the original's lighting
//! sees (`R_SetupEntityLighting` turns the light into the model's scaled axes).

use std::collections::HashMap;
use std::ops::Range;

use swjkja_bsp::{Bsp, DrawVert};
use swjkja_fs::Vfs;
use swjkja_model::animation;
use swjkja_model::gla::{self, Mat34};
use swjkja_model::glm::{self, Ghoul2Model};
use swjkja_model::md3::{Md3, Md3Model};
use swjkja_model::skin::Skin;
use swjkja_world::Geometry;
use swjkja_world::npc;
use swjkja_world::static_model::{Pose, Rules, StaticModel};

use crate::camera::Camera;
use crate::fog::FogVolume;
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
    /// state, then its static models) and bakes each placement (call
    /// [`StaticModels::locate`] before drawing). A model that does not load stops the
    /// original's map load; here its placements are skipped with a warning.
    ///
    /// A single-player model's fog is the one it has seen from outside any fog. One
    /// retail model's fog depends on where the view is (taspir1's Raven's Claw, from
    /// inside fog 1); a retail-data test checks there are no others.
    pub fn load(vfs: &Vfs, bsp: &Bsp, fogs: &[FogVolume], rules: Rules) -> Self {
        let sp = matches!(rules, Rules::SinglePlayer { .. });
        let mut placements = swjkja_world::static_model::entity_models(&bsp.entities, rules);
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
            placements.extend(npc::npc_models(&bsp.entities, rules, &parms, &sabers));
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
                    let body = Part::new(
                        m,
                        body_pose(vfs, m, g.pose.as_ref()),
                        skin(&g.skin),
                        &g.switches,
                    );
                    let mut parts = Vec::new();
                    for a in &g.attached {
                        // G_CreateG2AttachedWeaponModel: nothing without the model or
                        // the bolt.
                        let Some(Model::Ghoul2(w)) = &models[&a.model.to_ascii_lowercase()] else {
                            continue;
                        };
                        let Some(root) = m
                            .glm
                            .find_surface(a.bolt)
                            .and_then(|s| glm::bolt(&m.glm, s, &body.pose))
                        else {
                            continue;
                        };
                        let pose = w.gla.pose_frames(&root, &vec![0; w.gla.bones.len()]);
                        parts.push(Part::new(w, pose, skin(&a.skin), &[]));
                    }
                    parts.insert(0, body);
                    let entity = instances.len() + 1;
                    instances.push(ghoul2_instance(
                        &parts,
                        p,
                        g.radius,
                        entity,
                        fogs,
                        &mut pieces,
                    ));
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
                        pieces.push(bake(mesh, shader, p, entity, fog));
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
            });
        }
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
    Ghoul2(Ghoul2Model),
}

impl Model {
    /// A ".glm" is a Ghoul2 mesh, anything else an MD3.
    fn load(vfs: &Vfs, name: &str) -> Result<Self, swjkja_model::Error> {
        let glm = name
            .get(name.len().saturating_sub(4)..)
            .is_some_and(|ext| ext.eq_ignore_ascii_case(".glm"));
        Ok(if glm {
            Model::Ghoul2(Ghoul2Model::load(vfs, name)?)
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

/// The pose of a Ghoul2 entity's skeleton: frame 0, or standing in `pose` with
/// the frames its animations start at in the skeleton's animation file. A part
/// whose animation the skeleton lacks (or a skeleton with no animation file) stays
/// as `NPC_Begin` started it, in `BOTH_STAND1`, or at frame 0 without that too
/// (`PM_SetAnimFinal` sets nothing then); so does a part whose bone the skeleton
/// lacks.
fn body_pose(vfs: &Vfs, model: &Ghoul2Model, pose: Option<&Pose>) -> Vec<Mat34> {
    let gla = &model.gla;
    let Some(pose) = pose else {
        return gla.pose(0);
    };
    let text = animation::config_file(vfs, &model.glm.anim_name)
        .and_then(|file| vfs.read(&file).ok())
        .map(|text| String::from_utf8_lossy(&text).into_owned())
        .unwrap_or_default();
    let start = |anim| animation::start_frame(&text, anim).filter(|&frame| frame < gla.frame_count);
    let stand = start(animation::STAND);
    let mut anims = Vec::new();
    for (bone, anim) in [
        ("model_root", pose.legs),
        ("lower_lumbar", pose.torso),
        ("Motion", pose.torso),
    ] {
        if let (Some(bone), Some(frame)) = (gla.find_bone(bone), start(anim).or(stand)) {
            anims.push((bone, frame));
        }
    }
    gla.pose_frames(&gla::ROOT, &gla.frames(&anims))
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
/// any fog.
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
            for part in parts {
                let glm = &part.model.glm;
                let surfaces = &glm.lods[level * glm.lods.len() / count];
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
                    pieces.push(bake(mesh, shader, p, entity, fog));
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
