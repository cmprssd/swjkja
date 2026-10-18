//! The world drawn the way the original draws it: every surface of the world model
//! with its resolved material ([`swjkja_shader::resolve`]), stage by stage, in the
//! original's order (materials by sort value, then by creation order; each material's
//! surfaces stage by stage).
//!
//! The map's static models ([`crate::models`]) join the world: their surfaces are
//! drawn with the world's, each with its entity's light.
//!
//! Not yet: normal and text deforms, distance LOD
//! for patches, visibility culling, video maps (drawn white), noise waves (their
//! base value), fog on sky layers, oriented and effect surface sprites.

use std::collections::HashMap;
use std::sync::Arc;

use ash::vk;
use bytemuck::{Pod, Zeroable};
use gpu_allocator::MemoryLocation;
use swjkja_bsp::Bsp;
use swjkja_fs::Vfs;
use swjkja_model::gla::Mat34;
use swjkja_shader::resolve::{
    FogAdjust, FogColor, FogPass, Image, Images, Material, MaterialStage, TexEnv,
    resolve_map_shader,
};
use swjkja_shader::{
    AlphaGen, AlphaTest, Cull, Deform, DepthTest, DstBlend, Func, RgbGen, ShaderText, SpriteKind,
    SrcBlend, TcGen, TexMod,
};
use swjkja_world::{Geometry, World};

use crate::camera::Camera;
use crate::deform;
use crate::fog;
use crate::frames::FRAMES_IN_FLIGHT;
use crate::gpu::{Gpu, Result};
use crate::models::StaticModels;
use crate::pipeline::{Pipeline, State, spirv};
use crate::resource::{Buffer, Frame};
use crate::sky;
use crate::sprites;
use crate::texture::{Texture, TextureSet};

/// `R_FindImageFile`: the image `name` resolves to, or `None` if none decodes or its
/// sides are not powers of two (the original refuses those as if missing).
fn find_image(vfs: &Vfs, name: &str) -> Option<swjkja_image::Image> {
    let image = swjkja_image::load(vfs, name).ok()?.image;
    (image.width.is_power_of_two() && image.height.is_power_of_two()).then_some(image)
}
use crate::vis;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    xyz: [f32; 3],
    normal: [f32; 3],
    st: [f32; 2],
    lightmap: [[f32; 2]; 4],
    color: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameData {
    view_proj: [[f32; 4]; 4],
    view_origin: [f32; 4],
    params: [f32; 4],
    fog_color: [f32; 4],
    misc: [u32; 4],
    view_forward: [f32; 4],
    styles: [[f32; 4]; swjkja_world::style::MAX_LIGHT_STYLES],
}

/// A map fog for the shader's `fogs` table (see `Fog` in world.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuFog {
    color: [f32; 4],
    surface: [f32; 4],
    params: [f32; 4],
}

/// Mirrors `Stage` in shaders/world.wgsl (std430, 480 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuStage {
    modes: [u32; 4],
    tcgen: [u32; 4],
    funcs: [u32; 4],
    tcmod_kind: [[u32; 4]; 2],
    constant: [f32; 4],
    rgb_wave: [f32; 4],
    alpha_wave: [f32; 4],
    tc_vector: [[f32; 4]; 4],
    tcmod: [[f32; 4]; 16],
    style: [f32; 4],
    fog: [u32; 4],
}

/// An entity's light and colour for the shader's `entities` table (see `Entity` in
/// world.wgsl); entity 0 is the world.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuEntity {
    ambient: [f32; 4],
    directed: [f32; 4],
    light_dir: [f32; 4],
    color: [f32; 4],
}

impl GpuEntity {
    /// The world: no light grid light, colour white.
    const WORLD: Self = Self {
        ambient: [0.0; 4],
        directed: [0.0; 4],
        light_dir: [0.0; 4],
        color: [255.0; 4],
    };
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Push {
    stage: u32,
    texture0: u32,
    texture1: u32,
    /// How the vertices are moved (see `vs_main` in world.wgsl): 0 not, 1 skinned by
    /// their bones (posed models), 2 + n turned by bone n (turning brush entities).
    bones: u32,
}

/// What a bundle samples: one slot, or animation frames in consecutive slots.
#[derive(Debug, Clone)]
enum Slots {
    One(u32),
    Animated {
        first: u32,
        count: u32,
        fps: f32,
        one_shot: bool,
    },
}

impl Slots {
    /// Held at the first frame (`RF_SETANIMINDEX` with frame 0).
    fn first(self) -> Self {
        match self {
            Slots::Animated { first, .. } => Slots::One(first),
            one => one,
        }
    }

    /// `R_BindAnimatedImage`: the frame at `time` seconds.
    fn at(&self, time: f32) -> u32 {
        match *self {
            Slots::One(s) => s,
            Slots::Animated {
                first,
                count,
                fps,
                one_shot,
            } => {
                let index = ((time * fps * 1024.0) as i32 >> 10).max(0) as u32;
                first
                    + if one_shot {
                        index.min(count - 1)
                    } else {
                        index % count
                    }
            }
        }
    }
}

#[derive(Clone)]
struct DrawStage {
    gpu_stage: u32,
    pipeline: usize,
    slots: [Slots; 2],
}

struct DrawMaterial {
    first_index: u32,
    index_count: u32,
    stages: Vec<DrawStage>,
    /// For materials with deforms: geometry rebuilt every frame.
    dynamic: Option<Dynamic>,
    /// Static geometry whose index list is rebuilt each frame from the visible
    /// surfaces (see draw()).
    culled: bool,
    /// Sky draws: drawn only if a surface of this sky gate is visible.
    gate: Option<usize>,
    /// How the frame's bones move its vertices ([`Push::bones`], see draw()).
    bones: u32,
}

/// Which vertex and index buffers a draw uses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bound {
    /// Built once (sky).
    Static,
    /// Static vertices, indexes of the visible surfaces.
    Visible,
    /// Rebuilt every frame (deforms, flares).
    Dynamic,
}

/// Geometry rebuilt every frame, and where it goes in the dynamic buffers.
struct Dynamic {
    source: DynamicSource,
    vertex_offset: u32,
    index_offset: u32,
}

enum DynamicSource {
    /// A material with deforms: its base geometry.
    Deformed {
        verts: Vec<deform::DeformVertex>,
        indexes: Vec<u32>,
        deforms: Vec<swjkja_shader::Deform>,
    },
    /// Flare surfaces (surface, origin, normal), and the flare radius (the
    /// shader's portalRange, or 30).
    Flares {
        flares: Vec<(usize, [f32; 3], [f32; 3])>,
        radius: f32,
    },
    /// The NPCs' shadows in fog `fog`: the polygons in that fog, from the shadows'
    /// shared region ([`Shadows`]), filled each frame (see draw()).
    Shadows { fog: usize },
    /// A surface sprite stage's sprites over the visible ones of its surfaces, at
    /// most `capacity`.
    Sprites {
        sprites: swjkja_shader::SurfaceSprites,
        additive: bool,
        surfaces: Vec<SpriteSurface>,
        capacity: usize,
    },
}

/// A world surface that scatters sprites: its number and triangles.
struct SpriteSurface {
    surface: usize,
    verts: Vec<sprites::SpriteVertex>,
    indexes: Vec<u32>,
}

/// The NPCs' blob shadows (`CG_PlayerShadow` with `cg_shadows 1`): what they are
/// traced against and laid on, and their region of the dynamic buffers (each
/// shadow material's vertices start at `first_vertex`).
struct Shadows {
    world: swjkja_world::collision::Collision,
    marks: swjkja_world::mark::MarkSurfaces,
    first_vertex: u32,
    first_index: u32,
    max_vertices: usize,
    /// Per instance: the shadow last laid, from where and facing which way, and
    /// its pieces with their fogs (laid again only when either changes).
    laid: Vec<Option<Laid>>,
}

/// One NPC's shadow: its origin and yaw, and its pieces, each with its fog.
struct Laid {
    from: ([f32; 3], f32),
    pieces: Vec<(usize, Vec<swjkja_world::mark::MarkVert>)>,
}

/// A material's identity: its shader, lightmap pages and styles, and whether its
/// animated images hold their first frame (see [`BrushDraw::shader_anim`]).
type MaterialKey = (String, [i32; 4], [u8; 4], bool);

/// A brush entity the renderer draws, as the map starts (see
/// `swjkja_world::brush_entity`).
struct BrushDraw {
    /// Its model's bounds' corners in the world (`R_CullLocalBox`).
    corners: [[f32; 3]; 8],
    entity_box: [[f32; 3]; 2],
    /// Where the server links its box (set once the map's visibility is loaded).
    link: Option<vis::EntityLink>,
    broadcast: bool,
    /// `EF_SHADER_ANIM`: its surfaces use materials whose animated images hold
    /// frame 0.
    shader_anim: bool,
    /// Its model's surfaces (map surface numbers), placed where it stands, or in its
    /// model's space if it turns.
    surfaces: Vec<usize>,
    /// A turning or sliding one: placed each frame.
    turning: Option<Turning>,
}

/// A brush entity that turns or slides as time passes, drawn by a bone of the
/// frame's bones placing its model's space where it stands (`R_RotateForEntity`).
struct Turning {
    /// The entity, without its "model2" (drawn by the static models, where it
    /// spawns) and door, so that a copy placed each frame allocates nothing.
    entity: swjkja_world::brush_entity::BrushEntity,
    /// Its model's bounds.
    bounds: [[f32; 3]; 2],
    /// Its bone in the frame's bones.
    bone: usize,
}

pub struct WorldRenderer {
    gpu: Arc<Gpu>,
    vertices: Buffer,
    indexes: Buffer,
    /// What each frame rewrites, one copy per frame in flight ([`Frame::slot`]).
    per_frame: Vec<PerFrame>,
    _stage_buffer: Buffer,
    _fog_buffer: Buffer,
    /// The map's light styles, animated each frame.
    styles: swjkja_world::style::LightStyles,
    frame_layout: vk::DescriptorSetLayout,
    frame_pool: vk::DescriptorPool,
    pipelines: Vec<Pipeline>,
    /// In drawing order.
    materials: Vec<DrawMaterial>,
    pub triangles: usize,
    pub material_count: usize,
    /// The global fog's colour and GL fog density, if the map has one.
    fog: Option<([f32; 3], f32)>,
    /// Which surfaces each view draws.
    vis: vis::WorldVis,
    /// Per surface: its culled draw material and index range in `static_indexes`.
    surface_draw: Vec<Option<(usize, u32, u32)>>,
    /// The brush entities drawn, in spawn order.
    brushes: Vec<BrushDraw>,
    /// Per surface: the sky it opens (drawn only when one of its surfaces is visible).
    surface_gate: Vec<Option<usize>>,
    gate_count: usize,
    /// The static index list on the CPU (the visible ranges are copied from it).
    static_indexes: Vec<u32>,
    /// The map's static models; their surfaces follow the map's in `surface_draw`.
    models: StaticModels,
    /// Where the models' surfaces start in `surface_draw`.
    first_model_surface: usize,
    /// Per model: where its bones start in the frame's bones, if it is posed.
    first_bone: Vec<Option<usize>>,
    light_grid: swjkja_world::light_grid::LightGrid,
    /// The map's fog volumes, by fog number - 1.
    fog_volumes: Vec<fog::FogVolume>,
    /// The NPCs' shadows, if the map has NPCs.
    shadows: Option<Shadows>,
    /// The field of view surface sprites were first drawn with
    /// ([`sprites::range_scale`]).
    sprite_fov: Option<f32>,
    /// Whether the map is drawn by the single-player renderer, whose environment
    /// mapping differs from multiplayer's.
    single_player: bool,
}

