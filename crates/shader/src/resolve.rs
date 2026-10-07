//! Turning a map surface's shader name into the material the original draws, as
//! `ShaderForShaderNum`, `R_FindShader` and `FinishShader` do (OpenJK
//! `rd-vanilla/tr_bsp.cpp`, `tr_shader.cpp`): the script's stages, or an implicit
//! material from an image of the same name; the default material when the script has
//! errors or a texture is missing; light styles as extra stages; sort order; and two
//! stages collapsed into one multitexture pass where the original does that.

use crate::{
    AlphaGen, AlphaTest, Cull, Deform, DepthTest, DstBlend, FogParms, RgbGen, ShaderDef,
    ShaderText, Sky, SrcBlend, Stage, TcGen, TexMod, Texture, sort,
};

/// Lightmap indexes with a special meaning (`tr_local.h`).
pub const LIGHTMAP_2D: i32 = -4;
pub const LIGHTMAP_BY_VERTEX: i32 = -3;
pub const LIGHTMAP_WHITEIMAGE: i32 = -2;
pub const LIGHTMAP_NONE: i32 = -1;
/// A light style slot that is not used (`LS_UNUSED`).
pub const STYLE_UNUSED: u8 = 0xfe;
/// `SS_FOG`, `SS_SEE_THROUGH`, `SS_BLEND0`.
const SORT_FOG: f32 = 12.0;
const SORT_BLEND0: f32 = 14.0;

/// An image a bundle shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Image {
    /// No image (no `map` line, or a video that did not open): the stage is off.
    None,
    /// A texture file (a name `swjkja_image::load` resolves).
    Named {
        name: String,
        clamp: bool,
    },
    /// Lightmap page n.
    Lightmap(usize),
    White,
    /// The engine's default image (a grey grid): what failed shaders show.
    Default,
    Animated {
        frames_per_second: f32,
        names: Vec<String>,
        clamp: bool,
        one_shot: bool,
    },
    Video(String),
}

/// One texture unit of a stage: what it samples and how its coordinates are made.
#[derive(Debug, Clone, PartialEq)]
pub struct Bundle {
    pub image: Image,
    pub is_lightmap: bool,
    pub tc_gen: TcGen,
    /// For extra light-style stages: which lightmap coordinate set (1-3) to use.
    pub lightmap_set: usize,
    pub tc_mods: Vec<TexMod>,
}

/// Fog adjustments `FinishShader` records for blended stages (`adjustColorsForFog`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FogAdjust {
    #[default]
    None,
    ModulateRgb,
    ModulateAlpha,
    ModulateRgba,
}

/// The colour GL fog fades a stage towards (`mGLFogColorOverride`): the fog's own,
/// or black or white so that additive and multiplying stages fade correctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FogColor {
    #[default]
    Fog,
    Black,
    White,
}

/// Whether and how a material is fogged (`fogPass`, set by `GeneratePermanentShader`
/// from the final sort).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FogPass {
    #[default]
    None,
    Equal,
    LessEqual,
}

/// How the second bundle combines with the first in a collapsed stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexEnv {
    Modulate,
    Add,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialStage {
    pub bundle: Bundle,
    /// The second texture unit, when two stages were collapsed into one.
    pub bundle2: Option<Bundle>,
    pub blend_src: Option<SrcBlend>,
    pub blend_dst: Option<DstBlend>,
    pub depth_write: bool,
    pub depth_test: DepthTest,
    pub alpha_test: AlphaTest,
    pub rgb_gen: RgbGen,
    pub alpha_gen: AlphaGen,
    pub constant: [u8; 4],
    pub glow: bool,
    pub detail: bool,
    /// The light style this stage's lightmap belongs to (0 = always on).
    pub lightmap_style: u8,
    /// False for a stage without an image; drawing stops at the first one.
    pub active: bool,
    pub fog_adjust: FogAdjust,
    pub fog_color: FogColor,
    pub surface_sprites: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub stages: Vec<MaterialStage>,
    /// How bundle 2 combines with bundle 1, if two stages were collapsed.
    pub multitexture: Option<TexEnv>,
    pub sort: f32,
    pub cull: Cull,
    pub polygon_offset: bool,
    pub deforms: Vec<Deform>,
    pub sky: Option<Sky>,
    pub fog_parms: Option<FogParms>,
    pub fog_pass: FogPass,
    /// `alphaGen portal`'s range, which flares also use as their size.
    pub portal_range: Option<f32>,
    pub no_mipmaps: bool,
    pub no_picmip: bool,
    pub entity_mergable: bool,
    pub surface_flags: u32,
    pub content_flags: u32,
    /// The lightmap indexes and styles the material was made for (cleared, like the
    /// original, when it has no lightmap stage).
    pub lightmaps: [i32; 4],
    pub styles: [u8; 4],
    /// The engine's default material, used because the real one had errors or a
    /// missing texture.
    pub is_default: bool,
}

