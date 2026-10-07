//! The world drawn the way the original draws it: every surface of the world model
//! with its resolved material ([`swjkja_shader::resolve`]), stage by stage, in the
//! original's order (materials by sort value, then by creation order; each material's
//! surfaces stage by stage).
//!
//! Not yet: fog volumes (brush fog), normal and text deforms, light-style animation, distance LOD
//! for patches, visibility culling, video maps (drawn white), noise waves (their
//! base value), fog on sky layers.

use std::collections::HashMap;
use std::sync::Arc;

use ash::vk;
use bytemuck::{Pod, Zeroable};
use gpu_allocator::MemoryLocation;
use swjkja_bsp::Bsp;
use swjkja_fs::Vfs;
use swjkja_shader::resolve::{
    FogColor, FogPass, Image, Images, Material, MaterialStage, TexEnv, resolve_map_shader,
};
use swjkja_shader::{
    AlphaGen, AlphaTest, Cull, Deform, DepthTest, DstBlend, Func, RgbGen, ShaderText, SrcBlend,
    TcGen, TexMod,
};
use swjkja_world::{Geometry, World};

use crate::camera::Camera;
use crate::deform;
use crate::gpu::{Gpu, Result};
use crate::pipeline::{Pipeline, State, spirv};
use crate::resource::Buffer;
use crate::sky;
use crate::texture::{Texture, TextureSet};

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

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Push {
    stage: u32,
    texture0: u32,
    texture1: u32,
    pad: u32,
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
}

/// A deformed material's base geometry and where its per-frame result goes in the
/// dynamic buffers.
struct Dynamic {
    verts: Vec<deform::DeformVertex>,
    indexes: Vec<u32>,
    deforms: Vec<swjkja_shader::Deform>,
    vertex_offset: u32,
    index_offset: u32,
}