/// One frame's data: its uniforms, deformed geometry and visible index ranges, and
/// the descriptor set with its uniforms (and the shared stage and fog tables).
struct PerFrame {
    frame_buffer: Buffer,
    frame_set: vk::DescriptorSet,
    dynamic_vertices: Buffer,
    dynamic_indexes: Buffer,
    culled_indexes: Buffer,
    /// The posed models' bones (see `Bone` in world.wgsl).
    bones: Buffer,
    /// The entity table: [`GpuEntity`] per entity.
    entities: Buffer,
}

/// The vertex attributes of [`Vertex`].
const ATTRIBUTES: [(u32, vk::Format, u32); 6] = [
    (0, vk::Format::R32G32B32_SFLOAT, 0),
    (1, vk::Format::R32G32B32_SFLOAT, 12),
    (2, vk::Format::R32G32_SFLOAT, 24),
    (3, vk::Format::R32G32B32A32_SFLOAT, 32),
    (4, vk::Format::R32G32B32A32_SFLOAT, 48),
    (5, vk::Format::R8G8B8A8_UNORM, 64),
];

/// Culling. Quake 3 winds its polygons opposite to OpenGL's default, so "front-sided"
/// materials cull what OpenGL calls front faces (`GL_Cull`). The GPU test
/// `quake_front_faces_survive_front_sided_culling` pins this mapping for Vulkan.
pub const FRONT_FACE: vk::FrontFace = vk::FrontFace::COUNTER_CLOCKWISE;

/// The Vulkan cull mode for a material's `cull` setting.
pub fn cull_mode(cull: Cull) -> vk::CullModeFlags {
    match cull {
        Cull::Front => vk::CullModeFlags::FRONT,
        Cull::Back => vk::CullModeFlags::BACK,
        Cull::TwoSided => vk::CullModeFlags::NONE,
    }
}

fn src_factor(b: SrcBlend) -> vk::BlendFactor {
    match b {
        SrcBlend::Zero => vk::BlendFactor::ZERO,
        SrcBlend::One => vk::BlendFactor::ONE,
        SrcBlend::DstColor => vk::BlendFactor::DST_COLOR,
        SrcBlend::OneMinusDstColor => vk::BlendFactor::ONE_MINUS_DST_COLOR,
        SrcBlend::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
        SrcBlend::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        SrcBlend::DstAlpha => vk::BlendFactor::DST_ALPHA,
        SrcBlend::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
        SrcBlend::AlphaSaturate => vk::BlendFactor::SRC_ALPHA_SATURATE,
    }
}

fn dst_factor(b: DstBlend) -> vk::BlendFactor {
    match b {
        DstBlend::Zero => vk::BlendFactor::ZERO,
        DstBlend::One => vk::BlendFactor::ONE,
        DstBlend::SrcColor => vk::BlendFactor::SRC_COLOR,
        DstBlend::OneMinusSrcColor => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
        DstBlend::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
        DstBlend::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        DstBlend::DstAlpha => vk::BlendFactor::DST_ALPHA,
        DstBlend::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
    }
}

fn state(m: &Material, s: &MaterialStage) -> State {
    let blend = match (s.blend_src, s.blend_dst) {
        (None, None) => None,
        // A blend with only one factor makes the original stop with an error; this
        // draws it with the missing factor's neutral value instead.
        (src, dst) => Some((
            src.map_or(vk::BlendFactor::ONE, src_factor),
            dst.map_or(vk::BlendFactor::ZERO, dst_factor),
        )),
    };
    State {
        blend,
        depth_write: s.depth_write,
        depth_compare: match s.depth_test {
            DepthTest::LessEqual => Some(vk::CompareOp::GREATER_OR_EQUAL),
            DepthTest::Equal => Some(vk::CompareOp::EQUAL),
            DepthTest::Disabled => None,
        },
        cull: cull_mode(m.cull),
        front_face: FRONT_FACE,
        depth_bias: m.polygon_offset,
        // The original's framebuffer has no alpha channel (24-bit colour), so
        // destination alpha always reads 1: alpha is never written, and cleared to 1.
        write_mask: vk::ColorComponentFlags::R
            | vk::ColorComponentFlags::G
            | vk::ColorComponentFlags::B,
    }
}

fn func(f: Func) -> u32 {
    match f {
        Func::Sin => 0,
        Func::Square => 1,
        Func::Triangle => 2,
        Func::Sawtooth => 3,
        Func::InverseSawtooth => 4,
        Func::Noise => 5,
        Func::Random => 6,
    }
}

fn wave_params(w: &swjkja_shader::Wave) -> [f32; 4] {
    [w.base, w.amplitude, w.phase, w.frequency]
}

fn gpu_stage(s: &MaterialStage, env: Option<TexEnv>) -> GpuStage {
    let mut g = GpuStage::zeroed();
    let (rgb, rgb_wave_func, rgb_wave) = match s.rgb_gen {
        RgbGen::Bad => (0, 0, [0.0; 4]),
        RgbGen::IdentityLighting => (1, 0, [0.0; 4]),
        RgbGen::Identity => (2, 0, [0.0; 4]),
        RgbGen::Entity => (3, 0, [0.0; 4]),
        RgbGen::OneMinusEntity => (4, 0, [0.0; 4]),
        RgbGen::ExactVertex => (5, 0, [0.0; 4]),
        RgbGen::Vertex => (6, 0, [0.0; 4]),
        RgbGen::OneMinusVertex => (7, 0, [0.0; 4]),
        RgbGen::Wave(w) => (8, func(w.func), wave_params(&w)),
        RgbGen::LightingDiffuse => (9, 0, [0.0; 4]),
        RgbGen::LightingDiffuseEntity => (10, 0, [0.0; 4]),
        RgbGen::Const => (11, 0, [0.0; 4]),
        RgbGen::LightmapStyle => (12, 0, [0.0; 4]),
    };
    let (alpha, alpha_wave_func, alpha_wave) = match s.alpha_gen {
        AlphaGen::Identity => (0, 0, [0.0; 4]),
        AlphaGen::Skip => (1, 0, [0.0; 4]),
        AlphaGen::Entity => (2, 0, [0.0; 4]),
        AlphaGen::OneMinusEntity => (3, 0, [0.0; 4]),
        AlphaGen::Vertex => (4, 0, [0.0; 4]),
        AlphaGen::OneMinusVertex => (5, 0, [0.0; 4]),
        AlphaGen::LightingSpecular => (6, 0, [0.0; 4]),
        AlphaGen::Wave(w) => (7, func(w.func), wave_params(&w)),
        AlphaGen::Portal => (8, 0, [0.0; 4]),
        AlphaGen::Const => (9, 0, [0.0; 4]),
        // Dot and oneMinusDot belong to RMG terrain; the world never uses them.
        AlphaGen::Dot | AlphaGen::OneMinusDot => (1, 0, [0.0; 4]),
    };
    let alpha_test = match s.alpha_test {
        AlphaTest::None => 0,
        AlphaTest::Gt0 => 1,
        AlphaTest::Lt128 => 2,
        AlphaTest::Ge128 => 3,
        AlphaTest::Ge192 => 4,
    };
    let env = match env {
        Some(_) if s.bundle2.is_none() => 0,
        Some(TexEnv::Modulate) => 1,
        Some(TexEnv::Add) => 2,
        None => 0,
    };
    g.modes = [rgb, alpha, env, alpha_test];
    g.constant = s.constant.map(f32::from);
    g.rgb_wave = rgb_wave;
    g.alpha_wave = alpha_wave;
    g.style = [f32::from(s.lightmap_style), 0.0, 0.0, 0.0];
    let bundles = [Some(&s.bundle), s.bundle2.as_ref()];
    let mut clamps = [0u32; 2];
    for (b, bundle) in bundles.iter().enumerate() {
        let Some(bundle) = bundle else { continue };
        let (generator, set) = match bundle.tc_gen {
            TcGen::Lightmap => (1, bundle.lightmap_set as u32),
            TcGen::Environment => (2, 0),
            TcGen::Vector([sv, tv]) => {
                g.tc_vector[b * 2] = [sv[0], sv[1], sv[2], 0.0];
                g.tc_vector[b * 2 + 1] = [tv[0], tv[1], tv[2], 0.0];
                (3, 0)
            }
            TcGen::Texture | TcGen::Default => (0, 0),
        };
        g.tcgen[b * 2] = generator;
        g.tcgen[b * 2 + 1] = set;
        clamps[b] = u32::from(match &bundle.image {
            Image::Named { clamp, .. } | Image::Animated { clamp, .. } => *clamp,
            Image::Lightmap(_) => true,
            _ => false,
        });
        for (i, m) in bundle.tc_mods.iter().take(4).enumerate() {
            let (kind, a, bb) = match *m {
                TexMod::None => (0, [0.0; 4], [0.0; 4]),
                TexMod::Turbulent(w) => (1, wave_params(&w), [0.0; 4]),
                TexMod::Scale(xy) => (2, [xy[0], xy[1], 0.0, 0.0], [0.0; 4]),
                TexMod::Scroll(xy) => (3, [xy[0], xy[1], 0.0, 0.0], [0.0; 4]),
                TexMod::Stretch(w) => (4, wave_params(&w), [func(w.func) as f32, 0.0, 0.0, 0.0]),
                TexMod::Transform { matrix, translate } => (
                    5,
                    [matrix[0][0], matrix[0][1], matrix[1][0], matrix[1][1]],
                    [translate[0], translate[1], 0.0, 0.0],
                ),
                TexMod::Rotate(dps) => (6, [dps, 0.0, 0.0, 0.0], [0.0; 4]),
                // The world entity's shaderTexCoord is zero: no movement.
                TexMod::EntityTranslate => (7, [0.0; 4], [0.0; 4]),
            };
            g.tcmod_kind[b][i] = kind;
            g.tcmod[b * 8 + i * 2] = a;
            g.tcmod[b * 8 + i * 2 + 1] = bb;
        }
    }
    g.funcs = [rgb_wave_func, alpha_wave_func, clamps[0], clamps[1]];
    g
}