/// What resolution needs to know about the outside world.
pub trait Images {
    /// Whether `name` loads as a texture (`R_FindImageFile` succeeds).
    fn exists(&mut self, name: &str) -> bool;
    /// Whether a video file plays (`CIN_PlayCinematic` succeeds).
    fn video_exists(&mut self, name: &str) -> bool {
        self.exists(name)
    }
}

impl<F: FnMut(&str) -> bool> Images for F {
    fn exists(&mut self, name: &str) -> bool {
        self(name)
    }
}

/// `COM_StripExtension`: drops a final `.ext` (not one inside a folder name).
pub fn strip_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if !name[dot..].contains('/') => &name[..dot],
        _ => name,
    }
}

fn stage_from(s: &Stage, bundle: Bundle) -> MaterialStage {
    MaterialStage {
        bundle,
        bundle2: None,
        blend_src: s.blend_src,
        blend_dst: s.blend_dst,
        depth_write: s.depth_write,
        depth_test: s.depth_test,
        alpha_test: s.alpha_test,
        rgb_gen: s.rgb_gen,
        alpha_gen: s.alpha_gen,
        constant: s.constant,
        glow: s.glow,
        detail: s.detail,
        lightmap_style: 0,
        active: true,
        fog_adjust: FogAdjust::None,
        fog_color: FogColor::Fog,
        surface_sprites: s.surface_sprites.clone(),
    }
}

fn bundle(image: Image, is_lightmap: bool, tc_gen: TcGen, tc_mods: Vec<TexMod>) -> Bundle {
    Bundle {
        image,
        is_lightmap,
        tc_gen,
        lightmap_set: 0,
        tc_mods,
    }
}

/// A plain implicit stage (`ClearGlobalShader` defaults, then what `R_FindShader`
/// sets): no blend unless given, depth write as given.
fn plain(
    image: Image,
    is_lightmap: bool,
    rgb: RgbGen,
    alpha: AlphaGen,
    depth_write: bool,
) -> MaterialStage {
    MaterialStage {
        bundle: bundle(image, is_lightmap, TcGen::Default, Vec::new()),
        bundle2: None,
        blend_src: None,
        blend_dst: None,
        depth_write,
        depth_test: DepthTest::LessEqual,
        alpha_test: AlphaTest::None,
        rgb_gen: rgb,
        alpha_gen: alpha,
        constant: [0; 4],
        glow: false,
        detail: false,
        lightmap_style: 0,
        active: true,
        fog_adjust: FogAdjust::None,
        fog_color: FogColor::Fog,
        surface_sprites: Vec::new(),
    }
}

impl Material {
    fn empty(name: &str, lightmaps: [i32; 4], styles: [u8; 4]) -> Self {
        Self {
            name: name.to_owned(),
            stages: Vec::new(),
            multitexture: None,
            sort: 0.0,
            cull: Cull::Front,
            polygon_offset: false,
            deforms: Vec::new(),
            sky: None,
            fog_parms: None,
            fog_pass: FogPass::None,
            portal_range: None,
            no_mipmaps: false,
            no_picmip: false,
            entity_mergable: false,
            surface_flags: 0,
            content_flags: crate::contents::SOLID | crate::contents::OPAQUE,
            lightmaps,
            styles,
            is_default: false,
        }
    }