pub struct WorldRenderer {
    gpu: Arc<Gpu>,
    vertices: Buffer,
    indexes: Buffer,
    /// Deformed geometry, rewritten every frame.
    dynamic_vertices: Buffer,
    dynamic_indexes: Buffer,
    frame_buffer: Buffer,
    _stage_buffer: Buffer,
    frame_layout: vk::DescriptorSetLayout,
    frame_pool: vk::DescriptorPool,
    frame_set: vk::DescriptorSet,
    pipelines: Vec<Pipeline>,
    /// In drawing order.
    materials: Vec<DrawMaterial>,
    pub triangles: usize,
    pub material_count: usize,
    /// The global fog's colour and GL fog density, if the map has one.
    fog: Option<([f32; 3], f32)>,
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
    // Light styles are not animated yet: every style shows full brightness.
    g.style = [255.0; 4];
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
    /// Resolves and uploads everything the world model needs. Textures go into
    /// `textures`, which must outlive the renderer.
    pub fn new(
        gpu: &Arc<Gpu>,
        textures: &mut TextureSet,
        vfs: &Vfs,
        shaders: &ShaderText,
        bsp: &Bsp,
        world: &World,
    ) -> Result<Self> {
        // Images: decode once, keep the decoded pixels to upload on first use.
        let mut decoded: HashMap<String, Option<swjkja_image::Image>> = HashMap::new();
        struct Loader<'a> {
            vfs: &'a Vfs,
            decoded: &'a mut HashMap<String, Option<swjkja_image::Image>>,
        }
        impl Images for Loader<'_> {
            fn exists(&mut self, name: &str) -> bool {
                let vfs = self.vfs;
                self.decoded
                    .entry(name.to_owned())
                    .or_insert_with(|| swjkja_image::load(vfs, name).ok().map(|l| l.image))
                    .is_some()
            }
        }

        // Materials, in creation order (the order surfaces first use them).
        let model = &bsp.models[0];
        let world_surfaces =
            model.first_surface as usize..(model.first_surface + model.num_surfaces) as usize;
        let mut material_index: HashMap<(String, [i32; 4], [u8; 4]), usize> = HashMap::new();
        let mut materials: Vec<Material> = Vec::new();
        // Per material, its surfaces by fog number (the original's sort key orders draws
        // by material, then fog).
        let mut surfaces_of: Vec<std::collections::BTreeMap<usize, Vec<usize>>> = Vec::new();
        for i in world_surfaces {
            let s = &world.surfaces[i];
            if matches!(s.geometry, Geometry::Skip | Geometry::Flare { .. }) {
                continue;
            }
            let shader_name = bsp.shaders[s.shader].name().to_owned();
            let styles = if s.lightmaps[0] == swjkja_world::LIGHTMAP_BY_VERTEX {
                s.vertex_styles
            } else {
                s.lightmap_styles
            };
            let key = (shader_name.clone(), s.lightmaps, styles);
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
                    surfaces_of.push(Default::default());
                    material_index.insert(key, materials.len() - 1);
                    materials.len() - 1
                }
            };
            surfaces_of[m].entry(s.fog).or_default().push(i);
        }

        // The global fog (the one without a brush; fog numbers are 1-based) and its
        // parameters, from its shader's fogParms (red at 250 units if it has none, as
        // in R_LoadFogs).
        let global_fog = bsp.fogs.iter().position(|f| f.brush == -1).map(|i| i + 1);
        let fog_parms = global_fog.map(|n| {
            let name = swjkja_shader::resolve::strip_extension(bsp.fogs[n - 1].shader());
            match shaders.parse(name) {
                Some(Ok(def)) if def.fog_parms.is_some() => def.fog_parms.unwrap(),
                _ => swjkja_shader::FogParms {
                    color: [1.0, 0.0, 0.0],
                    depth_for_opaque: 250.0,
                },
            }
        });

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
        let lightmaps: Vec<u32> = bsp
            .lightmaps
            .chunks(swjkja_bsp::LIGHTMAP_BYTES)
            .map(|page| {
                let rgba: Vec<u8> = page
                    .chunks(3)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect();
                Ok(textures.add(Texture::new(gpu, 128, 128, &rgba, false)?))
            })
            .collect::<Result<_>>()?;
        let mut named: HashMap<String, u32> = HashMap::new();
        let mut upload = |name: &str, textures: &mut TextureSet| -> Result<u32> {
            if let Some(&slot) = named.get(name) {
                return Ok(slot);
            }
            let slot = match decoded
                .get(name)
                .cloned()
                .flatten()
                .or_else(|| swjkja_image::load(vfs, name).ok().map(|l| l.image))
            {
                Some(img) => {
                    textures.add(Texture::new(gpu, img.width, img.height, &img.rgba, true)?)
                }
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
                        let img = swjkja_image::load(vfs, name).ok().map(|l| l.image);
                        match img {
                            Some(img) => textures
                                .add(Texture::new(gpu, img.width, img.height, &img.rgba, true)?),
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
        let (mut dynamic_vertices, mut dynamic_indexes) = (0u32, 0u32);
        for &m in &order {
            let material = &materials[m];
            if let Some(sky) = &material.sky {
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
                let (first_index, index_count, dynamic) = if deformed {
                    // Kept on the CPU and rebuilt each frame (see draw()).
                    let mut dv = Vec::new();
                    let mut di = Vec::new();
                    for &i in group {
                        append_deformable(&world.surfaces[i].geometry, &mut dv, &mut di);
                    }
                    let dynamic = Dynamic {
                        vertex_offset: dynamic_vertices,
                        index_offset: dynamic_indexes,
                        verts: dv,
                        indexes: di,
                        deforms: material.deforms.clone(),
                    };
                    // Deforms keep the vertex and index counts (autoSprite rebuilds
                    // whole quads, 4 vertices and 6 indexes each).
                    dynamic_vertices += dynamic.verts.len() as u32;
                    dynamic_indexes +=
                        (dynamic.indexes.len() as u32).max(dynamic.verts.len() as u32 / 4 * 6);
                    (0, 0, Some(dynamic))
                } else {
                    let first_index = indexes.len() as u32;
                    for &i in group {
                        append(&world.surfaces[i].geometry, &mut vertices, &mut indexes);
                    }
                    (first_index, indexes.len() as u32 - first_index, None)
                };
                if index_count == 0 && dynamic.as_ref().is_none_or(|d| d.indexes.is_empty()) {
                    continue;
                }
                // GL fog for the global fog on fogged (opaque and see-through) materials.
                // Fog volumes (brush fog) are not drawn yet.
                let gl_fog =
                    fog != 0 && Some(fog) == global_fog && material.fog_pass != FogPass::None;
                let mut stages = Vec::new();
                for s in &material.stages {
                    let st = state(material, s);
                    let pipeline = states.iter().position(|x| *x == st).unwrap_or_else(|| {
                        states.push(st);
                        states.len() - 1
                    });
                    let mut g = gpu_stage(s, material.multitexture);
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
                    stages.push(DrawStage {
                        gpu_stage: gpu_stages.len() as u32 - 1,
                        pipeline,
                        slots: [slots_for(&s.bundle.image, textures)?, slot1],
                    });
                }
                draw_materials.push(DrawMaterial {
                    first_index,
                    index_count,
                    stages,
                    dynamic,
                });
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

        let frame_buffer = Buffer::new(
            gpu,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            size_of::<FrameData>() as u64,
            MemoryLocation::CpuToGpu,
        )?;
        let stage_buffer = Buffer::with_data(
            gpu,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            bytemuck::cast_slice(&gpu_stages),
        )?;
        let sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: 1,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 1,
            },
        ];
        let frame_pool = unsafe {
            d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )?
        };
        let frame_set = unsafe {
            d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(frame_pool)
                    .set_layouts(&[frame_layout]),
            )?[0]
        };
        let ubo = [vk::DescriptorBufferInfo {
            buffer: frame_buffer.buffer,
            offset: 0,
            range: vk::WHOLE_SIZE,
        }];
        let ssbo = [vk::DescriptorBufferInfo {
            buffer: stage_buffer.buffer,
            offset: 0,
            range: vk::WHOLE_SIZE,
        }];
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
        ];
        unsafe { d.update_descriptor_sets(&writes, &[]) };

        let dynamic_vertices_buffer = Buffer::new(
            gpu,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            (dynamic_vertices.max(1) as usize * size_of::<Vertex>()) as u64,
            MemoryLocation::CpuToGpu,
        )?;
        let dynamic_indexes_buffer = Buffer::new(
            gpu,
            vk::BufferUsageFlags::INDEX_BUFFER,
            dynamic_indexes.max(1) as u64 * 4,
            MemoryLocation::CpuToGpu,
        )?;
        Ok(Self {
            gpu: gpu.clone(),
            dynamic_vertices: dynamic_vertices_buffer,
            dynamic_indexes: dynamic_indexes_buffer,
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
            frame_buffer,
            _stage_buffer: stage_buffer,
            frame_layout,
            frame_pool,
            frame_set,
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
        })
    }

    /// The colour to clear the view to: the global fog's colour, or black.
    pub fn clear_color(&self) -> [f32; 4] {
        self.fog
            .map_or([0.0, 0.0, 0.0, 1.0], |f| [f.0[0], f.0[1], f.0[2], 1.0])
    }

    /// Records the world for one view at `time` seconds (inside [`crate::Target::render`]).
    pub fn draw(
        &mut self,
        cmd: vk::CommandBuffer,
        textures: &TextureSet,
        camera: &Camera,
        time: f32,
    ) -> Result<()> {
        let frame = FrameData {
            view_proj: camera.view_proj(),
            view_origin: [camera.origin[0], camera.origin[1], camera.origin[2], 1.0],
            // tr.identityLight is 1 without overbright bits (the original's default).
            params: [time, 1.0, self.fog.map_or(0.0, |f| f.1), 0.0],
            fog_color: self.fog.map_or([0.0; 4], |f| [f.0[0], f.0[1], f.0[2], 1.0]),
        };
        self.frame_buffer
            .mapped_mut()?
            .copy_from_slice(bytemuck::bytes_of(&frame));

        // Deforms: rebuild each deformed material's geometry for this view and time
        // (RB_DeformTessGeometry), into the dynamic buffers.
        let [forward, right, up] = camera.axes();
        let view = deform::View {
            forward,
            left: [-right[0], -right[1], -right[2]],
            up,
        };
        let vb = self.dynamic_vertices.mapped_mut()?;
        let ib = self.dynamic_indexes.mapped_mut()?;
        for m in &mut self.materials {
            let Some(dynamic) = &m.dynamic else { continue };
            let mut verts = dynamic.verts.clone();
            let mut indexes = dynamic.indexes.clone();
            deform::apply(
                &dynamic.deforms,
                &mut verts,
                &mut indexes,
                &view,
                time,
                time * 1000.0,
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
            let vstart = dynamic.vertex_offset as usize * size_of::<Vertex>();
            vb[vstart..vstart + out.len() * size_of::<Vertex>()]
                .copy_from_slice(bytemuck::cast_slice(&out));
            let istart = dynamic.index_offset as usize * 4;
            ib[istart..istart + indexes.len() * 4].copy_from_slice(bytemuck::cast_slice(&indexes));
            m.first_index = dynamic.index_offset;
            m.index_count = indexes.len() as u32;
        }

        let d = &self.gpu.device;
        let layout = self.pipelines.first().map(|p| p.layout);
        let Some(layout) = layout else { return Ok(()) };
        unsafe {
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                layout,
                0,
                &[textures.set, self.frame_set],
                &[],
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.vertices.buffer], &[0]);
            d.cmd_bind_index_buffer(cmd, self.indexes.buffer, 0, vk::IndexType::UINT32);
            let mut bound = usize::MAX;
            let mut dynamic_bound = false;
            for m in &self.materials {
                // Switch between the static and the per-frame (deformed) geometry.
                if m.dynamic.is_some() != dynamic_bound {
                    dynamic_bound = m.dynamic.is_some();
                    let (v, i) = if dynamic_bound {
                        (self.dynamic_vertices.buffer, self.dynamic_indexes.buffer)
                    } else {
                        (self.vertices.buffer, self.indexes.buffer)
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
                        pad: 0,
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

impl Drop for WorldRenderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.gpu.device.device_wait_idle();
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