impl WorldRenderer {
    /// Resolves and uploads everything the world model needs, for the map at `map`
    /// (as "maps/academy2.bsp"; [`StaticModels::load`]). Textures go into
    /// `textures`, which must outlive the renderer. NPCs are posed at each draw's
    /// time; a surface of theirs with vertex deforms would stand as it does at game
    /// time `time` (seconds), but no retail one has any.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gpu: &Arc<Gpu>,
        textures: &mut TextureSet,
        vfs: &Vfs,
        shaders: &ShaderText,
        bsp: &Bsp,
        world: &World,
        map: &str,
        time: f32,
    ) -> Result<Self> {
        // The global fog (the one without a brush; fog numbers are 1-based) and its
        // parameters, from its shader's fogParms (red at 250 units if it has none, as
        // in R_LoadFogs).
        let global_fog = bsp.fogs.iter().position(|f| f.brush == -1).map(|i| i + 1);
        let parms_of = |n: usize| {
            let name = swjkja_shader::resolve::strip_extension(bsp.fogs[n - 1].shader());
            match shaders.parse(name) {
                Some(Ok(def)) if def.fog_parms.is_some() => def.fog_parms.unwrap(),
                _ => swjkja_shader::FogParms {
                    color: [1.0, 0.0, 0.0],
                    depth_for_opaque: 250.0,
                },
            }
        };
        let fog_parms = global_fog.map(parms_of);
        // The fogs table, indexed by fog number (entry 0 unused).
        let fog_volumes: Vec<fog::FogVolume> = (1..=bsp.fogs.len())
            .map(|n| fog::FogVolume::load(bsp, n - 1, &parms_of(n)))
            .collect();
        let mut gpu_fogs = vec![GpuFog::zeroed()];
        for f in &fog_volumes {
            gpu_fogs.push(GpuFog {
                color: f.color.map(f32::from),
                surface: f.surface,
                params: [f.tc_scale, 0.0, 0.0, 0.0],
            });
        }

        let rules = swjkja_world::static_model::Rules::for_map(map);
        // Game time in whole milliseconds, as cg.time.
        let mut models = StaticModels::load(
            vfs,
            bsp,
            &fog_volumes,
            map,
            (f64::from(time) * 1000.0).round() as i32,
        );

        // Images: decode once, keep the decoded pixels to upload on first use. Every
        // image the map's shaders can ask for is decoded up front, in parallel; a name
        // missed here decodes when asked.
        let mut decoded: HashMap<String, Option<swjkja_image::Image>> = HashMap::new();
        {
            let mut names: Vec<String> = Vec::new();
            let map_shaders = bsp.shaders.iter().map(|s| s.name());
            for shader in map_shaders.chain(models.shaders.iter().map(String::as_str)) {
                let stripped = swjkja_shader::resolve::strip_extension(shader);
                names.push(stripped.to_owned());
                if let Some(Ok(def)) = shaders.parse(stripped) {
                    for stage in &def.stages {
                        match &stage.texture {
                            swjkja_shader::Texture::Map { name, .. } => names.push(name.clone()),
                            swjkja_shader::Texture::Animated { names: frames, .. } => {
                                names.extend(frames.iter().cloned())
                            }
                            _ => {}
                        }
                    }
                }
            }
            names.sort_unstable();
            names.dedup();
            let images = crate::parallel::map(&names, |name| find_image(vfs, name));
            decoded.extend(names.into_iter().zip(images));
        }
        struct Loader<'a> {
            vfs: &'a Vfs,
            decoded: &'a mut HashMap<String, Option<swjkja_image::Image>>,
        }
        impl Images for Loader<'_> {
            fn exists(&mut self, name: &str) -> bool {
                let vfs = self.vfs;
                self.decoded
                    .entry(name.to_owned())
                    .or_insert_with(|| find_image(vfs, name))
                    .is_some()
            }
        }

        // The brush entities the map spawns, each surface of their models placed
        // where the entity stands (a model two entities share is drawn by the first),
        // the doors NPCs stand in opening as they begin.
        let surfaces_in = |model: usize| {
            let m = &bsp.models[model];
            m.first_surface.max(0) as usize
                ..(m.first_surface.max(0) + m.num_surfaces.max(0)) as usize
        };
        // A turning or sliding one's surfaces stay in its model's space; its bones follow the
        // posed models' (`first_bone`).
        let mut placed: HashMap<usize, Geometry> = HashMap::new();
        let mut brushes: Vec<BrushDraw> = Vec::new();
        // Per turning entity's surface: its bone among the turning entities'.
        let mut turned_by: HashMap<usize, usize> = HashMap::new();
        let mut turning_count = 0;
        let mut entities =
            swjkja_world::brush_entity::brush_entities(&bsp.entities, &bsp.models, rules);
        swjkja_world::brush_entity::touch_doors(
            &mut entities,
            &models.bodies,
            swjkja_world::script::FIRST_FRAME,
        );
        for e in entities {
            let surfaces: Vec<usize> = surfaces_in(e.model)
                .filter(|i| *i < world.surfaces.len() && !placed.contains_key(i))
                .collect();
            let m = &bsp.models[e.model];
            let turning = (e.spin.is_some() || e.slide.is_some()).then(|| {
                turning_count += 1;
                turning_count - 1
            });
            for &i in &surfaces {
                let geometry = &world.surfaces[i].geometry;
                placed.insert(
                    i,
                    match turning {
                        Some(_) => geometry.clone(),
                        None => e.placed(geometry),
                    },
                );
                if let Some(bone) = turning {
                    turned_by.insert(i, bone);
                }
            }
            brushes.push(BrushDraw {
                corners: e.corners([m.mins, m.maxs]),
                link: None,
                entity_box: e.entity_box,
                broadcast: e.broadcast,
                shader_anim: e.shader_anim,
                surfaces,
                turning: turning.map(|bone| Turning {
                    bounds: [m.mins, m.maxs],
                    bone,
                    entity: swjkja_world::brush_entity::BrushEntity {
                        model2: None,
                        door: None,
                        ..e
                    },
                }),
            });
        }

        // Materials, in creation order (the order surfaces first use them).
        let mut material_index: HashMap<MaterialKey, usize> = HashMap::new();
        let mut materials: Vec<Material> = Vec::new();
        // Per material: whether its animated images hold their first frame.
        let mut held: Vec<bool> = Vec::new();
        // Per material, its surfaces by fog number (the original's sort key orders draws
        // by material, then fog).
        let mut surfaces_of: Vec<std::collections::BTreeMap<usize, Vec<usize>>> = Vec::new();
        let mut surface_cull = vec![vis::SurfaceCull::Never; bsp.surfaces.len()];
        let entity_surfaces = brushes
            .iter()
            .flat_map(|b| b.surfaces.iter().map(|&i| (i, b.shader_anim)));
        for (i, hold) in surfaces_in(0).map(|i| (i, false)).chain(entity_surfaces) {
            let s = &world.surfaces[i];
            let geometry = placed.get(&i).unwrap_or(&s.geometry);
            if matches!(geometry, Geometry::Skip) {
                continue;
            }
            let shader_name = bsp.shaders[s.shader].name().to_owned();
            let styles = if s.lightmaps[0] == swjkja_world::LIGHTMAP_BY_VERTEX {
                s.vertex_styles
            } else {
                s.lightmap_styles
            };
            let key = (shader_name.clone(), s.lightmaps, styles, hold);
            let m = match material_index.get(&key) {
                Some(&m) => m,
                None => {
                    let mut loader = Loader {
                        vfs,
                        decoded: &mut decoded,
                    };
                    let material = if s.default_shader {
                        Material::default_material()
                    } else {
                        resolve_map_shader(
                            shaders,
                            &shader_name,
                            s.lightmaps,
                            styles,
                            bsp.lightmap_pages(),
                            &mut loader,
                            &mut |_| false,
                        )
                    };
                    materials.push(material);
                    held.push(hold);
                    surfaces_of.push(Default::default());
                    material_index.insert(key, materials.len() - 1);
                    materials.len() - 1
                }
            };
            surfaces_of[m].entry(s.fog).or_default().push(i);
            // R_CullSurface's per-surface test, with the surface's material.
            surface_cull[i] = match geometry {
                Geometry::Face { normal, dist, .. } if materials[m].cull != Cull::TwoSided => {
                    vis::SurfaceCull::Plane {
                        normal: *normal,
                        dist: *dist,
                        front_sided: materials[m].cull == Cull::Front,
                    }
                }
                Geometry::Grid(g) => vis::SurfaceCull::Bounds {
                    mins: g.mesh_bounds[0],
                    maxs: g.mesh_bounds[1],
                },
                Geometry::Triangles { bounds, .. } => vis::SurfaceCull::Bounds {
                    mins: bounds[0],
                    maxs: bounds[1],
                },
                _ => vis::SurfaceCull::Never,
            };
        }

        // The NPCs' shadows' material, as cgame registers it once the world has
        // loaded ("markShadow", no lightmap, default styles), with a draw for each fog
        // number its polygons can take.
        let npcs = models.instances.iter().filter(|m| m.rig.is_some()).count();
        let shadow_material = (npcs > 0).then(|| {
            let mut loader = Loader {
                vfs,
                decoded: &mut decoded,
            };
            let lightmaps = [swjkja_shader::resolve::LIGHTMAP_NONE; 4];
            let styles = [0, 255, 255, 255];
            materials.push(resolve_map_shader(
                shaders,
                SHADOW_SHADER,
                lightmaps,
                styles,
                bsp.lightmap_pages(),
                &mut loader,
                &mut |_| false,
            ));
            held.push(false);
            surfaces_of.push((0..=bsp.fogs.len()).map(|f| (f, Vec::new())).collect());
            let m = materials.len() - 1;
            material_index.insert((SHADOW_SHADER.to_owned(), lightmaps, styles, false), m);
            m
        });

        // The models' materials, in the order the models register their shaders
        // (R_LoadMD3: no lightmap, default styles), then their surfaces.
        let world_materials = materials.len();
        let first_model_surface = bsp.surfaces.len();
        let model_key = |name: &str| {
            (
                name.to_owned(),
                [swjkja_shader::resolve::LIGHTMAP_NONE; 4],
                [0, 255, 255, 255],
                false,
            )
        };
        let names = models.shaders.iter().map(String::as_str);
        for name in names.chain(models.pieces.iter().map(|p| p.shader.as_str())) {
            let key = model_key(name);
            if material_index.contains_key(&key) {
                continue;
            }
            let mut loader = Loader {
                vfs,
                decoded: &mut decoded,
            };
            materials.push(resolve_map_shader(
                shaders,
                &key.0,
                key.1,
                key.2,
                bsp.lightmap_pages(),
                &mut loader,
                &mut |_| false,
            ));
            held.push(false);
            surfaces_of.push(Default::default());
            material_index.insert(key, materials.len() - 1);
        }
        for (k, piece) in models.pieces.iter().enumerate() {
            let m = material_index[&model_key(&piece.shader)];
            surfaces_of[m]
                .entry(piece.fog)
                .or_default()
                .push(first_model_surface + k);
        }
        let geometry = |i: usize| match i.checked_sub(first_model_surface) {
            Some(k) => &models.pieces[k].geometry,
            None => placed.get(&i).unwrap_or(&world.surfaces[i].geometry),
        };
        let surface_count = first_model_surface + models.pieces.len();

        // Textures.
        let default_image: Vec<u8> = (0..16 * 16)
            .flat_map(|i| {
                let (x, y) = (i % 16, i / 16);
                if x == 0 || y == 0 || x == 15 || y == 15 {
                    [255; 4]
                } else {
                    [32; 4]
                }
            })
            .collect();
        let default_slot = textures.add(Texture::new(gpu, 16, 16, &default_image, true)?);
        let white = textures.add(Texture::new(gpu, 8, 8, &[255; 8 * 8 * 4], true)?);
        let fog_image = textures.add(Texture::new(
            gpu,
            fog::FOG_S as u32,
            fog::FOG_T as u32,
            &fog::fog_image(),
            false,
        )?);
        let uploads = std::cell::RefCell::new(crate::texture::Uploads::default());
        let decoded = std::cell::RefCell::new(decoded);
        let lightmaps: Vec<u32> = bsp
            .lightmaps
            .chunks(swjkja_bsp::LIGHTMAP_BYTES)
            .map(|page| {
                let rgba: Vec<u8> = page
                    .chunks(3)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect();
                Ok(textures.add(Texture::queued(
                    gpu,
                    128,
                    128,
                    rgba,
                    false,
                    &mut uploads.borrow_mut(),
                )?))
            })
            .collect::<Result<_>>()?;
        let mut named: HashMap<String, u32> = HashMap::new();
        let mut upload = |name: &str, textures: &mut TextureSet| -> Result<u32> {
            if let Some(&slot) = named.get(name) {
                return Ok(slot);
            }
            // Each name uploads once (`named`), so its decoded pixels can move.
            let cached = decoded.borrow_mut().remove(name).flatten();
            let slot = match cached.or_else(|| find_image(vfs, name)) {
                Some(img) => textures.add(Texture::queued(
                    gpu,
                    img.width,
                    img.height,
                    img.rgba,
                    true,
                    &mut uploads.borrow_mut(),
                )?),
                None => default_slot,
            };
            named.insert(name.to_owned(), slot);
            Ok(slot)
        };
        let mut slots_for = |image: &Image, textures: &mut TextureSet| -> Result<Slots> {
            Ok(match image {
                Image::Named { name, .. } => Slots::One(upload(name, textures)?),
                Image::Lightmap(n) => Slots::One(lightmaps.get(*n).copied().unwrap_or(white)),
                Image::White | Image::Video(_) | Image::None => Slots::One(white),
                Image::Default => Slots::One(default_slot),
                Image::Animated {
                    frames_per_second,
                    names,
                    one_shot,
                    ..
                } => {
                    // Frames need consecutive slots: upload them in order, fresh.
                    let first = textures.len() as u32;
                    for name in names {
                        // Frames may repeat across materials: copy, don't take.
                        let cached = decoded.borrow().get(name).cloned().flatten();
                        let img = cached.or_else(|| find_image(vfs, name));
                        match img {
                            Some(img) => textures.add(Texture::queued(
                                gpu,
                                img.width,
                                img.height,
                                img.rgba,
                                true,
                                &mut uploads.borrow_mut(),
                            )?),
                            None => textures.add(Texture::new(gpu, 16, 16, &default_image, true)?),
                        };
                    }
                    Slots::Animated {
                        first,
                        count: names.len().max(1) as u32,
                        fps: *frames_per_second,
                        one_shot: *one_shot,
                    }
                }
            })
        };

        // Frame descriptor set: uniform frame data and the stage table.
        let d = &gpu.device;
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX),
            vk::DescriptorSetLayoutBinding::default()
                .binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX),
        ];
        let frame_layout = unsafe {
            d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?
        };

        // Geometry, stages and pipelines, in drawing order.
        let mut order: Vec<usize> = (0..materials.len()).collect();
        order.sort_by(|&a, &b| {
            materials[a]
                .sort
                .total_cmp(&materials[b].sort)
                .then(a.cmp(&b))
        });
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indexes: Vec<u32> = Vec::new();
        let mut gpu_stages: Vec<GpuStage> = Vec::new();
        let mut states: Vec<State> = Vec::new();
        let mut draw_materials = Vec::new();
        // Per surface: its draw material and index range in the static index list,
        // or its sky gate.
        let mut surface_draw: Vec<Option<(usize, u32, u32)>> = vec![None; surface_count];
        let mut surface_gate: Vec<Option<usize>> = vec![None; surface_count];
        let mut gates = 0usize;
        let (mut dynamic_vertices, mut dynamic_indexes) = (0u32, 0u32);
        // Each posed model's bones, one after another.
        let mut bone_count = 0;
        let first_bone: Vec<Option<usize>> = models
            .instances
            .iter()
            .map(|m| {
                let first = bone_count;
                bone_count += m.rig.as_ref()?.bone_count();
                Some(first)
            })
            .collect();
        // The shadows' region: an NPC's shadow has at most MAX_MARK_POINTS points,
        // each piece drawn as a fan (fewer indexes than three per point).
        let shadows = shadow_material.map(|_| {
            let max_vertices = npcs * swjkja_world::mark::MAX_MARK_POINTS;
            let shadows = Shadows {
                world: swjkja_world::collision::Collision::new(bsp),
                marks: swjkja_world::mark::MarkSurfaces::new(surfaces_in(0).map(|i| {
                    let shader = &bsp.shaders[world.surfaces[i].shader];
                    (
                        &world.surfaces[i],
                        (shader.surface_flags, shader.content_flags),
                    )
                })),
                first_vertex: dynamic_vertices,
                first_index: dynamic_indexes,
                max_vertices,
                laid: Vec::new(),
            };
            dynamic_vertices += max_vertices as u32;
            dynamic_indexes += max_vertices as u32 * 3;
            shadows
        });
        let first_turning_bone = bone_count;
        bone_count += turning_count;
        for b in &mut brushes {
            if let Some(t) = &mut b.turning {
                t.bone += first_turning_bone;
            }
        }
        for &m in &order {
            let material = &materials[m];
            if let Some(sky) = &material.sky {
                let gate = gates;
                gates += 1;
                for group in surfaces_of[m].values() {
                    for &i in group {
                        surface_gate[i] = Some(gate);
                    }
                }
                // RB_StageIteratorSky: the outer box, then the cloud layers drawn with
                // the material's stages; the sky surfaces themselves are not drawn.
                // (The original draws only the box blocks behind visible sky polygons;
                // this draws the whole box, which differs only through holes in a map.)
                let sky_state = State {
                    blend: None,
                    depth_write: false,
                    depth_compare: Some(vk::CompareOp::GREATER_OR_EQUAL),
                    cull: vk::CullModeFlags::NONE,
                    front_face: FRONT_FACE,
                    depth_bias: false,
                    write_mask: vk::ColorComponentFlags::R
                        | vk::ColorComponentFlags::G
                        | vk::ColorComponentFlags::B,
                };
                let pipeline_for = |st: State, states: &mut Vec<State>| {
                    states.iter().position(|x| *x == st).unwrap_or_else(|| {
                        states.push(st);
                        states.len() - 1
                    })
                };
                let push_sky = |verts: &[sky::SkyVertex],
                                idx: &[u32],
                                vertices: &mut Vec<Vertex>,
                                indexes: &mut Vec<u32>| {
                    let base = vertices.len() as u32;
                    vertices.extend(verts.iter().map(|&(xyz, st)| Vertex {
                        xyz,
                        normal: [0.0; 3],
                        st,
                        lightmap: [[0.0; 2]; 4],
                        color: [255; 4],
                    }));
                    let first = indexes.len() as u32;
                    indexes.extend(idx.iter().map(|&i| base + i));
                    (first, idx.len() as u32)
                };
                // The outer box, unless its first image is missing (then the original
                // has the default image there and skips the box).
                if let Some(outer) = &sky.outer_box {
                    let mut slots: Vec<Option<Image>> = Vec::new();
                    for suffix in sky::SIDE_SUFFIXES {
                        let name = format!("{outer}_{suffix}");
                        let image = swjkja_image::load(vfs, &name)
                            .is_ok()
                            .then_some(Image::Named { name, clamp: true });
                        // A missing side uses the previous side's image.
                        let image = image.or_else(|| slots.last().cloned().flatten());
                        slots.push(image);
                    }
                    if slots[0].is_some() {
                        let mut stage = GpuStage::zeroed();
                        stage.modes = [1, 0, 0, 0]; // identity lighting: tr.identityLight
                        stage.funcs[2] = 1; // clamp
                        stage.fog[1] = 1;
                        gpu_stages.push(stage);
                        let gpu_stage = gpu_stages.len() as u32 - 1;
                        let pipeline = pipeline_for(sky_state, &mut states);
                        for (axis, image) in slots.iter().enumerate() {
                            let (verts, idx) = sky::box_side(axis);
                            let (first_index, index_count) =
                                push_sky(&verts, &idx, &mut vertices, &mut indexes);
                            let slot = slots_for(image.as_ref().unwrap(), textures)?;
                            draw_materials.push(DrawMaterial {
                                first_index,
                                index_count,
                                stages: vec![DrawStage {
                                    gpu_stage,
                                    pipeline,
                                    slots: [slot, Slots::One(white)],
                                }],
                                dynamic: None,
                                culled: false,
                                gate: Some(gate),
                                bones: 0,
                            });
                        }
                    }
                }
                if sky.cloud_height != 0.0 && !material.stages.is_empty() {
                    let first_index = indexes.len() as u32;
                    for axis in 0..5 {
                        let (verts, idx) = sky::cloud_side(axis, sky.cloud_height);
                        push_sky(&verts, &idx, &mut vertices, &mut indexes);
                    }
                    let index_count = indexes.len() as u32 - first_index;
                    let mut stages = Vec::new();
                    for st in &material.stages {
                        let mut g = gpu_stage(st, material.multitexture);
                        g.fog[1] = 1;
                        gpu_stages.push(g);
                        let state = State {
                            cull: vk::CullModeFlags::NONE,
                            ..state(material, st)
                        };
                        let slot1 = match &st.bundle2 {
                            Some(b) => slots_for(&b.image, textures)?,
                            None => Slots::One(white),
                        };
                        stages.push(DrawStage {
                            gpu_stage: gpu_stages.len() as u32 - 1,
                            pipeline: pipeline_for(state, &mut states),
                            slots: [slots_for(&st.bundle.image, textures)?, slot1],
                        });
                    }
                    draw_materials.push(DrawMaterial {
                        first_index,
                        index_count,
                        stages,
                        dynamic: None,
                        culled: false,
                        gate: Some(gate),
                        bones: 0,
                    });
                }
                continue;
            }
            // Not drawn yet: fog volumes.
            if material.stages.is_empty() {
                continue;
            }
            const FORCESIGHT: u32 = 0x200_0000;
            if material.surface_flags & FORCESIGHT != 0 {
                continue; // hidden unless force sight is on
            }
            let deformed = material.deforms.iter().any(|d| {
                matches!(
                    d,
                    Deform::Wave { .. }
                        | Deform::Bulge { .. }
                        | Deform::Move { .. }
                        | Deform::AutoSprite
                        | Deform::AutoSprite2
                )
            });
            for (&fog, group) in &surfaces_of[m] {
                // The posed models' surfaces, then each turning brush entity's,
                // drawn just after the others (first index, index count, bones).
                let mut moved_draws: Vec<(u32, u32, u32)> = Vec::new();
                let flares: Vec<(usize, [f32; 3], [f32; 3])> = group
                    .iter()
                    .filter_map(|&i| match *geometry(i) {
                        Geometry::Flare { origin, normal, .. } => Some((i, origin, normal)),
                        _ => None,
                    })
                    .collect();
                let (first_index, index_count, dynamic) = if let (Some(s), true) =
                    (&shadows, Some(m) == shadow_material)
                {
                    let dynamic = Dynamic {
                        vertex_offset: s.first_vertex,
                        index_offset: s.first_index,
                        source: DynamicSource::Shadows { fog },
                    };
                    (0, 0, Some(dynamic))
                } else if !flares.is_empty() {
                    // Built each frame from the view (RB_SurfaceFlare).
                    let n = flares.len() as u32;
                    let dynamic = Dynamic {
                        vertex_offset: dynamic_vertices,
                        index_offset: dynamic_indexes,
                        source: DynamicSource::Flares {
                            radius: material.portal_range.filter(|r| *r != 0.0).unwrap_or(30.0),
                            flares,
                        },
                    };
                    dynamic_vertices += n * 4;
                    dynamic_indexes += n * 6;
                    (0, 0, Some(dynamic))
                } else if deformed {
                    // Kept on the CPU and rebuilt each frame (see draw()).
                    let mut dv = Vec::new();
                    let mut di = Vec::new();
                    for &i in group {
                        append_deformable(geometry(i), &mut dv, &mut di);
                    }
                    if di.is_empty() {
                        continue;
                    }
                    // Deforms keep the vertex and index counts (autoSprite rebuilds
                    // whole quads, 4 vertices and 6 indexes each).
                    let (nv, ni) = (
                        dv.len() as u32,
                        (di.len() as u32).max(dv.len() as u32 / 4 * 6),
                    );
                    let dynamic = Dynamic {
                        vertex_offset: dynamic_vertices,
                        index_offset: dynamic_indexes,
                        source: DynamicSource::Deformed {
                            verts: dv,
                            indexes: di,
                            deforms: material.deforms.clone(),
                        },
                    };
                    dynamic_vertices += nv;
                    dynamic_indexes += ni;
                    (0, 0, Some(dynamic))
                } else {
                    let posed = |i: usize| {
                        let k = i.checked_sub(first_model_surface)?;
                        let (surface, first) = models.posed_surface(k)?;
                        let instance = models.pieces[k].source?.instance;
                        Some((surface, first_bone[instance]? + first, instance + 1))
                    };
                    let (moving, fixed): (Vec<usize>, Vec<usize>) =
                        group.iter().partition(|&&i| posed(i).is_some());
                    let (turned, fixed): (Vec<usize>, Vec<usize>) =
                        fixed.iter().partition(|i| turned_by.contains_key(i));
                    let first_index = indexes.len() as u32;
                    for &i in &fixed {
                        let start = indexes.len() as u32;
                        append(geometry(i), &mut vertices, &mut indexes);
                        surface_draw[i] =
                            Some((draw_materials.len(), start, indexes.len() as u32 - start));
                    }
                    let count = indexes.len() as u32 - first_index;
                    let skinned_first = indexes.len() as u32;
                    for &i in &moving {
                        let (surface, first, entity) = posed(i).unwrap();
                        let start = indexes.len() as u32;
                        append_skinned(surface, first, entity, &mut vertices, &mut indexes);
                        surface_draw[i] = Some((
                            draw_materials.len() + 1,
                            start,
                            indexes.len() as u32 - start,
                        ));
                    }
                    let skinned_count = indexes.len() as u32 - skinned_first;
                    if skinned_count != 0 {
                        moved_draws.push((skinned_first, skinned_count, 1));
                    }
                    let mut by_bone: std::collections::BTreeMap<usize, Vec<usize>> =
                        Default::default();
                    for &i in &turned {
                        by_bone.entry(turned_by[&i]).or_default().push(i);
                    }
                    for (bone, surfaces) in by_bone {
                        let first = indexes.len() as u32;
                        let at = draw_materials.len() + 1 + moved_draws.len();
                        for i in surfaces {
                            let start = indexes.len() as u32;
                            append(geometry(i), &mut vertices, &mut indexes);
                            surface_draw[i] = Some((at, start, indexes.len() as u32 - start));
                        }
                        let bone = first_turning_bone + bone;
                        moved_draws.push((first, indexes.len() as u32 - first, 2 + bone as u32));
                    }
                    if count == 0 && moved_draws.is_empty() {
                        continue;
                    }
                    (first_index, count, None)
                };
                let is_flare = matches!(
                    dynamic.as_ref().map(|d| &d.source),
                    Some(DynamicSource::Flares { .. })
                );
                // GL fog for the global fog on fogged (opaque and see-through) materials;
                // a fog pass for the other fogs (RB_FogPass, with r_drawfog 2).
                let gl_fog =
                    fog != 0 && Some(fog) == global_fog && material.fog_pass != FogPass::None;
                let fog_pass =
                    fog != 0 && Some(fog) != global_fog && material.fog_pass != FogPass::None;
                let mut stages = Vec::new();
                // Surface sprite stages are drawn as sprites (below), not on the
                // surfaces.
                for s in material
                    .stages
                    .iter()
                    .filter(|s| s.surface_sprites.is_none())
                {
                    let st = state(material, s);
                    let pipeline = states.iter().position(|x| *x == st).unwrap_or_else(|| {
                        states.push(st);
                        states.len() - 1
                    });
                    let mut g = gpu_stage(s, material.multitexture);
                    if m >= world_materials {
                        g.style[1] = 1.0; // a model's: lit by its entity
                    }
                    g.fog[2] = u32::from(is_flare);
                    g.fog[3] = fog as u32
                        | (match s.fog_adjust {
                            FogAdjust::None => 0,
                            FogAdjust::ModulateRgb => 1,
                            FogAdjust::ModulateAlpha => 2,
                            FogAdjust::ModulateRgba => 3,
                        }) << 16;
                    if gl_fog {
                        g.fog[0] = match s.fog_color {
                            FogColor::Fog => 1,
                            FogColor::Black => 2,
                            FogColor::White => 3,
                        };
                    }
                    gpu_stages.push(g);
                    let slot1 = match &s.bundle2 {
                        Some(b) => slots_for(&b.image, textures)?,
                        None => Slots::One(white),
                    };
                    let mut slots = [slots_for(&s.bundle.image, textures)?, slot1];
                    if held[m] {
                        slots = slots.map(Slots::first);
                    }
                    stages.push(DrawStage {
                        gpu_stage: gpu_stages.len() as u32 - 1,
                        pipeline,
                        slots,
                    });
                }
                if fog_pass {
                    // RB_FogPass: the fog image over the surface, coloured with the fog
                    // (alpha blended, no depth write; depth equal for opaque materials).
                    let mut g = GpuStage::zeroed();
                    g.modes = [13, 1, 0, 0];
                    g.tcgen = [4, 0, 0, 0];
                    g.funcs = [0, 0, 1, 0];
                    g.fog = [0, 0, u32::from(is_flare), fog as u32];
                    gpu_stages.push(g);
                    let st = State {
                        blend: Some((
                            vk::BlendFactor::SRC_ALPHA,
                            vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
                        )),
                        depth_write: false,
                        depth_compare: Some(if material.fog_pass == FogPass::Equal {
                            vk::CompareOp::EQUAL
                        } else {
                            vk::CompareOp::GREATER_OR_EQUAL
                        }),
                        cull: cull_mode(material.cull),
                        front_face: FRONT_FACE,
                        depth_bias: material.polygon_offset,
                        write_mask: vk::ColorComponentFlags::R
                            | vk::ColorComponentFlags::G
                            | vk::ColorComponentFlags::B,
                    };
                    stages.push(DrawStage {
                        gpu_stage: gpu_stages.len() as u32 - 1,
                        pipeline: states.iter().position(|x| *x == st).unwrap_or_else(|| {
                            states.push(st);
                            states.len() - 1
                        }),
                        slots: [Slots::One(fog_image), Slots::One(white)],
                    });
                }
                let moved_stages = stages.clone();
                draw_materials.push(DrawMaterial {
                    first_index,
                    index_count,
                    stages,
                    culled: dynamic.is_none(),
                    dynamic,
                    gate: None,
                    bones: 0,
                });
                for (first_index, index_count, bones) in moved_draws {
                    draw_materials.push(DrawMaterial {
                        first_index,
                        index_count,
                        stages: moved_stages.clone(),
                        dynamic: None,
                        culled: true,
                        gate: None,
                        bones,
                    });
                }
                // Surface sprites (RB_DrawSurfaceSprites, after the surfaces' stages):
                // each sprite stage but the first, over the world's own surfaces
                // (not brush entities' or models', nor deformed ones), coloured by
                // their vertices, textured whole, both sides drawn.
                for s in material.stages.iter().skip(1) {
                    let Some(ss) = s.surface_sprites else {
                        continue;
                    };
                    if !matches!(ss.kind, SpriteKind::Vertical | SpriteKind::Flattened) || deformed
                    {
                        continue;
                    }
                    let surfaces: Vec<SpriteSurface> = group
                        .iter()
                        .filter(|&&i| i < first_model_surface && !placed.contains_key(&i))
                        .map(|&i| {
                            let (mut v, mut indexes) = (Vec::new(), Vec::new());
                            append(geometry(i), &mut v, &mut indexes);
                            let verts = v
                                .iter()
                                .map(|v| sprites::SpriteVertex {
                                    xyz: v.xyz,
                                    normal_z: v.normal[2],
                                    light: v.color[2],
                                })
                                .collect();
                            SpriteSurface {
                                surface: i,
                                verts,
                                indexes,
                            }
                        })
                        .collect();
                    let capacity: usize = surfaces
                        .iter()
                        .map(|t| sprites::capacity(&ss, &t.verts, &t.indexes))
                        .sum();
                    if capacity == 0 {
                        continue;
                    }
                    let sprite_stage = MaterialStage {
                        bundle: swjkja_shader::resolve::Bundle {
                            tc_gen: TcGen::Texture,
                            tc_mods: Vec::new(),
                            ..s.bundle.clone()
                        },
                        bundle2: None,
                        rgb_gen: RgbGen::ExactVertex,
                        alpha_gen: AlphaGen::Vertex,
                        ..s.clone()
                    };
                    let st = State {
                        cull: vk::CullModeFlags::NONE,
                        depth_bias: false,
                        ..state(material, &sprite_stage)
                    };
                    let pipeline = states.iter().position(|x| *x == st).unwrap_or_else(|| {
                        states.push(st);
                        states.len() - 1
                    });
                    let mut g = gpu_stage(&sprite_stage, None);
                    if gl_fog {
                        g.fog[0] = match s.fog_color {
                            FogColor::Fog => 1,
                            FogColor::Black => 2,
                            FogColor::White => 3,
                        };
                    }
                    gpu_stages.push(g);
                    let mut slot = slots_for(&s.bundle.image, textures)?;
                    if held[m] {
                        slot = slot.first();
                    }
                    draw_materials.push(DrawMaterial {
                        first_index: 0,
                        index_count: 0,
                        stages: vec![DrawStage {
                            gpu_stage: gpu_stages.len() as u32 - 1,
                            pipeline,
                            slots: [slot, Slots::One(white)],
                        }],
                        dynamic: Some(Dynamic {
                            vertex_offset: dynamic_vertices,
                            index_offset: dynamic_indexes,
                            source: DynamicSource::Sprites {
                                sprites: ss,
                                additive: (s.blend_src, s.blend_dst)
                                    == (Some(SrcBlend::One), Some(DstBlend::One)),
                                surfaces,
                                capacity,
                            },
                        }),
                        culled: false,
                        gate: None,
                        bones: 0,
                    });
                    dynamic_vertices += capacity as u32 * 4;
                    dynamic_indexes += capacity as u32 * 6;
                }
            }
        }
        let layouts = [textures.layout, frame_layout];
        let pipelines = states
            .iter()
            .map(|&st| {
                Pipeline::with_state(
                    gpu,
                    spirv::WORLD_VERT,
                    spirv::WORLD_FRAG,
                    size_of::<Vertex>() as u32,
                    &ATTRIBUTES,
                    size_of::<Push>() as u32,
                    &layouts,
                    st,
                )
            })
            .collect::<Result<Vec<_>>>()?;

        let stage_buffer = Buffer::with_data(
            gpu,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            bytemuck::cast_slice(&gpu_stages),
        )?;
        let fog_buffer = Buffer::with_data(
            gpu,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            bytemuck::cast_slice(&gpu_fogs),
        )?;
        let sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: FRAMES_IN_FLIGHT as u32,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 4 * FRAMES_IN_FLIGHT as u32,
            },
        ];
        let frame_pool = unsafe {
            d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(FRAMES_IN_FLIGHT as u32)
                    .pool_sizes(&sizes),
                None,
            )?
        };
        let frame_sets = unsafe {
            d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(frame_pool)
                    .set_layouts(&[frame_layout; FRAMES_IN_FLIGHT]),
            )?
        };
        let mut per_frame = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for frame_set in frame_sets {
            let frame_buffer = Buffer::new(
                gpu,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                size_of::<FrameData>() as u64,
                MemoryLocation::CpuToGpu,
            )?;
            let info = |b: &Buffer| {
                [vk::DescriptorBufferInfo {
                    buffer: b.buffer,
                    offset: 0,
                    range: vk::WHOLE_SIZE,
                }]
            };
            let entities = Buffer::new(
                gpu,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                ((models.instances.len() + 1) * size_of::<GpuEntity>()) as u64,
                MemoryLocation::CpuToGpu,
            )?;
            let bones = Buffer::new(
                gpu,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                (bone_count.max(1) * size_of::<Mat34>()) as u64,
                MemoryLocation::CpuToGpu,
            )?;
            let (ubo, ssbo, fog_info, entity_info, bone_info) = (
                info(&frame_buffer),
                info(&stage_buffer),
                info(&fog_buffer),
                info(&entities),
                info(&bones),
            );
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(frame_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&ubo),
                vk::WriteDescriptorSet::default()
                    .dst_set(frame_set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&ssbo),
                vk::WriteDescriptorSet::default()
                    .dst_set(frame_set)
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&fog_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(frame_set)
                    .dst_binding(3)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&entity_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(frame_set)
                    .dst_binding(4)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&bone_info),
            ];
            unsafe { d.update_descriptor_sets(&writes, &[]) };
            per_frame.push(PerFrame {
                frame_buffer,
                frame_set,
                entities,
                bones,
                dynamic_vertices: Buffer::new(
                    gpu,
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                    (dynamic_vertices.max(1) as usize * size_of::<Vertex>()) as u64,
                    MemoryLocation::CpuToGpu,
                )?,
                dynamic_indexes: Buffer::new(
                    gpu,
                    vk::BufferUsageFlags::INDEX_BUFFER,
                    dynamic_indexes.max(1) as u64 * 4,
                    MemoryLocation::CpuToGpu,
                )?,
                culled_indexes: Buffer::new(
                    gpu,
                    vk::BufferUsageFlags::INDEX_BUFFER,
                    indexes.len().max(1) as u64 * 4,
                    MemoryLocation::CpuToGpu,
                )?,
            });
        }
        // Every texture's pixels, in a few large copies.
        uploads.borrow_mut().flush(gpu)?;
        let vis = vis::WorldVis::new(bsp, surface_cull);
        models.locate(&vis);
        for b in &mut brushes {
            b.link = Some(vis.link(b.entity_box));
        }
        Ok(Self {
            gpu: gpu.clone(),
            per_frame,
            vertices: Buffer::with_data(
                gpu,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                bytemuck::cast_slice(&vertices),
            )?,
            indexes: Buffer::with_data(
                gpu,
                vk::BufferUsageFlags::INDEX_BUFFER,
                bytemuck::cast_slice(&indexes),
            )?,
            static_indexes: indexes.clone(),
            _stage_buffer: stage_buffer,
            _fog_buffer: fog_buffer,
            styles: swjkja_world::style::LightStyles::spawn(&bsp.entities),
            frame_layout,
            frame_pool,
            pipelines,
            triangles: indexes.len() / 3,
            material_count: draw_materials.len(),
            // GL_EXP2 with density sqrt(-ln(1/255)) / depthForOpaque: the fog reaches
            // 1/255 of the surface colour at depthForOpaque.
            fog: fog_parms.map(|f| {
                (
                    f.color,
                    (-(1.0f32 / 255.0).ln()).sqrt() / f.depth_for_opaque,
                )
            }),
            materials: draw_materials,
            vis,
            surface_draw,
            brushes,
            models,
            first_model_surface,
            first_bone,
            light_grid: swjkja_world::light_grid::LightGrid::load(bsp, 0).with_rules(rules),
            fog_volumes,
            shadows,
            surface_gate,
            gate_count: gates,
            sprite_fov: None,
            single_player: matches!(
                rules,
                swjkja_world::static_model::Rules::SinglePlayer { .. }
            ),
        })
    }

    /// The colour to clear the view to: the global fog's colour, or black.
    pub fn clear_color(&self) -> [f32; 4] {
        self.fog
            .map_or([0.0, 0.0, 0.0, 1.0], |f| [f.0[0], f.0[1], f.0[2], 1.0])
    }

    /// Records the world for one view at `time` seconds (inside [`crate::Target::render`]).
    /// `depth_slot` is the texture slot of the target's depth copy, for flares.
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        textures: &TextureSet,
        camera: &Camera,
        time: f32,
        depth_slot: u32,
    ) -> Result<()> {
        // Game time in whole milliseconds, as cg.time.
        let styles = self
            .styles
            .colors((f64::from(time) * 1000.0).round() as i64);
        let data = FrameData {
            view_proj: camera.view_proj(),
            view_origin: [camera.origin[0], camera.origin[1], camera.origin[2], 1.0],
            // tr.identityLight is 1 without overbright bits (the original's default).
            params: [time, 1.0, self.fog.map_or(0.0, |f| f.1), camera.z_near],
            fog_color: self.fog.map_or([0.0; 4], |f| [f.0[0], f.0[1], f.0[2], 1.0]),
            misc: [
                depth_slot,
                camera.width,
                camera.height,
                u32::from(self.single_player),
            ],
            view_forward: {
                let f = camera.axes()[0];
                [f[0], f[1], f[2], 0.0]
            },
            styles: styles.map(|c| c.map(f32::from)),
        };
        let slot = frame.slot;
        self.per_frame[slot]
            .frame_buffer
            .mapped_mut()?
            .copy_from_slice(bytemuck::bytes_of(&data));

        // Geometry that depends on the view and time: deforms (RB_DeformTessGeometry),
        // into the dynamic buffers; flares follow once the visible surfaces are known.
        let [forward, right, up] = camera.axes();
        let view = deform::View {
            forward,
            left: [-right[0], -right[1], -right[2]],
            up,
        };
        let PerFrame {
            dynamic_vertices,
            dynamic_indexes,
            culled_indexes,
            frame_set,
            entities,
            bones,
            ..
        } = &mut self.per_frame[slot];
        let vb = dynamic_vertices.mapped_mut()?;
        let ib = dynamic_indexes.mapped_mut()?;
        for m in &mut self.materials {
            let Some(dynamic) = &m.dynamic else { continue };
            let (verts, indexes) = match &dynamic.source {
                DynamicSource::Deformed {
                    verts,
                    indexes,
                    deforms,
                } => {
                    let (mut v, mut i) = (verts.clone(), indexes.clone());
                    deform::apply(deforms, &mut v, &mut i, &view, time, time * 1000.0);
                    (v, i)
                }
                DynamicSource::Flares { .. }
                | DynamicSource::Shadows { .. }
                | DynamicSource::Sprites { .. } => continue,
            };
            let out: Vec<Vertex> = verts
                .iter()
                .map(|v| Vertex {
                    xyz: v.xyz,
                    normal: v.normal,
                    st: v.st,
                    lightmap: v.lightmap,
                    color: v.color,
                })
                .collect();
            let vstart = dynamic.vertex_offset as usize * size_of::<Vertex>();
            vb[vstart..vstart + out.len() * size_of::<Vertex>()]
                .copy_from_slice(bytemuck::cast_slice(&out));
            let istart = dynamic.index_offset as usize * 4;
            ib[istart..istart + indexes.len() * 4].copy_from_slice(bytemuck::cast_slice(&indexes));
            m.first_index = dynamic.index_offset;
            m.index_count = indexes.len() as u32;
        }

        // The visible surfaces' index ranges, grouped by material in the order the
        // view found them (the original's sort is stable within a material).
        let mut visible = Vec::new();
        self.vis.visible(camera, &mut visible);
        let mut gate_open = vec![false; self.gate_count];
        let mut ranges: Vec<(usize, u32, u32)> = Vec::with_capacity(visible.len());
        let frustum = vis::Frustum::new(camera);
        let view_cluster = self.vis.cluster(camera.origin);
        // The brush entities the server sends this view, culled by their model's box
        // (R_AddBrushModelSurfaces), then each surface as the world's. They come
        // first: the entity number orders draws within a material, and the world's
        // (ENTITYNUM_WORLD) is the last. A moving one is placed where it is now,
        // its surfaces culled in its model's space (R_CullSurface with
        // `or.viewOrigin`, R_CullLocalBox).
        let bone_table = bones.mapped_mut()?;
        let ms = (f64::from(time) * 1000.0).round() as i32;
        // The brush entities' surfaces this view draws, for their flares.
        let mut brush_shown = Vec::new();
        for b in &self.brushes {
            let sent = b.broadcast
                || b.link
                    .as_ref()
                    .is_some_and(|l| self.vis.sends(view_cluster, l));
            if !sent {
                continue;
            }
            let (corners, local) = match &b.turning {
                None => (b.corners, None),
                Some(t) => {
                    let mut e = t.entity.clone();
                    e.axes = swjkja_world::static_model::angles_to_axis(
                        e.angles_at(ms).unwrap_or([0.0; 3]),
                    );
                    e.origin = e.origin_at(ms);
                    let place: Mat34 = std::array::from_fn(|i| {
                        [e.axes[0][i], e.axes[1][i], e.axes[2][i], e.origin[i]]
                    });
                    let at = t.bone * size_of::<Mat34>();
                    bone_table[at..at + size_of::<Mat34>()]
                        .copy_from_slice(bytemuck::bytes_of(&place));
                    let d = [0, 1, 2].map(|k| camera.origin[k] - e.origin[k]);
                    let origin = e.axes.map(|a| (0..3).map(|k| a[k] * d[k]).sum::<f32>());
                    let corners = e.corners(t.bounds);
                    (corners, Some((origin, frustum.local(e.axes, e.origin))))
                }
            };
            if frustum.culls_points(&corners) {
                continue;
            }
            for &s in &b.surfaces {
                let culled = match &local {
                    None => self.vis.culls_surface(s, camera.origin, &frustum),
                    Some((origin, frustum)) => self.vis.culls_surface(s, *origin, frustum),
                };
                if culled {
                    continue;
                }
                brush_shown.push(s);
                if let Some(g) = self.surface_gate[s] {
                    gate_open[g] = true;
                }
                if let Some(r) = self.surface_draw[s] {
                    ranges.push(r);
                }
            }
        }
        for &s in &visible {
            if let Some(g) = self.surface_gate[s] {
                gate_open[g] = true;
            }
            if let Some(r) = self.surface_draw[s] {
                ranges.push(r);
            }
        }
        // The visible world surfaces' sprites, scattered for this view.
        let mut shown = vec![false; self.surface_draw.len()];
        for &s in &visible {
            shown[s] = true;
        }
        // The flares of the surfaces drawn (RB_SurfaceFlare): like any surface, only
        // from leaves the view sees, or from the brush entities it draws.
        let mut flare_shown = shown.clone();
        for &s in &brush_shown {
            flare_shown[s] = true;
        }
        for m in &mut self.materials {
            let Some(Dynamic {
                source: DynamicSource::Flares { flares, radius },
                vertex_offset,
                index_offset,
            }) = &m.dynamic
            else {
                continue;
            };
            let (verts, indexes) = flare_quads(
                flares
                    .iter()
                    .filter(|f| flare_shown[f.0])
                    .map(|f| (f.1, f.2)),
                *radius,
                camera,
                &view,
            );
            let out: Vec<Vertex> = verts
                .iter()
                .map(|v| Vertex {
                    xyz: v.xyz,
                    normal: v.normal,
                    st: v.st,
                    lightmap: v.lightmap,
                    color: v.color,
                })
                .collect();
            let vstart = *vertex_offset as usize * size_of::<Vertex>();
            vb[vstart..vstart + size_of_val(&out[..])].copy_from_slice(bytemuck::cast_slice(&out));
            let istart = *index_offset as usize * 4;
            ib[istart..istart + indexes.len() * 4].copy_from_slice(bytemuck::cast_slice(&indexes));
            m.first_index = *index_offset;
            m.index_count = indexes.len() as u32;
        }
        let mut sprite_view = None;
        let mut scattered = Vec::new();
        for m in &mut self.materials {
            let Some(Dynamic {
                source:
                    DynamicSource::Sprites {
                        sprites: ss,
                        additive,
                        surfaces,
                        capacity,
                    },
                vertex_offset,
                index_offset,
            }) = &m.dynamic
            else {
                continue;
            };
            scattered.clear();
            for t in surfaces.iter().filter(|t| shown[t.surface]) {
                // The first sprites drawn set the standard field of view; a camera
                // the scripts enabled drew the view before this one.
                let sprite_view = sprite_view.get_or_insert_with(|| {
                    if self.sprite_fov.is_none() && self.models.camera.is_some_and(|c| c <= ms) {
                        self.sprite_fov = Some(swjkja_world::script::CAMERA_DEFAULT_FOV);
                    }
                    sprites::SpriteView {
                        origin: camera.origin,
                        left: view.left,
                        time_ms: ms,
                        range_scale: sprites::range_scale(&mut self.sprite_fov, camera.fov_x),
                    }
                });
                sprites::scatter(
                    ss,
                    *additive,
                    &t.verts,
                    &t.indexes,
                    sprite_view,
                    &mut scattered,
                );
            }
            scattered.truncate(*capacity);
            const ST: [[f32; 2]; 4] = [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]];
            let verts: Vec<Vertex> = scattered
                .iter()
                .flat_map(|q| {
                    (0..4).map(move |k| Vertex {
                        xyz: q.corners[k],
                        normal: [0.0, 0.0, 1.0],
                        st: ST[k],
                        lightmap: [[0.0; 2]; 4],
                        color: q.color,
                    })
                })
                .collect();
            let indexes: Vec<u32> = (0..scattered.len() as u32)
                .flat_map(|q| sprites::QUAD_TRIANGLES.map(|k| q * 4 + k))
                .collect();
            let vstart = *vertex_offset as usize * size_of::<Vertex>();
            vb[vstart..vstart + size_of_val(&verts[..])]
                .copy_from_slice(bytemuck::cast_slice(&verts));
            let istart = *index_offset as usize * 4;
            ib[istart..istart + indexes.len() * 4].copy_from_slice(bytemuck::cast_slice(&indexes));
            m.first_index = *index_offset;
            m.index_count = indexes.len() as u32;
        }
        // The static models this view draws (each at its level of detail), lit from
        // the light grid, NPCs posed as they stand now; entity n + 1 is model n.
        let table = entities.mapped_mut()?;
        // The NPCs posed for this view, kept for their shadows.
        let mut posed: Vec<Option<Vec<Mat34>>> = vec![None; self.models.instances.len()];
        table[..size_of::<GpuEntity>()].copy_from_slice(bytemuck::bytes_of(&GpuEntity::WORLD));
        for (n, posed) in posed.iter_mut().enumerate() {
            let Some(level) = self
                .models
                .visible(n, camera, &frustum, view_cluster, &self.vis)
            else {
                continue;
            };
            let m = &self.models.instances[n];
            let light = self.light_grid.entity_light(
                m.light_origin,
                &styles,
                [0.0; 3],
                self.models.more_light(n, ms),
            );
            let v4 = |v: [f32; 3]| [v[0], v[1], v[2], 0.0];
            let entity = GpuEntity {
                ambient: v4(light.ambient),
                directed: v4(light.directed),
                light_dir: v4(light.dir),
                color: m.color.map(f32::from),
            };
            let at = (n + 1) * size_of::<GpuEntity>();
            table[at..at + size_of::<GpuEntity>()].copy_from_slice(bytemuck::bytes_of(&entity));
            if let Some(first) = self.first_bone[n] {
                let pose = self.models.bones(n, ms);
                let at = first * size_of::<Mat34>();
                bone_table[at..at + size_of_val(&pose[..])]
                    .copy_from_slice(bytemuck::cast_slice(&pose));
                *posed = Some(pose);
            }
            for piece in m.lods[level].pieces.clone() {
                if let Some(r) = self.surface_draw[self.first_model_surface + piece] {
                    ranges.push(r);
                }
            }
        }
        if let Some(shadows) = &mut self.shadows {
            let cast = lay_shadows(
                &self.models,
                shadows,
                &self.fog_volumes,
                camera.origin,
                |link| self.vis.sends(view_cluster, link),
                ms,
                &posed,
            );
            let pieces = cast
                .iter()
                .flat_map(|&n| &shadows.laid[n].as_ref().unwrap().pieces);
            // Each fog's pieces together, as fans, the vertices from the region's
            // start.
            let mut verts: Vec<Vertex> = Vec::new();
            let mut fans: Vec<(usize, Vec<u32>)> = Vec::new();
            for (fog, piece) in pieces {
                let fog = *fog;
                if verts.len() + piece.len() > shadows.max_vertices {
                    break;
                }
                let base = verts.len() as u32;
                verts.extend(piece.iter().map(|v| Vertex {
                    xyz: v.xyz,
                    normal: [0.0; 3],
                    st: v.st,
                    lightmap: [[0.0; 2]; 4],
                    color: [255, 255, 255, v.alpha],
                }));
                let fan = (2..piece.len() as u32).flat_map(|k| [base, base + k - 1, base + k]);
                fans.push((fog, fan.collect()));
            }
            fans.sort_by_key(|f| f.0);
            let vstart = shadows.first_vertex as usize * size_of::<Vertex>();
            vb[vstart..vstart + size_of_val(&verts[..])]
                .copy_from_slice(bytemuck::cast_slice(&verts));
            let mut at = shadows.first_index;
            for m in &mut self.materials {
                let Some(Dynamic {
                    source: DynamicSource::Shadows { fog },
                    ..
                }) = &m.dynamic
                else {
                    continue;
                };
                m.first_index = at;
                for (_, fan) in fans.iter().filter(|f| f.0 == *fog) {
                    let i = at as usize * 4;
                    ib[i..i + fan.len() * 4].copy_from_slice(bytemuck::cast_slice(fan));
                    at += fan.len() as u32;
                }
                m.index_count = at - m.first_index;
            }
        }
        ranges.sort_by_key(|r| r.0);
        for m in self.materials.iter_mut().filter(|m| m.culled) {
            m.index_count = 0;
        }
        let out = culled_indexes.mapped_mut()?;
        let mut at = 0usize;
        for (i, &(m, start, count)) in ranges.iter().enumerate() {
            if i == 0 || ranges[i - 1].0 != m {
                self.materials[m].first_index = at as u32;
            }
            self.materials[m].index_count += count;
            let src = &self.static_indexes[start as usize..(start + count) as usize];
            out[at * 4..(at + src.len()) * 4].copy_from_slice(bytemuck::cast_slice(src));
            at += src.len();
        }

        let Some(layout) = self.pipelines.first().map(|p| p.layout) else {
            return Ok(());
        };
        let d = &self.gpu.device;
        let sets = [textures.set, *frame_set];
        let frame_data = &self.per_frame[slot];
        let mut depth_copied = false;
        let mut bound = usize::MAX;
        let mut buffers_bound: Option<Bound> = None;
        for m in &self.materials {
            if m.index_count == 0 || m.gate.is_some_and(|g| !gate_open[g]) {
                continue;
            }
            // The original reads the depth buffer for each flare as it draws it; with
            // flares sorting after the opaque world, one copy before the first does.
            let flare = matches!(
                m.dynamic.as_ref().map(|d| &d.source),
                Some(DynamicSource::Flares { .. })
            );
            if flare && !depth_copied {
                frame.copy_depth();
                depth_copied = true;
                bound = usize::MAX;
                buffers_bound = None;
            }
            let cmd = frame.cmd;
            unsafe {
                if buffers_bound.is_none() {
                    d.cmd_bind_descriptor_sets(
                        cmd,
                        vk::PipelineBindPoint::GRAPHICS,
                        layout,
                        0,
                        &sets,
                        &[],
                    );
                }
                // Switch between the static, visible and per-frame geometry.
                let geometry = match (&m.dynamic, m.culled) {
                    (Some(_), _) => Bound::Dynamic,
                    (None, true) => Bound::Visible,
                    (None, false) => Bound::Static,
                };
                if buffers_bound != Some(geometry) {
                    buffers_bound = Some(geometry);
                    let (v, i) = match geometry {
                        Bound::Dynamic => (
                            frame_data.dynamic_vertices.buffer,
                            frame_data.dynamic_indexes.buffer,
                        ),
                        Bound::Visible => (self.vertices.buffer, frame_data.culled_indexes.buffer),
                        Bound::Static => (self.vertices.buffer, self.indexes.buffer),
                    };
                    d.cmd_bind_vertex_buffers(cmd, 0, &[v], &[0]);
                    d.cmd_bind_index_buffer(cmd, i, 0, vk::IndexType::UINT32);
                }
                let vertex_offset = m.dynamic.as_ref().map_or(0, |d| d.vertex_offset as i32);
                for s in &m.stages {
                    if s.pipeline != bound {
                        d.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            self.pipelines[s.pipeline].pipeline,
                        );
                        bound = s.pipeline;
                    }
                    let push = Push {
                        stage: s.gpu_stage,
                        texture0: s.slots[0].at(time),
                        texture1: s.slots[1].at(time),
                        bones: m.bones,
                    };
                    d.cmd_push_constants(
                        cmd,
                        layout,
                        vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        0,
                        bytemuck::bytes_of(&push),
                    );
                    d.cmd_draw_indexed(cmd, m.index_count, 1, m.first_index, vertex_offset, 0);
                }
            }
        }
        Ok(())
    }
}