    /// `tr.defaultShader`: the default image, depth-writing, opaque.
    pub fn default_material() -> Self {
        let mut m = Self::empty(
            "<default>",
            [LIGHTMAP_NONE; 4],
            [0, STYLE_UNUSED, STYLE_UNUSED, STYLE_UNUSED],
        );
        // CreateInternalShaders sets no rgbGen, so it stays CGEN_BAD.
        m.stages.push(plain(
            Image::Default,
            false,
            RgbGen::Bad,
            AlphaGen::Identity,
            true,
        ));
        m.is_default = true;
        finish(m, true)
    }
}

/// Resolves a map surface's shader like `ShaderForShaderNum`: `lightmaps` and
/// `styles` come from the surface (vertex styles for vertex-lit surfaces), and
/// `lightmap_pages` is how many lightmaps the map has. `external_lightmap` answers
/// whether `maps/<map>/lm_NNNN.tga` exists for an index past the map's own pages.
pub fn resolve_map_shader(
    text: &ShaderText,
    name: &str,
    lightmaps: [i32; 4],
    styles: [u8; 4],
    lightmap_pages: usize,
    images: &mut dyn Images,
    external_lightmap: &mut dyn FnMut(i32) -> bool,
) -> Material {
    if name.is_empty() {
        return Material::default_material();
    }
    // R_FindLightmap: an index past the map's pages needs an external lightmap image,
    // else the surface becomes vertex-lit; anything below LIGHTMAP_2D is vertex-lit too.
    let vertex = [LIGHTMAP_BY_VERTEX; 4];
    let mut lightmaps = lightmaps;
    if lightmaps[0] >= 0
        && lightmaps[0] as usize >= lightmap_pages
        && !external_lightmap(lightmaps[0])
    {
        lightmaps = vertex;
    }
    if lightmaps[0] < LIGHTMAP_2D {
        lightmaps = vertex;
    }
    let stripped = strip_extension(name);
    let mut m = Material::empty(stripped, lightmaps, styles);

    match text.parse(stripped) {
        Some(Ok(def)) => match from_script(&mut m, &def, lightmap_pages, images) {
            Ok(()) => finish(m, false),
            // ShaderForShaderNum: "if the shader had errors, just use default shader".
            Err(()) => Material::default_material(),
        },
        Some(Err(_)) => Material::default_material(),
        None => {
            if !images.exists(stripped) {
                return Material::default_material();
            }
            implicit(&mut m, stripped);
            finish(m, false)
        }
    }
}

/// The stages `R_FindShader` builds for an image without a script.
fn implicit(m: &mut Material, image: &str) {
    let named = || Image::Named {
        name: image.to_owned(),
        clamp: false,
    };
    let lm = m.lightmaps[0];
    let texture_over = |m: &mut Material, first: MaterialStage| {
        m.stages.push(first);
        let mut second = plain(named(), false, RgbGen::Identity, AlphaGen::Identity, false);
        second.blend_src = Some(SrcBlend::DstColor);
        second.blend_dst = Some(DstBlend::Zero);
        m.stages.push(second);
    };
    match lm {
        LIGHTMAP_NONE => m.stages.push(plain(
            named(),
            false,
            RgbGen::LightingDiffuse,
            AlphaGen::Identity,
            true,
        )),
        LIGHTMAP_BY_VERTEX => m.stages.push(plain(
            named(),
            false,
            RgbGen::ExactVertex,
            AlphaGen::Skip,
            true,
        )),
        LIGHTMAP_2D => {
            let mut s = plain(named(), false, RgbGen::Vertex, AlphaGen::Vertex, false);
            s.depth_test = DepthTest::Disabled;
            s.blend_src = Some(SrcBlend::SrcAlpha);
            s.blend_dst = Some(DstBlend::OneMinusSrcAlpha);
            m.stages.push(s);
        }
        LIGHTMAP_WHITEIMAGE => texture_over(
            m,
            plain(
                Image::White,
                false,
                RgbGen::IdentityLighting,
                AlphaGen::Identity,
                true,
            ),
        ),
        n => texture_over(
            m,
            plain(
                Image::Lightmap(n as usize),
                true,
                RgbGen::Identity,
                AlphaGen::Identity,
                true,
            ),
        ),
    }
}

