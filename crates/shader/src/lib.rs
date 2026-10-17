//! Jedi Academy `.shader` scripts, loaded, looked up and parsed the way the original
//! renderer does (OpenJK `rd-vanilla/tr_shader.cpp`: `ScanAndLoadShaderFiles`,
//! `FindShaderInShaderText`, `ParseShader`, `ParseStage`).
//!
//! Parsing is split from resolving. [`parse`] turns a shader's text into a
//! [`ShaderDef`] holding texture *names*; it fails exactly where `ParseShader` fails for
//! a reason in the text. The original also fails a shader when one of its textures
//! does not load, or uses the white image for `$lightmap` without a lightmap: that is
//! the renderer's job when it resolves a `ShaderDef` against the loaded map.

pub mod resolve;
pub mod text;

use std::collections::HashMap;

use swjkja_fs::Vfs;
use text::{Cursor, atof, atoi, compress, until_nul};

pub const MAX_STAGES: usize = 8;
pub const MAX_TEX_MODS: usize = 4;
pub const MAX_DEFORMS: usize = 3;
pub const MAX_ANIMATION_FRAMES: usize = 32;

/// Content flags (`surfaceflags.h`).
pub mod contents {
    pub const SOLID: u32 = 0x1;
    pub const LAVA: u32 = 0x2;
    pub const WATER: u32 = 0x4;
    pub const FOG: u32 = 0x8;
    pub const PLAYERCLIP: u32 = 0x10;
    pub const MONSTERCLIP: u32 = 0x20;
    pub const BOTCLIP: u32 = 0x40;
    pub const SHOTCLIP: u32 = 0x80;
    pub const TRIGGER: u32 = 0x400;
    pub const NODROP: u32 = 0x800;
    pub const TERRAIN: u32 = 0x1000;
    pub const LADDER: u32 = 0x2000;
    pub const ABSEIL: u32 = 0x4000;
    pub const OPAQUE: u32 = 0x8000;
    pub const OUTSIDE: u32 = 0x1_0000;
    pub const SLIME: u32 = 0x2_0000;
    pub const DETAIL: u32 = 0x800_0000;
    pub const INSIDE: u32 = 0x1000_0000;
    pub const TRANSLUCENT: u32 = 0x8000_0000;
}

/// Surface flags (`surfaceflags.h`). The low bits hold the material number.
pub mod surface {
    pub const SKY: u32 = 0x2000;
    pub const SLICK: u32 = 0x4000;
    pub const METALSTEPS: u32 = 0x8000;
    pub const FORCEFIELD: u32 = 0x1_0000;
    pub const NODAMAGE: u32 = 0x4_0000;
    pub const NOIMPACT: u32 = 0x8_0000;
    pub const NOMARKS: u32 = 0x10_0000;
    pub const NODRAW: u32 = 0x20_0000;
    pub const NOSTEPS: u32 = 0x40_0000;
    pub const NODLIGHT: u32 = 0x80_0000;
    pub const NOMISCENTS: u32 = 0x100_0000;
    pub const FORCESIGHT: u32 = 0x200_0000;
}

/// `surfaceParm` names: (name, content bits kept, surface flags added, contents added).
const SURFACE_PARMS: &[(&str, u32, u32, u32)] = {
    use contents::*;
    use surface::*;
    const ALL: u32 = u32::MAX;
    &[
        ("nonsolid", !SOLID, 0, 0),
        ("nonopaque", !OPAQUE, 0, 0),
        ("lava", !SOLID, 0, LAVA),
        ("slime", !SOLID, 0, SLIME),
        ("water", !SOLID, 0, WATER),
        ("fog", !SOLID, 0, FOG),
        ("shotclip", !SOLID, 0, SHOTCLIP),
        ("playerclip", !(SOLID | OPAQUE), 0, PLAYERCLIP),
        ("monsterclip", !(SOLID | OPAQUE), 0, MONSTERCLIP),
        ("botclip", !(SOLID | OPAQUE), 0, BOTCLIP),
        ("trigger", !(SOLID | OPAQUE), 0, TRIGGER),
        ("nodrop", !(SOLID | OPAQUE), 0, NODROP),
        ("terrain", !(SOLID | OPAQUE), 0, TERRAIN),
        ("ladder", !(SOLID | OPAQUE), 0, LADDER),
        ("abseil", !(SOLID | OPAQUE), 0, ABSEIL),
        ("outside", !(SOLID | OPAQUE), 0, OUTSIDE),
        ("inside", !(SOLID | OPAQUE), 0, INSIDE),
        ("detail", ALL, 0, DETAIL),
        ("trans", ALL, 0, TRANSLUCENT),
        ("sky", ALL, SKY, 0),
        ("slick", ALL, SLICK, 0),
        ("nodamage", ALL, NODAMAGE, 0),
        ("noimpact", ALL, NOIMPACT, 0),
        ("nomarks", ALL, NOMARKS, 0),
        ("nodraw", ALL, NODRAW, 0),
        ("nosteps", ALL, NOSTEPS, 0),
        ("nodlight", ALL, NODLIGHT, 0),
        ("metalsteps", ALL, METALSTEPS, 0),
        ("nomiscents", ALL, NOMISCENTS, 0),
        ("forcefield", ALL, FORCEFIELD, 0),
        ("forcesight", ALL, FORCESIGHT, 0),
    ]
};