/// The shader of the NPCs' shadows (`cgs.media.shadowMarkShader`).
const SHADOW_SHADER: &str = "markShadow";

/// Lays the NPCs' shadows (`CG_PlayerShadow`, `_PlayerShadow`) in `shadows.laid`,
/// each piece with its fog (`RE_AddPolyToScene`, seen from outside any fog), and
/// returns the NPCs that cast one: those the server sends (`sent`) whose shadow's
/// origin is within [`swjkja_world::mark::SHADOW_RANGE`] of the view, a mark of
/// [`swjkja_world::mark::SHADOW_RADIUS`] on the floor below, turned with their
/// legs. `posed` holds the NPCs this view posed. The rancor's and the AT-ST's
/// larger shadows, and cloaked NPCs' and sand creatures' lack of one, are left
/// out.
fn lay_shadows(
    models: &StaticModels,
    shadows: &mut Shadows,
    fogs: &[fog::FogVolume],
    view: [f32; 3],
    sent: impl Fn(&vis::EntityLink) -> bool,
    time: i32,
    posed: &[Option<Vec<Mat34>>],
) -> Vec<usize> {
    use swjkja_world::mark::{SHADOW_RADIUS, SHADOW_RANGE, impact_mark, shadow_floor};
    shadows.laid.resize_with(models.instances.len(), || None);
    let mut cast = Vec::new();
    for (n, m) in models.instances.iter().enumerate() {
        if !m.link.as_ref().is_some_and(&sent) {
            continue;
        }
        let Some(from @ (origin, yaw)) = models.shadow(n, time, posed[n].as_deref()) else {
            continue;
        };
        let d = [0, 1, 2].map(|k| origin[k] - view[k]);
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > SHADOW_RANGE * SHADOW_RANGE {
            continue;
        }
        if shadows.laid[n].as_ref().is_none_or(|l| l.from != from) {
            let pieces = shadow_floor(&shadows.world, origin).map_or_else(Vec::new, |floor| {
                let (floor, normal, alpha) = floor;
                let marks = impact_mark(&shadows.marks, floor, normal, yaw, alpha, SHADOW_RADIUS);
                marks
                    .into_iter()
                    .map(|piece| {
                        let fold = |f: fn(f32, f32) -> f32| {
                            let first = piece[0].xyz;
                            piece
                                .iter()
                                .fold(first, |b, v| [0, 1, 2].map(|k| f(b[k], v.xyz[k])))
                        };
                        let (lo, hi) = (fold(f32::min), fold(f32::max));
                        (crate::models::sp_fog_num_box(fogs, lo, hi, 0), piece)
                    })
                    .collect()
            });
            shadows.laid[n] = Some(Laid { from, pieces });
        }
        cast.push(n);
    }
    cast
}