/// Builds the material's stages from its script; `Err` where `ParseStage` fails for a
/// texture that does not load.
fn from_script(
    m: &mut Material,
    def: &ShaderDef,
    pages: usize,
    images: &mut dyn Images,
) -> Result<(), ()> {
    m.sort = def.sort.unwrap_or(0.0);
    m.cull = def.cull;
    m.polygon_offset = def.polygon_offset;
    m.deforms = def.deforms.clone();
    m.sky = def.sky.clone();
    m.fog_parms = def.fog_parms;
    m.portal_range = def.portal_range;
    m.no_mipmaps = def.no_mipmaps;
    m.no_picmip = def.no_picmip;
    m.entity_mergable = def.entity_mergable;
    m.surface_flags = def.surface_flags;
    m.content_flags = def.content_flags;
    for s in &def.stages {
        let (image, is_lightmap) = match &s.texture {
            Texture::None => (Image::None, false),
            Texture::White => (Image::White, false),
            Texture::Lightmap => {
                let lm = m.lightmaps[0];
                if lm < 0 || lm as usize >= pages {
                    (Image::White, true)
                } else {
                    (Image::Lightmap(lm as usize), true)
                }
            }
            Texture::Map { name, clamp } => {
                if !images.exists(name) {
                    return Err(());
                }
                (
                    Image::Named {
                        name: name.clone(),
                        clamp: *clamp,
                    },
                    false,
                )
            }
            Texture::Animated {
                frames_per_second,
                names,
                clamp,
                one_shot,
            } => {
                if names.iter().any(|n| !images.exists(n)) {
                    return Err(());
                }
                (
                    Image::Animated {
                        frames_per_second: *frames_per_second,
                        names: names.clone(),
                        clamp: *clamp,
                        one_shot: *one_shot,
                    },
                    false,
                )
            }
            Texture::Video(name) => {
                if images.video_exists(name) {
                    (Image::Video(name.clone()), false)
                } else {
                    (Image::None, false)
                }
            }
        };
        m.stages.push(stage_from(
            s,
            bundle(image, is_lightmap, s.tc_gen, s.tc_mods.clone()),
        ));
    }
    Ok(())
}

fn blended(s: &MaterialStage) -> bool {
    s.blend_src.is_some() || s.blend_dst.is_some()
}