/// `material` names; the index is ORed into the surface flags.
pub const MATERIALS: [&str; 32] = [
    "none",
    "solidwood",
    "hollowwood",
    "solidmetal",
    "hollowmetal",
    "shortgrass",
    "longgrass",
    "dirt",
    "sand",
    "gravel",
    "glass",
    "concrete",
    "marble",
    "water",
    "snow",
    "ice",
    "flesh",
    "mud",
    "bpglass",
    "dryleaves",
    "greenleaves",
    "fabric",
    "canvas",
    "rock",
    "rubber",
    "plastic",
    "tiles",
    "carpet",
    "plaster",
    "shatterglass",
    "armor",
    "computer",
];

/// Named sort values (`shaderSort_t`).
pub mod sort {
    pub const PORTAL: f32 = 1.0;
    pub const ENVIRONMENT: f32 = 2.0;
    pub const OPAQUE: f32 = 3.0;
    pub const DECAL: f32 = 4.0;
    pub const SEE_THROUGH: f32 = 5.0;
    pub const BANNER: f32 = 6.0;
    pub const INSIDE: f32 = 7.0;
    pub const MID_INSIDE: f32 = 8.0;
    pub const MIDDLE: f32 = 9.0;
    pub const MID_OUTSIDE: f32 = 10.0;
    pub const OUTSIDE: f32 = 11.0;
    pub const UNDERWATER: f32 = 13.0;
    pub const BLEND1: f32 = 15.0;
    pub const NEAREST: f32 = 21.0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cull {
    #[default]
    Front,
    Back,
    TwoSided,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Func {
    #[default]
    Sin,
    Square,
    Triangle,
    Sawtooth,
    InverseSawtooth,
    Noise,
    Random,
}

impl Func {
    /// `NameToGenFunc`: unknown names are `Sin`.
    fn from_name(name: &[u8]) -> Self {
        match lower(name).as_str() {
            "square" => Func::Square,
            "triangle" => Func::Triangle,
            "sawtooth" => Func::Sawtooth,
            "inversesawtooth" => Func::InverseSawtooth,
            "noise" => Func::Noise,
            "random" => Func::Random,
            _ => Func::Sin,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Wave {
    pub func: Func,
    pub base: f32,
    pub amplitude: f32,
    pub phase: f32,
    pub frequency: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Texture {
    /// No `map` line: the stage has no image.
    #[default]
    None,
    /// `map $whiteimage`.
    White,
    /// `map $lightmap`: the surface's lightmap, or white if it has none.
    Lightmap,
    Map {
        name: String,
        clamp: bool,
    },
    Animated {
        frames_per_second: f32,
        names: Vec<String>,
        clamp: bool,
        one_shot: bool,
    },
    Video(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrcBlend {
    Zero,
    One,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    AlphaSaturate,
}

impl SrcBlend {
    /// `NameToSrcBlendMode`: unknown names are `One`.
    fn from_name(name: &[u8]) -> Self {
        match lower(name).as_str() {
            "gl_zero" => Self::Zero,
            "gl_dst_color" => Self::DstColor,
            "gl_one_minus_dst_color" => Self::OneMinusDstColor,
            "gl_src_alpha" => Self::SrcAlpha,
            "gl_one_minus_src_alpha" => Self::OneMinusSrcAlpha,
            "gl_dst_alpha" => Self::DstAlpha,
            "gl_one_minus_dst_alpha" => Self::OneMinusDstAlpha,
            "gl_src_alpha_saturate" => Self::AlphaSaturate,
            _ => Self::One,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DstBlend {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
}

impl DstBlend {
    /// `NameToDstBlendMode`: unknown names are `One`.
    fn from_name(name: &[u8]) -> Self {
        match lower(name).as_str() {
            "gl_zero" => Self::Zero,
            "gl_src_alpha" => Self::SrcAlpha,
            "gl_one_minus_src_alpha" => Self::OneMinusSrcAlpha,
            "gl_dst_alpha" => Self::DstAlpha,
            "gl_one_minus_dst_alpha" => Self::OneMinusDstAlpha,
            "gl_src_color" => Self::SrcColor,
            "gl_one_minus_src_color" => Self::OneMinusSrcColor,
            _ => Self::One,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaTest {
    #[default]
    None,
    Gt0,
    Lt128,
    Ge128,
    Ge192,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DepthTest {
    #[default]
    LessEqual,
    Equal,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum RgbGen {
    /// Never left in a parsed stage: replaced by the end-of-stage default.
    #[default]
    Bad,
    IdentityLighting,
    Identity,
    Entity,
    OneMinusEntity,
    ExactVertex,
    Vertex,
    OneMinusVertex,
    Wave(Wave),
    LightingDiffuse,
    LightingDiffuseEntity,
    Const,
    /// The colour of the stage's light style (only on stages a light style adds).
    LightmapStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AlphaGen {
    #[default]
    Identity,
    Skip,
    Entity,
    OneMinusEntity,
    Vertex,
    OneMinusVertex,
    LightingSpecular,
    Wave(Wave),
    Portal,
    Const,
    Dot,
    OneMinusDot,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TcGen {
    /// No `tcGen` line; the renderer picks texture or lightmap coordinates.
    #[default]
    Default,
    Environment,
    Lightmap,
    Texture,
    Vector([[f32; 3]; 2]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TexMod {
    /// A `tcMod` line whose arguments ran out, or an unknown kind: the original
    /// still counts it.
    None,
    Turbulent(Wave),
    Scale([f32; 2]),
    Scroll([f32; 2]),
    Stretch(Wave),
    Transform {
        matrix: [[f32; 2]; 2],
        translate: [f32; 2],
    },
    Rotate(f32),
    EntityTranslate,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stage {
    pub texture: Texture,
    pub alpha_test: AlphaTest,
    pub depth_test: DepthTest,
    /// `None` is the default: the stage replaces what is below it.
    pub blend_src: Option<SrcBlend>,
    pub blend_dst: Option<DstBlend>,
    pub depth_write: bool,
    pub rgb_gen: RgbGen,
    pub alpha_gen: AlphaGen,
    /// From `rgbGen const` and `alphaGen const`, 0-255 per channel.
    pub constant: [u8; 4],
    pub tc_gen: TcGen,
    pub tc_mods: Vec<TexMod>,
    pub detail: bool,
    pub glow: bool,
    /// From a valid `surfaceSprites` line and the `ss*` lines after it: the stage is
    /// drawn as sprites scattered over its surfaces instead of on them.
    pub surface_sprites: Option<SurfaceSprites>,
}

/// A `surfaceSprites` stage (`surfaceSprite_t`), without the `effect` kind's
/// `ssFX*` parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSprites {
    pub kind: SpriteKind,
    pub width: f32,
    pub height: f32,
    /// About one sprite per `density` units each way.
    pub density: f32,
    /// Sprites start fading out at `fade_dist` and are gone at `fade_max`.
    pub fade_dist: f32,
    pub fade_max: f32,
    pub fade_scale: f32,
    pub wind: f32,
    pub wind_idle: f32,
    pub variance: [f32; 2],
    pub facing: Facing,
    pub vert_skew: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteKind {
    Vertical,
    Oriented,
    Effect,
    Flattened,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Normal,
    Up,
    Down,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Deform {
    /// An unknown or incomplete `deformVertexes` line, which the original counts.
    None,
    Wave {
        spread: f32,
        wave: Wave,
    },
    Normals {
        amplitude: f32,
        frequency: f32,
    },
    Bulge {
        width: f32,
        height: f32,
        speed: f32,
    },
    Move {
        vector: [f32; 3],
        wave: Wave,
    },
    ProjectionShadow,
    AutoSprite,
    AutoSprite2,
    Text(u8),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sky {
    /// Six images `<name>_rt`, `_lf`, `_bk`, `_ft`, `_up`, `_dn`; `None` for `-` or
    /// when missing.
    pub outer_box: Option<String>,
    /// 0 when the line ended before it; 512 when written as 0.
    pub cloud_height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FogParms {
    pub color: [f32; 3],
    pub depth_for_opaque: f32,
}

/// The sun a shader declares (`q3map_sun`); the original stores it globally.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sun {
    pub light: [f32; 3],
    pub direction: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderDef {
    pub name: String,
    pub stages: Vec<Stage>,
    pub surface_flags: u32,
    pub content_flags: u32,
    /// `None` when the script sets no sort; the renderer then derives one.
    pub sort: Option<f32>,
    pub cull: Cull,
    pub deforms: Vec<Deform>,
    pub sky: Option<Sky>,
    pub fog_parms: Option<FogParms>,
    pub no_mipmaps: bool,
    pub no_picmip: bool,
    pub no_gl_fog: bool,
    pub polygon_offset: bool,
    pub no_tc: bool,
    pub entity_mergable: bool,
    pub clamp_time: f32,
    pub portal_range: Option<f32>,
    pub sun: Option<Sun>,
    pub surface_light: Option<i32>,
}

impl ShaderDef {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            stages: Vec::new(),
            surface_flags: 0,
            content_flags: contents::SOLID | contents::OPAQUE,
            sort: None,
            cull: Cull::Front,
            deforms: Vec::new(),
            sky: None,
            fog_parms: None,
            no_mipmaps: false,
            no_picmip: false,
            no_gl_fog: false,
            polygon_offset: false,
            no_tc: false,
            entity_mergable: false,
            clamp_time: 0.0,
            portal_range: None,
            sun: None,
            surface_light: None,
        }
    }
}

/// Why the original would reject a shader's text (it then builds an implicit shader
/// from an image of the same name instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    MissingOpeningBrace,
    MissingClosingBrace,
    TooManyStages,
    UnknownKeyword(String),
    UnknownStageKeyword(String),
    MissingArgument(&'static str),
    BadFogParms,
    NoStages,
    /// More than four `tcMod` lines: the original aborts the whole map load.
    TooManyTexMods,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingOpeningBrace => f.write_str("expected '{'"),
            Self::MissingClosingBrace => f.write_str("no closing '}'"),
            Self::TooManyStages => write!(f, "more than {MAX_STAGES} stages"),
            Self::UnknownKeyword(k) => write!(f, "unknown shader keyword '{k}'"),
            Self::UnknownStageKeyword(k) => write!(f, "unknown stage keyword '{k}'"),
            Self::MissingArgument(k) => write!(f, "'{k}' without its argument"),
            Self::BadFogParms => f.write_str("malformed fogParms vector"),
            Self::NoStages => f.write_str("no stages, and neither sky nor fog"),
            Self::TooManyTexMods => write!(f, "more than {MAX_TEX_MODS} tcMods in one stage"),
        }
    }
}

impl std::error::Error for ParseError {}

/// All shader text, joined and indexed like `ScanAndLoadShaderFiles`.
#[derive(Debug, Default)]
pub struct ShaderText {
    text: Vec<u8>,
    /// Lower-cased name → position just after the name (the first definition wins).
    index: HashMap<String, usize>,
    /// Files skipped for unbalanced braces, with the reason.
    pub rejected_files: Vec<(String, String)>,
}

impl ShaderText {
    /// Loads every `shaders/*.shader` the filesystem lists, in the original's order:
    /// files are listed in search order, joined last-listed first, and the first
    /// definition of a name in the joined text wins. So the file listed last wins,
    /// and within a file the first definition wins.
    pub fn load(vfs: &Vfs) -> Result<Self, swjkja_fs::Error> {
        let mut files = Vec::new();
        for name in vfs.list("shaders", ".shader") {
            let path = format!("shaders/{name}");
            files.push((path.clone(), vfs.read(&path)?));
        }
        Ok(Self::from_files(files))
    }

    /// Builds the index from (file name, contents) in listing order.
    pub fn from_files(files: Vec<(String, Vec<u8>)>) -> Self {
        let mut rejected_files = Vec::new();
        let mut joined = Vec::new();
        let kept: Vec<Vec<u8>> = files
            .into_iter()
            .filter_map(|(name, data)| match check_structure(until_nul(&data)) {
                Ok(()) => Some(data),
                Err(why) => {
                    rejected_files.push((name, why));
                    None
                }
            })
            .collect();
        for data in kept.iter().rev() {
            joined.extend_from_slice(until_nul(data));
            joined.push(b'\n');
        }
        let text = compress(&joined);

        let mut index = HashMap::new();
        let mut c = Cursor::new(&text);
        loop {
            let name = c.token(true);
            if name.is_empty() {
                break;
            }
            if name[0] == b'#' {
                c.skip_rest_of_line();
                continue;
            }
            index.entry(lower(name)).or_insert(c.pos);
            c.skip_braced_section(0);
        }
        Self {
            text,
            index,
            rejected_files,
        }
    }

    /// Names with a definition, lower-cased.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(String::as_str)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_ascii_lowercase())
    }

    /// Parses the winning definition of `name`; `None` if no script defines it.
    pub fn parse(&self, name: &str) -> Option<Result<ShaderDef, ParseError>> {
        let &pos = self.index.get(&name.to_ascii_lowercase())?;
        Some(parse(name, &mut Cursor::at(&self.text, pos)))
    }
}

/// The per-file check `ScanAndLoadShaderFiles` makes on the raw text: every name must
/// be followed by a balanced `{ }` block, or the whole file is ignored.
fn check_structure(data: &[u8]) -> Result<(), String> {
    let mut c = Cursor::new(data);
    loop {
        let name = c.token(true);
        if name.is_empty() {
            return Ok(());
        }
        if name[0] == b'#' {
            c.skip_rest_of_line();
            continue;
        }
        let name = String::from_utf8_lossy(name).into_owned();
        if c.token(true) != b"{" {
            return Err(format!("shader \"{name}\" is missing its opening brace"));
        }
        if !c.skip_braced_section(1) {
            return Err(format!("shader \"{name}\" is missing its closing brace"));
        }
    }
}

fn lower(token: &[u8]) -> String {
    String::from_utf8_lossy(token).to_ascii_lowercase()
}

fn is(token: &[u8], word: &str) -> bool {
    token.eq_ignore_ascii_case(word.as_bytes())
}

fn starts(token: &[u8], prefix: &str) -> bool {
    token.len() >= prefix.len() && token[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Reads up to `N` floats from the rest of the line, stopping at the first missing
/// one; the values read so far are kept, like the original's sequential `atof`s.
fn floats<const N: usize>(c: &mut Cursor, out: &mut [f32; N]) -> bool {
    for v in out.iter_mut() {
        let t = c.token(false);
        if t.is_empty() {
            return false;
        }
        *v = atof(t);
    }
    true
}

/// `ParseWaveForm`.
fn wave(c: &mut Cursor) -> Wave {
    let mut w = Wave::default();
    let t = c.token(false);
    if t.is_empty() {
        return w;
    }
    w.func = Func::from_name(t);
    let mut v = [0.0; 4];
    floats(c, &mut v);
    [w.base, w.amplitude, w.phase, w.frequency] = v;
    w
}

/// `ParseVector`: `( a b c )`. False on a missing parenthesis or element; the
/// elements read before that stay in `out`.
fn vector<const N: usize>(c: &mut Cursor, out: &mut [f32; N]) -> bool {
    if c.token(false) != b"(" {
        return false;
    }
    floats(c, out) && c.token(false) == b")"
}

/// The rest of the line's tokens, joined with spaces (how the original buffers
/// `tcMod` and surface-sprite arguments).
fn rest_of_line(c: &mut Cursor) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let t = c.token(false);
        if t.is_empty() {
            return out;
        }
        out.extend_from_slice(t);
        out.push(b' ');
    }
}

/// Parses one shader body (`{ ... }`) at the cursor, like `ParseShader`.
pub fn parse(name: &str, c: &mut Cursor) -> Result<ShaderDef, ParseError> {
    let mut s = ShaderDef::new(name);
    if c.token(true) != b"{" {
        return Err(ParseError::MissingOpeningBrace);
    }
    loop {
        let t = c.token(true);
        if t.is_empty() {
            return Err(ParseError::MissingClosingBrace);
        }
        if t[0] == b'}' {
            break;
        } else if t[0] == b'{' {
            if s.stages.len() >= MAX_STAGES {
                return Err(ParseError::TooManyStages);
            }
            let stage = parse_stage(&mut s, c)?;
            s.stages.push(stage);
        } else if starts(t, "qer") {
            c.skip_rest_of_line();
        } else if is(t, "material") || is(t, "q3map_material") {
            let m = c.token(false);
            if let Some(i) = MATERIALS.iter().position(|n| is(m, n)) {
                s.surface_flags |= i as u32;
            }
        } else if is(t, "sun") || is(t, "q3map_sun") || is(t, "q3map_sunExt") {
            let mut v = [0.0f32; 6];
            for x in &mut v {
                *x = atof(c.token(false));
            }
            let [r, g, b, intensity, a, e] = v;
            let len = (r * r + g * g + b * b).sqrt();
            let n = if len > 0.0 {
                [r / len, g / len, b / len]
            } else {
                [r, g, b]
            };
            let (a, e) = (
                a / 180.0 * std::f32::consts::PI,
                e / 180.0 * std::f32::consts::PI,
            );
            s.sun = Some(Sun {
                light: n.map(|x| x * intensity),
                direction: [a.cos() * e.cos(), a.sin() * e.cos(), e.sin()],
            });
            c.skip_rest_of_line();
        } else if is(t, "surfacelight") || is(t, "q3map_surfacelight") {
            s.surface_light = Some(atoi(c.token(false)));
        } else if is(t, "lightColor") || is(t, "tesssize") || starts(t, "q3map") {
            c.skip_rest_of_line();
        } else if is(t, "deformvertexes") || is(t, "deform") {
            parse_deform(&mut s, c);
        } else if is(t, "clampTime") {
            let v = c.token(false);
            if !v.is_empty() {
                s.clamp_time = atof(v);
            }
        } else if is(t, "surfaceParm") {
            let p = c.token(false);
            if let Some(&(_, keep, flags, add)) = SURFACE_PARMS.iter().find(|(n, ..)| is(p, n)) {
                s.surface_flags |= flags;
                s.content_flags |= add;
                s.content_flags &= keep;
            }
        } else if is(t, "nomipmaps") {
            s.no_mipmaps = true;
            s.no_picmip = true;
        } else if is(t, "nopicmip") {
            s.no_picmip = true;
        } else if is(t, "noglfog") {
            s.no_gl_fog = true;
        } else if is(t, "polygonOffset") {
            s.polygon_offset = true;
        } else if is(t, "noTC") {
            s.no_tc = true;
        } else if is(t, "entityMergable") {
            s.entity_mergable = true;
        } else if is(t, "fogParms") {
            let mut color = [0.0; 3];
            if !vector(c, &mut color) {
                return Err(ParseError::BadFogParms);
            }
            let mut fog = FogParms {
                color,
                depth_for_opaque: 0.0,
            };
            let d = c.token(false);
            if !d.is_empty() {
                fog.depth_for_opaque = atof(d);
                c.skip_rest_of_line();
            }
            s.fog_parms = Some(fog);
        } else if is(t, "portal") {
            s.sort = Some(sort::PORTAL);
        } else if is(t, "skyparms") {
            s.sky = Some(parse_sky(c));
        } else if is(t, "light") {
            c.token(false);
        } else if is(t, "cull") {
            let v = c.token(false);
            if is(v, "none") || is(v, "twosided") || is(v, "disable") {
                s.cull = Cull::TwoSided;
            } else if is(v, "back") || is(v, "backside") || is(v, "backsided") {
                s.cull = Cull::Back;
            }
        } else if is(t, "sort") {
            parse_sort(&mut s, c);
        } else {
            return Err(ParseError::UnknownKeyword(lower(t)));
        }
    }
    if s.stages.is_empty() && s.sky.is_none() && s.content_flags & contents::FOG == 0 {
        return Err(ParseError::NoStages);
    }
    Ok(s)
}

fn parse_stage(shader: &mut ShaderDef, c: &mut Cursor) -> Result<Stage, ParseError> {
    let mut st = Stage {
        depth_write: true,
        ..Stage::default()
    };
    let mut depth_write_explicit = false;
    loop {
        let t = c.token(true);
        if t.is_empty() {
            return Err(ParseError::MissingClosingBrace);
        }
        if t[0] == b'}' {
            break;
        } else if is(t, "map") {
            let n = c.token(false);
            st.texture = if n.is_empty() {
                return Err(ParseError::MissingArgument("map"));
            } else if is(n, "$whiteimage") {
                Texture::White
            } else if is(n, "$lightmap") {
                Texture::Lightmap
            } else {
                Texture::Map {
                    name: lower(n),
                    clamp: false,
                }
            };
        } else if is(t, "clampmap") {
            let n = c.token(false);
            if n.is_empty() {
                return Err(ParseError::MissingArgument("clampmap"));
            }
            st.texture = Texture::Map {
                name: lower(n),
                clamp: true,
            };
        } else if is(t, "animMap") || is(t, "clampanimMap") || is(t, "oneshotanimMap") {
            let (clamp, one_shot) = (is(t, "clampanimMap"), is(t, "oneshotanimMap"));
            let speed = c.token(false);
            if speed.is_empty() {
                return Err(ParseError::MissingArgument("animMap"));
            }
            let mut names = Vec::new();
            loop {
                let n = c.token(false);
                if n.is_empty() {
                    break;
                }
                if names.len() < MAX_ANIMATION_FRAMES {
                    names.push(lower(n));
                }
            }
            st.texture = Texture::Animated {
                frames_per_second: atof(speed),
                names,
                clamp,
                one_shot,
            };
        } else if is(t, "videoMap") {
            let n = c.token(false);
            if n.is_empty() {
                return Err(ParseError::MissingArgument("videoMap"));
            }
            st.texture = Texture::Video(lower(n));
        } else if is(t, "alphaFunc") {
            let f = c.token(false);
            if f.is_empty() {
                return Err(ParseError::MissingArgument("alphaFunc"));
            }
            st.alpha_test = match lower(f).as_str() {
                "gt0" => AlphaTest::Gt0,
                "lt128" => AlphaTest::Lt128,
                "ge128" => AlphaTest::Ge128,
                "ge192" => AlphaTest::Ge192,
                _ => AlphaTest::None,
            };
        } else if is(t, "depthfunc") {
            let f = c.token(false);
            if f.is_empty() {
                return Err(ParseError::MissingArgument("depthfunc"));
            }
            if is(f, "lequal") {
                st.depth_test = DepthTest::LessEqual;
            } else if is(f, "equal") {
                st.depth_test = DepthTest::Equal;
            } else if is(f, "disable") {
                st.depth_test = DepthTest::Disabled;
            }
        } else if is(t, "detail") {
            st.detail = true;
        } else if is(t, "blendfunc") {
            let a = c.token(false);
            if a.is_empty() {
                continue;
            }
            if is(a, "add") {
                (st.blend_src, st.blend_dst) = (Some(SrcBlend::One), Some(DstBlend::One));
            } else if is(a, "filter") {
                (st.blend_src, st.blend_dst) = (Some(SrcBlend::DstColor), Some(DstBlend::Zero));
            } else if is(a, "blend") {
                (st.blend_src, st.blend_dst) =
                    (Some(SrcBlend::SrcAlpha), Some(DstBlend::OneMinusSrcAlpha));
            } else {
                st.blend_src = Some(SrcBlend::from_name(a));
                let b = c.token(false);
                if b.is_empty() {
                    // The original stops here, before clearing the depth write.
                    continue;
                }
                st.blend_dst = Some(DstBlend::from_name(b));
            }
            if !depth_write_explicit {
                st.depth_write = false;
            }
        } else if is(t, "rgbGen") {
            parse_rgb_gen(&mut st, c);
        } else if is(t, "alphaGen") {
            parse_alpha_gen(shader, &mut st, c);
        } else if is(t, "texgen") || is(t, "tcGen") {
            let g = c.token(false);
            if is(g, "environment") {
                st.tc_gen = TcGen::Environment;
            } else if is(g, "lightmap") {
                st.tc_gen = TcGen::Lightmap;
            } else if is(g, "texture") || is(g, "base") {
                st.tc_gen = TcGen::Texture;
            } else if is(g, "vector") {
                let (mut s, mut t) = ([0.0; 3], [0.0; 3]);
                vector(c, &mut s);
                vector(c, &mut t);
                st.tc_gen = TcGen::Vector([s, t]);
            }
        } else if is(t, "tcMod") {
            if st.tc_mods.len() == MAX_TEX_MODS {
                return Err(ParseError::TooManyTexMods);
            }
            let args = rest_of_line(c);
            st.tc_mods.push(parse_tex_mod(&args));
        } else if is(t, "depthwrite") {
            st.depth_write = true;
            depth_write_explicit = true;
        } else if is(t, "glow") {
            st.glow = true;
        } else if is(t, "surfaceSprites") {
            let args = rest_of_line(c);
            if let Some(ss) = parse_surface_sprites(&args) {
                st.surface_sprites = Some(ss);
            }
        } else if starts(t, "ss") {
            let args = rest_of_line(c);
            // Before a valid surfaceSprites line, the line's defaults replace these.
            if let Some(ss) = &mut st.surface_sprites {
                parse_surface_sprites_optional(t, &args, ss);
            }
        } else {
            return Err(ParseError::UnknownStageKeyword(lower(t)));
        }
    }

    // End-of-stage defaults, as ParseStage applies them.
    if st.rgb_gen == RgbGen::Bad {
        st.rgb_gen = match st.blend_src {
            Some(SrcBlend::One | SrcBlend::SrcAlpha) => RgbGen::IdentityLighting,
            _ => RgbGen::Identity,
        };
    }
    if st.blend_src == Some(SrcBlend::One) && st.blend_dst == Some(DstBlend::Zero) {
        (st.blend_src, st.blend_dst) = (None, None);
        st.depth_write = true;
    }
    if st.alpha_gen == AlphaGen::Identity
        && matches!(st.rgb_gen, RgbGen::Identity | RgbGen::LightingDiffuse)
    {
        st.alpha_gen = AlphaGen::Skip;
    }
    Ok(st)
}

fn parse_rgb_gen(st: &mut Stage, c: &mut Cursor) {
    let g = c.token(false);
    st.rgb_gen = match lower(g).as_str() {
        "" => return,
        "wave" => RgbGen::Wave(wave(c)),
        "const" => {
            let mut color = [0.0; 3];
            vector(c, &mut color);
            for (out, v) in st.constant.iter_mut().zip(color) {
                *out = to_byte(255.0 * v);
            }
            RgbGen::Const
        }
        "identity" => RgbGen::Identity,
        "identitylighting" => RgbGen::IdentityLighting,
        "entity" => RgbGen::Entity,
        "oneminusentity" => RgbGen::OneMinusEntity,
        "vertex" => {
            if st.alpha_gen == AlphaGen::Identity {
                st.alpha_gen = AlphaGen::Vertex;
            }
            RgbGen::Vertex
        }
        "exactvertex" => RgbGen::ExactVertex,
        "lightingdiffuse" => RgbGen::LightingDiffuse,
        "lightingdiffuseentity" => RgbGen::LightingDiffuseEntity,
        "oneminusvertex" => RgbGen::OneMinusVertex,
        _ => return,
    };
}

fn parse_alpha_gen(shader: &mut ShaderDef, st: &mut Stage, c: &mut Cursor) {
    let g = c.token(false);
    st.alpha_gen = match lower(g).as_str() {
        "" => return,
        "wave" => AlphaGen::Wave(wave(c)),
        "const" => {
            st.constant[3] = to_byte(255.0 * atof(c.token(false)));
            AlphaGen::Const
        }
        "identity" => AlphaGen::Identity,
        "entity" => AlphaGen::Entity,
        "oneminusentity" => AlphaGen::OneMinusEntity,
        "vertex" => AlphaGen::Vertex,
        "lightingspecular" => AlphaGen::LightingSpecular,
        "oneminusvertex" => AlphaGen::OneMinusVertex,
        "dot" => AlphaGen::Dot,
        "oneminusdot" => AlphaGen::OneMinusDot,
        "portal" => {
            let r = c.token(false);
            shader.portal_range = Some(if r.is_empty() { 256.0 } else { atof(r) });
            AlphaGen::Portal
        }
        _ => return,
    };
}

/// A float stored into a byte as x86 C code does: truncate, keep the low 8 bits.
fn to_byte(v: f32) -> u8 {
    v as i32 as u8
}

/// `ParseTexMod`, from the line's tokens joined with spaces.
fn parse_tex_mod(args: &[u8]) -> TexMod {
    let mut c = Cursor::new(args);
    let kind = c.token(false);
    let mut v = [0.0f32; 6];
    if is(kind, "turb") {
        let mut w = [0.0; 4];
        if floats(&mut c, &mut w) {
            let [base, amplitude, phase, frequency] = w;
            return TexMod::Turbulent(Wave {
                func: Func::Sin,
                base,
                amplitude,
                phase,
                frequency,
            });
        }
    } else if is(kind, "scale") || is(kind, "scroll") {
        let mut xy = [0.0; 2];
        if floats(&mut c, &mut xy) {
            return if is(kind, "scale") {
                TexMod::Scale(xy)
            } else {
                TexMod::Scroll(xy)
            };
        }
    } else if is(kind, "stretch") {
        let f = c.token(false);
        let mut w = [0.0; 4];
        if !f.is_empty() && floats(&mut c, &mut w) {
            let [base, amplitude, phase, frequency] = w;
            return TexMod::Stretch(Wave {
                func: Func::from_name(f),
                base,
                amplitude,
                phase,
                frequency,
            });
        }
    } else if is(kind, "transform") {
        if floats(&mut c, &mut v) {
            let [a, b, cc, d, x, y] = v;
            return TexMod::Transform {
                matrix: [[a, b], [cc, d]],
                translate: [x, y],
            };
        }
    } else if is(kind, "rotate") {
        let mut r = [0.0; 1];
        if floats(&mut c, &mut r) {
            return TexMod::Rotate(r[0]);
        }
    } else if is(kind, "entityTranslate") {
        return TexMod::EntityTranslate;
    }
    TexMod::None
}

/// `ParseDeform`.
fn parse_deform(s: &mut ShaderDef, c: &mut Cursor) {
    let t = c.token(false);
    if t.is_empty() || s.deforms.len() == MAX_DEFORMS {
        return;
    }
    let deform = if is(t, "projectionShadow") {
        Deform::ProjectionShadow
    } else if is(t, "autosprite") {
        Deform::AutoSprite
    } else if is(t, "autosprite2") {
        Deform::AutoSprite2
    } else if starts(t, "text") {
        // "text0".."text7"; any other fifth character (even none) is text0.
        let n = t.get(4).map_or(0, |&b| b.wrapping_sub(b'0'));
        Deform::Text(if n > 7 { 0 } else { n })
    } else if is(t, "bulge") {
        let mut v = [0.0; 3];
        if floats(c, &mut v) {
            let [width, height, speed] = v;
            Deform::Bulge {
                width,
                height,
                speed,
            }
        } else {
            Deform::None
        }
    } else if is(t, "wave") {
        let spread = c.token(false);
        if spread.is_empty() {
            Deform::None
        } else {
            let sp = atof(spread);
            Deform::Wave {
                spread: if sp != 0.0 { 1.0 / sp } else { 100.0 },
                wave: wave(c),
            }
        }
    } else if is(t, "normal") {
        let mut v = [0.0; 2];
        if floats(c, &mut v) {
            Deform::Normals {
                amplitude: v[0],
                frequency: v[1],
            }
        } else {
            Deform::None
        }
    } else if is(t, "move") {
        let mut v = [0.0; 3];
        if floats(c, &mut v) {
            Deform::Move {
                vector: v,
                wave: wave(c),
            }
        } else {
            Deform::None
        }
    } else {
        Deform::None
    };
    s.deforms.push(deform);
}

/// `ParseSurfaceSprites`: `<kind> <width> <height> <density> <fadedist>`, with
/// the optional parameters' defaults; `None` (the stage unchanged) if any is missing
/// or out of range.
fn parse_surface_sprites(args: &[u8]) -> Option<SurfaceSprites> {
    let mut c = Cursor::new(args);
    let t = c.token(false);
    let kind = if is(t, "vertical") {
        SpriteKind::Vertical
    } else if is(t, "oriented") {
        SpriteKind::Oriented
    } else if is(t, "effect") {
        SpriteKind::Effect
    } else if is(t, "flattened") {
        SpriteKind::Flattened
    } else {
        return None;
    };
    let mut next = || {
        let t = c.token(false);
        (!t.is_empty()).then(|| atof(t))
    };
    let width = next().filter(|&v| v > 0.0)?;
    let height = next().filter(|&v| v > 0.0)?;
    let density = next().filter(|&v| v > 0.0)?;
    let fade_dist = next().filter(|&v| v >= 32.0)?;
    Some(SurfaceSprites {
        kind,
        width,
        height,
        density,
        fade_dist,
        fade_max: (f64::from(fade_dist) * 1.33) as f32,
        fade_scale: 0.0,
        wind: 0.0,
        wind_idle: 0.0,
        variance: [0.0; 2],
        facing: Facing::Normal,
        vert_skew: 0.0,
    })
}

/// `ParseSurfaceSpritesOptional`: one `ss*` line. A missing or out-of-range value,
/// a second facing, or an unknown name changes nothing.
fn parse_surface_sprites_optional(name: &[u8], args: &[u8], ss: &mut SurfaceSprites) {
    let mut c = Cursor::new(args);
    let mut next = || {
        let t = c.token(false);
        (!t.is_empty()).then(|| atof(t))
    };
    let facing = match lower(name).as_str() {
        "sshangdown" => Some(Facing::Down),
        "ssanyangle" => Some(Facing::Any),
        "ssfaceup" => Some(Facing::Up),
        _ => None,
    };
    if let Some(f) = facing {
        if ss.facing == Facing::Normal {
            ss.facing = f;
        }
        return;
    }
    match lower(name).as_str() {
        "ssfademax" => {
            if let Some(v) = next().filter(|&v| v > ss.fade_dist) {
                ss.fade_max = v;
            }
        }
        "ssfadescale" => {
            if let Some(v) = next() {
                ss.fade_scale = v;
            }
        }
        "ssvariance" => {
            if let Some(w) = next().filter(|&v| v >= 0.0) {
                ss.variance[0] = w;
                if let Some(h) = next().filter(|&v| v >= 0.0) {
                    ss.variance[1] = h;
                }
            }
        }
        "sswind" => {
            if let Some(v) = next().filter(|&v| v >= 0.0) {
                ss.wind = v;
                // It also sets the idle sway, which is usually the same.
                if ss.wind_idle <= 0.0 {
                    ss.wind_idle = v;
                }
            }
        }
        "sswindidle" => {
            if let Some(v) = next().filter(|&v| v >= 0.0) {
                ss.wind_idle = v;
            }
        }
        "ssvertskew" => {
            if let Some(v) = next().filter(|&v| v >= 0.0) {
                ss.vert_skew = v;
            }
        }
        _ => {}
    }
}

/// `ParseSkyParms`: `skyParms <outerbox> <cloudheight> <innerbox>`; innerboxes are
/// not supported by the original either.
fn parse_sky(c: &mut Cursor) -> Sky {
    let mut sky = Sky::default();
    let outer = c.token(false);
    if outer.is_empty() {
        return sky;
    }
    if outer != b"-" {
        sky.outer_box = Some(lower(outer));
    }
    let height = c.token(false);
    if height.is_empty() {
        return sky;
    }
    sky.cloud_height = atof(height);
    if sky.cloud_height == 0.0 {
        sky.cloud_height = 512.0;
    }
    c.token(false); // innerbox
    sky
}

/// `ParseSort`.
fn parse_sort(s: &mut ShaderDef, c: &mut Cursor) {
    let t = c.token(false);
    if t.is_empty() {
        return;
    }
    s.sort = Some(match lower(t).as_str() {
        "portal" => sort::PORTAL,
        "sky" => sort::ENVIRONMENT,
        "opaque" => sort::OPAQUE,
        "decal" => sort::DECAL,
        "seethrough" => sort::SEE_THROUGH,
        "banner" => sort::BANNER,
        "additive" => sort::BLEND1,
        "nearest" => sort::NEAREST,
        "underwater" => sort::UNDERWATER,
        "inside" => sort::INSIDE,
        "mid_inside" => sort::MID_INSIDE,
        "middle" => sort::MIDDLE,
        "mid_outside" => sort::MID_OUTSIDE,
        "outside" => sort::OUTSIDE,
        _ => atof(t),
    });
}