/// `RB_SurfaceFlare` without its depth test (the shader does that): a quad 3 units in
/// front of each flare, facing the viewer, dimmed as the flare turns away, radius
/// shrinking within 512 units (never below 5). The flare origin goes in the vertices'
/// lightmap sets 2-3 for the depth test.
fn flare_quads(
    flares: impl Iterator<Item = ([f32; 3], [f32; 3])>,
    radius: f32,
    camera: &Camera,
    view: &deform::View,
) -> (Vec<deform::DeformVertex>, Vec<u32>) {
    let (mut verts, mut indexes) = (Vec::new(), Vec::new());
    for (origin, normal) in flares {
        let center = [0, 1, 2].map(|k| origin[k] + 3.0 * normal[k]);
        let dir = [0, 1, 2].map(|k| center[k] - camera.origin[k]);
        let dist = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        let dir = if dist > 0.0 {
            dir.map(|x| x / dist)
        } else {
            dir
        };
        let d = -(dir[0] * normal[0] + dir[1] * normal[1] + dir[2] * normal[2]);
        let d = d.abs();
        let gray = (d * 255.0) as u8;
        let mut r = radius;
        if dist < 512.0 {
            r = r * dist / 512.0;
        }
        let r = r.max(5.0);
        let (left, up) = (view.left.map(|x| x * r), view.up.map(|x| x * r));
        let n = verts.len() as u32;
        let corner = |sl: f32, su: f32| [0, 1, 2].map(|k| center[k] + sl * left[k] + su * up[k]);
        for (xyz, st) in [
            (corner(1.0, 1.0), [0.0, 0.0]),
            (corner(-1.0, 1.0), [1.0, 0.0]),
            (corner(-1.0, -1.0), [1.0, 1.0]),
            (corner(1.0, -1.0), [0.0, 1.0]),
        ] {
            verts.push(deform::DeformVertex {
                xyz,
                normal: view.forward.map(|x| -x),
                st,
                lightmap: [st, [0.0; 2], [origin[0], origin[1]], [origin[2], 0.0]],
                color: [gray, gray, gray, 255],
            });
        }
        indexes.extend([n, n + 1, n + 3, n + 3, n + 1, n + 2]);
    }
    (verts, indexes)
}