/// `FinishShader` (with `r_detailTextures` on and vertex lighting off, the defaults).
fn finish(mut m: Material, is_default: bool) -> Material {
    m.is_default = is_default;
    if m.sky.is_some() {
        m.sort = sort::ENVIRONMENT;
    }
    if m.polygon_offset && m.sort == 0.0 {
        m.sort = sort::DECAL;
    }

    // Vertex-lit surfaces drop or recolour their lightmap stage.
    let mut lm_stage = m.stages.iter().position(|s| s.bundle.is_lightmap);
    if let Some(lm) = lm_stage
        && m.lightmaps[0] == LIGHTMAP_BY_VERTEX
    {
        if lm == 0 {
            m.stages.remove(0);
            if let Some(s) = m.stages.first_mut() {
                s.blend_src = None;
                s.blend_dst = None;
                s.depth_write = true;
                s.depth_test = DepthTest::LessEqual;
                s.alpha_test = AlphaTest::None;
            }
        }
        if let Some(s) = m.stages.get_mut(lm) {
            s.rgb_gen = RgbGen::ExactVertex;
            s.alpha_gen = AlphaGen::Skip;
        }
        lm_stage = None;
    }

    // Light styles: one additive copy of the lightmap stage per extra style. (With
    // even the first style slot unused, nothing is set.)
    let used = m
        .styles
        .iter()
        .position(|&s| s >= STYLE_UNUSED)
        .unwrap_or(4);
    if let Some(lm) = lm_stage
        && used > 0
    {
        let extra = used - 1;
        for i in 0..extra {
            let mut s = m.stages[lm].clone();
            match m.lightmaps[i + 1] {
                LIGHTMAP_BY_VERTEX => s.bundle.image = Image::White,
                n if n < 0 => s.bundle.image = Image::White, // the original aborts the map here
                n => {
                    s.bundle.image = Image::Lightmap(n as usize);
                    s.bundle.tc_gen = TcGen::Lightmap;
                    s.bundle.lightmap_set = i + 1;
                }
            }
            s.rgb_gen = RgbGen::LightmapStyle;
            s.blend_src = Some(SrcBlend::One);
            s.blend_dst = Some(DstBlend::One);
            m.stages.insert(lm + 1 + i, s);
        }
        m.stages.truncate(crate::MAX_STAGES);
        for i in 0..=extra {
            if let Some(s) = m.stages.get_mut(lm + i) {
                s.lightmap_style = m.styles[i];
            }
        }
    }

    // The per-stage pass: image-less stages are turned off (but later stages are
    // still processed); default texture coordinates; sort and fog adjustments.
    let first_blended = m.stages.first().is_some_and(blended);
    let mut has_lightmap_stage = false;
    let next_is_lightmap: Vec<bool> = (0..m.stages.len())
        .map(|i| m.stages.get(i + 1).is_some_and(|n| n.bundle.is_lightmap))
        .collect();
    for (index, s) in m.stages.iter_mut().enumerate() {
        if s.bundle.image == Image::None {
            s.active = false;
            continue;
        }
        s.fog_color = fog_color(s, index, next_is_lightmap[index]);
        if s.bundle.tc_gen == TcGen::Default {
            s.bundle.tc_gen = if s.bundle.is_lightmap {
                TcGen::Lightmap
            } else {
                TcGen::Texture
            };
        }
        has_lightmap_stage |= s.bundle.is_lightmap;
        if blended(s) && first_blended {
            s.fog_adjust = match (s.blend_src, s.blend_dst) {
                (Some(SrcBlend::One), Some(DstBlend::One))
                | (Some(SrcBlend::Zero), Some(DstBlend::OneMinusSrcColor)) => {
                    FogAdjust::ModulateRgb
                }
                (Some(SrcBlend::SrcAlpha), Some(DstBlend::OneMinusSrcAlpha)) => {
                    FogAdjust::ModulateAlpha
                }
                (Some(SrcBlend::One), Some(DstBlend::OneMinusSrcAlpha)) => FogAdjust::ModulateRgba,
                _ => FogAdjust::None,
            };
            if m.sort == 0.0 {
                m.sort = if s.depth_write {
                    sort::SEE_THROUGH
                } else if (s.blend_src, s.blend_dst) == (Some(SrcBlend::One), Some(DstBlend::One)) {
                    sort::BLEND1
                } else {
                    SORT_BLEND0
                };
            }
        }
    }
    if m.sort == 0.0 {
        m.sort = sort::OPAQUE;
    }
    let processed = m.stages.len();
    if m.stages.len() > 1 && m.stages[0].active && m.stages[1].active {
        collapse(&mut m);
    }
    // The renderer draws stages up to the first inactive one.
    let drawn = m
        .stages
        .iter()
        .position(|s| !s.active)
        .unwrap_or(m.stages.len());
    m.stages.truncate(drawn);
    if m.lightmaps[0] >= 0 && !has_lightmap_stage {
        m.lightmaps = [LIGHTMAP_NONE; 4];
        m.styles = [0, STYLE_UNUSED, STYLE_UNUSED, STYLE_UNUSED];
    }
    if processed == 0 && m.sky.is_none() {
        m.sort = SORT_FOG;
    }
    // GeneratePermanentShader: opaque and see-through materials get fogged; this
    // overrides even `noglfog`, as in the original.
    m.fog_pass = if m.sort <= sort::SEE_THROUGH {
        FogPass::Equal
    } else if m.content_flags & crate::contents::FOG != 0 {
        FogPass::LessEqual
    } else {
        FogPass::None
    };
    m
}