impl Drop for WorldRenderer {
    fn drop(&mut self) {
        unsafe {
            self.gpu.wait_idle();
            self.gpu
                .device
                .destroy_descriptor_pool(self.frame_pool, None);
            self.gpu
                .device
                .destroy_descriptor_set_layout(self.frame_layout, None);
        }
    }
}

/// Appends a surface's vertices and triangles for CPU deforms, in the same order as
/// [`append`] (so autoSprite sees each 4-vertex face as one quad).
fn append_deformable(
    geometry: &Geometry,
    verts: &mut Vec<deform::DeformVertex>,
    indexes: &mut Vec<u32>,
) {
    let (mut v, mut i) = (Vec::new(), Vec::new());
    append(geometry, &mut v, &mut i);
    let base = verts.len() as u32;
    indexes.extend(i.iter().map(|&i| base + i));
    verts.extend(v.iter().map(|v| deform::DeformVertex {
        xyz: v.xyz,
        normal: v.normal,
        st: v.st,
        lightmap: v.lightmap,
        color: v.color,
    }));
}

/// Appends a surface's triangles (patch grids at full detail, as `RB_SurfaceGrid`
/// orders them).
fn append(geometry: &Geometry, vertices: &mut Vec<Vertex>, indexes: &mut Vec<u32>) {
    let base = vertices.len() as u32;
    let vertex = |v: &swjkja_bsp::DrawVert| Vertex {
        xyz: v.xyz,
        normal: v.normal,
        st: v.st,
        lightmap: v.lightmap,
        color: v.color[0],
    };
    match geometry {
        Geometry::Face {
            verts, indexes: i, ..
        }
        | Geometry::Triangles {
            verts, indexes: i, ..
        } => {
            vertices.extend(verts.iter().map(vertex));
            indexes.extend(i.iter().map(|&i| base + i));
        }
        Geometry::Grid(g) => {
            vertices.extend(g.verts.iter().map(vertex));
            let w = g.width as u32;
            for row in 0..g.height as u32 - 1 {
                for col in 0..w - 1 {
                    let v1 = base + row * w + col + 1;
                    let v2 = v1 - 1;
                    let v3 = v2 + w;
                    let v4 = v3 + 1;
                    indexes.extend([v2, v3, v1, v1, v3, v4]);
                }
            }
        }
        Geometry::Flare { .. } | Geometry::Skip => {}
    }
}

/// Appends a posed model's surface in its model's space, to be skinned by the bones
/// from `first_bone` on (`RB_SurfaceGhoul`, see `vs_main` in world.wgsl): each
/// vertex's bones (up to four, the first repeated with no weight where it has fewer)
/// in lightmap sets 0 and 1, the first three weights in sets 2 and 3 (the fourth is
/// what they leave of 1), and the entity number in set 3's second coordinate.
fn append_skinned(
    surface: &swjkja_model::glm::Surface,
    first_bone: usize,
    entity: usize,
    vertices: &mut Vec<Vertex>,
    indexes: &mut Vec<u32>,
) {
    let base = vertices.len() as u32;
    vertices.extend(surface.verts.iter().zip(&surface.st).map(|(v, &st)| {
        let (mut bone, mut weight) = ([v.bones[0].0; 4], [0.0; 4]);
        for (k, &(b, w)) in v.bones.iter().enumerate() {
            (bone[k], weight[k]) = (b, w);
        }
        let bone = bone.map(|b| (first_bone + b) as f32);
        Vertex {
            xyz: v.xyz,
            normal: v.normal,
            st,
            lightmap: [
                [bone[0], bone[1]],
                [bone[2], bone[3]],
                [weight[0], weight[1]],
                [weight[2], entity as f32],
            ],
            color: [255; 4],
        }
    }));
    indexes.extend(surface.indexes.iter().map(|&i| base + i));
}