/// `FinishShader`'s `mGLFogColorOverride` rules, in their order.
fn fog_color(s: &MaterialStage, index: usize, next_is_lightmap: bool) -> FogColor {
    use DstBlend as D;
    use SrcBlend as S;
    match (s.blend_src, s.blend_dst) {
        (Some(S::One), Some(D::One)) => FogColor::Black,
        (Some(S::SrcAlpha), Some(D::One))
            if s.alpha_gen == AlphaGen::LightingSpecular && index > 0 =>
        {
            FogColor::Black
        }
        (Some(S::Zero), Some(D::Zero)) | (Some(S::One), Some(D::Zero)) => FogColor::White,
        (None, None) if index > 0 => FogColor::White,
        (None, None)
            if s.bundle.is_lightmap && index < crate::MAX_STAGES - 1 && next_is_lightmap =>
        {
            FogColor::White
        }
        (Some(S::DstColor), Some(D::Zero)) if s.bundle.is_lightmap => FogColor::White,
        (Some(S::DstColor), Some(D::Zero)) | (Some(S::One), Some(D::OneMinusSrcColor)) => {
            FogColor::Black
        }
        _ => FogColor::Fog,
    }
}

/// `CollapseMultitexture`: two stages into one where the blend pair is in the
/// original's table and everything else matches.
fn collapse(m: &mut Material) {
    use DstBlend as D;
    use SrcBlend as S;
    type Blend = (Option<S>, Option<D>);
    const NONE: Blend = (None, None);
    let table: [(Blend, Blend, TexEnv, Blend); 8] = [
        (
            NONE,
            (Some(S::Zero), Some(D::SrcColor)),
            TexEnv::Modulate,
            NONE,
        ),
        (
            NONE,
            (Some(S::DstColor), Some(D::Zero)),
            TexEnv::Modulate,
            NONE,
        ),
        (
            (Some(S::DstColor), Some(D::Zero)),
            (Some(S::DstColor), Some(D::Zero)),
            TexEnv::Modulate,
            (Some(S::DstColor), Some(D::Zero)),
        ),
        (
            (Some(S::Zero), Some(D::SrcColor)),
            (Some(S::DstColor), Some(D::Zero)),
            TexEnv::Modulate,
            (Some(S::DstColor), Some(D::Zero)),
        ),
        (
            (Some(S::DstColor), Some(D::Zero)),
            (Some(S::Zero), Some(D::SrcColor)),
            TexEnv::Modulate,
            (Some(S::DstColor), Some(D::Zero)),
        ),
        (
            (Some(S::Zero), Some(D::SrcColor)),
            (Some(S::Zero), Some(D::SrcColor)),
            TexEnv::Modulate,
            (Some(S::DstColor), Some(D::Zero)),
        ),
        (NONE, (Some(S::One), Some(D::One)), TexEnv::Add, NONE),
        (
            (Some(S::One), Some(D::One)),
            (Some(S::One), Some(D::One)),
            TexEnv::Add,
            (Some(S::One), Some(D::One)),
        ),
    ];
    let (a, b) = (&m.stages[0], &m.stages[1]);
    // Everything but blend and depth write must match (depth test, alpha test).
    if (a.depth_test, a.alpha_test) != (b.depth_test, b.alpha_test) {
        return;
    }
    let Some(&(_, _, env, result)) = table.iter().find(|(ba, bb, ..)| {
        *ba == (a.blend_src, a.blend_dst) && *bb == (b.blend_src, b.blend_dst)
    }) else {
        return;
    };
    if a.rgb_gen != b.rgb_gen || a.alpha_gen != b.alpha_gen {
        return;
    }
    if env == TexEnv::Add && a.rgb_gen != RgbGen::Identity {
        return;
    }
    let b = m.stages.remove(1);
    let a = &mut m.stages[0];
    if a.bundle.is_lightmap {
        a.bundle2 = Some(std::mem::replace(&mut a.bundle, b.bundle));
    } else {
        a.bundle2 = Some(b.bundle);
    }
    (a.blend_src, a.blend_dst) = result;
    m.multitexture = Some(env);
}
