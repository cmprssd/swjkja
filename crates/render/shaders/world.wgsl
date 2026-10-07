enable wgpu_binding_array;

// One shader stage of a material, drawn the way the original's fixed-function
// pipeline drew it (OpenJK rd-vanilla tr_shade.cpp ComputeColors/ComputeTexCoords,
// tr_shade_calc.cpp): per-vertex colours and texture coordinates from the stage's
// generators, then texture(s) x colour, then the alpha test. Blending, depth and
// culling are pipeline state.

struct Frame {
    view_proj: mat4x4<f32>,
    // xyz: the view origin.
    view_origin: vec4<f32>,
    // x: shader time in seconds, y: tr.identityLight, z: global fog density, w: r_znear.
    params: vec4<f32>,
    // The global fog's colour.
    fog_color: vec4<f32>,
    // x: the texture slot of the depth copy (for flares), y: the view width,
    // z: the view height.
    misc: vec4<u32>,
}

struct Stage {
    // rgb_gen, alpha_gen, multitexture env (0 none, 1 modulate, 2 add), alpha test.
    modes: vec4<u32>,
    // tc_gen and lightmap coordinate set for bundle 0, then bundle 1.
    tcgen: vec4<u32>,
    // rgb wave function, alpha wave function, bundle 0 clamps, bundle 1 clamps.
    funcs: vec4<u32>,
    // tcMod kinds, 4 per bundle.
    tcmod_kind: array<vec4<u32>, 2>,
    // 0-255 colour for rgbGen/alphaGen const.
    constant: vec4<f32>,
    // base, amplitude, phase, frequency.
    rgb_wave: vec4<f32>,
    alpha_wave: vec4<f32>,
    // tcGen vector: s and t for bundle 0, then bundle 1.
    tc_vector: array<vec4<f32>, 4>,
    // Two vec4 of parameters per tcMod: [bundle * 8 + index * 2 + 0..1].
    tcmod: array<vec4<f32>, 16>,
    // The light style's colour (0-255) for rgbGen lightmapStyle.
    style: vec4<f32>,
    // x: GL fog for this stage: 0 none, 1 towards the fog colour, 2 black, 3 white.
    // y: 1 for sky geometry: positions are offsets from the viewer, drawn at the far
    // plane (the original's glDepthRange(1, 1)).
    // z: 1 for flares: lm23.xyz holds the flare's origin, tested against the depth
    // copy (RB_TestZFlare).
    fog: vec4<u32>,
}

struct Push {
    stage: u32,
    texture0: u32,
    texture1: u32,
    pad: u32,
}

var<immediate> push: Push;

@group(0) @binding(0) var textures: binding_array<texture_2d<f32>, 4096>;
@group(0) @binding(1) var repeat_sampler: sampler;
@group(0) @binding(2) var clamp_sampler: sampler;
@group(1) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(1) var<storage, read> stages: array<Stage>;

// Generator values, as the shader crate numbers them.
const RGB_BAD: u32 = 0u;
const RGB_IDENTITY_LIGHTING: u32 = 1u;
const RGB_IDENTITY: u32 = 2u;
const RGB_ENTITY: u32 = 3u;
const RGB_ONE_MINUS_ENTITY: u32 = 4u;
const RGB_EXACT_VERTEX: u32 = 5u;
const RGB_VERTEX: u32 = 6u;
const RGB_ONE_MINUS_VERTEX: u32 = 7u;
const RGB_WAVE: u32 = 8u;
const RGB_LIGHTING_DIFFUSE: u32 = 9u;
const RGB_LIGHTING_DIFFUSE_ENTITY: u32 = 10u;
const RGB_CONST: u32 = 11u;
const RGB_LIGHTMAP_STYLE: u32 = 12u;

const ALPHA_IDENTITY: u32 = 0u;
const ALPHA_SKIP: u32 = 1u;
const ALPHA_ENTITY: u32 = 2u;
const ALPHA_ONE_MINUS_ENTITY: u32 = 3u;
const ALPHA_VERTEX: u32 = 4u;
const ALPHA_ONE_MINUS_VERTEX: u32 = 5u;
const ALPHA_LIGHTING_SPECULAR: u32 = 6u;
const ALPHA_WAVE: u32 = 7u;
const ALPHA_PORTAL: u32 = 8u;
const ALPHA_CONST: u32 = 9u;

const TC_TEXTURE: u32 = 0u;
const TC_LIGHTMAP: u32 = 1u;
const TC_ENVIRONMENT: u32 = 2u;
const TC_VECTOR: u32 = 3u;

const MOD_NONE: u32 = 0u;
const MOD_TURBULENT: u32 = 1u;
const MOD_SCALE: u32 = 2u;
const MOD_SCROLL: u32 = 3u;
const MOD_STRETCH: u32 = 4u;
const MOD_TRANSFORM: u32 = 5u;
const MOD_ROTATE: u32 = 6u;
const MOD_ENTITY_TRANSLATE: u32 = 7u;

const PI: f32 = 3.14159265358979;

// The original's 1024-entry function tables (R_InitFuncTables), indexed the way
// WAVEVALUE does: truncate (x * 1024) toward zero, wrap with & 1023.
fn table(func: u32, x: f32) -> f32 {
    let i = i32(x * 1024.0) & 1023;
    let f = f32(i);
    switch func {
        // sin: note the original divides by 1023, not 1024.
        case 0u: { return sin(f * 360.0 / 1023.0 * PI / 180.0); }
        case 1u: { return select(-1.0, 1.0, i < 512); }
        case 2u: {
            let j = i & 511;
            var t = f32(j) / 256.0;
            if j >= 256 { t = 1.0 - f32(j - 256) / 256.0; }
            return select(-t, t, i < 512);
        }
        case 3u: { return f / 1024.0; }
        case 4u: { return 1.0 - f / 1024.0; }
        // noise and random are not implemented yet: their base value.
        default: { return 0.0; }
    }
}

fn wave(func: u32, w: vec4<f32>) -> f32 {
    return w.x + table(func, w.z + frame.params.x * w.w) * w.y;
}

fn sin_table(index: i32) -> f32 {
    return sin(f32(index & 1023) * 360.0 / 1023.0 * PI / 180.0);
}

fn tc_gen(s: Stage, bundle: u32, xyz: vec3<f32>, normal: vec3<f32>, st: vec2<f32>, lm01: vec4<f32>, lm23: vec4<f32>) -> vec2<f32> {
    let gen = s.tcgen[bundle * 2u];
    let lm_set = s.tcgen[bundle * 2u + 1u];
    switch gen {
        case 1u: {
            switch lm_set {
                case 1u: { return lm01.zw; }
                case 2u: { return lm23.xy; }
                case 3u: { return lm23.zw; }
                default: { return lm01.xy; }
            }
        }
        case 2u: {
            // RB_CalcEnvironmentTexCoords.
            let viewer = normalize(frame.view_origin.xyz - xyz);
            let d = dot(normal, viewer);
            let reflected = normal * 2.0 * d - viewer;
            return vec2<f32>(0.5 + reflected.y * 0.5, 0.5 - reflected.z * 0.5);
        }
        case 3u: {
            return vec2<f32>(dot(xyz, s.tc_vector[bundle * 2u].xyz), dot(xyz, s.tc_vector[bundle * 2u + 1u].xyz));
        }
        default: { return st; }
    }
}

fn transform(st: vec2<f32>, m: vec4<f32>, t: vec2<f32>) -> vec2<f32> {
    // m = (m00, m01, m10, m11): s' = s*m00 + t*m10 + t0, t' = s*m01 + t*m11 + t1.
    return vec2<f32>(st.x * m.x + st.y * m.z + t.x, st.x * m.y + st.y * m.w + t.y);
}

fn tc_mods(s: Stage, bundle: u32, xyz: vec3<f32>, st_in: vec2<f32>) -> vec2<f32> {
    var st = st_in;
    let time = frame.params.x;
    for (var i = 0u; i < 4u; i++) {
        let kind = s.tcmod_kind[bundle][i];
        let a = s.tcmod[bundle * 8u + i * 2u];
        let b = s.tcmod[bundle * 8u + i * 2u + 1u];
        if kind == MOD_NONE {
            break; // the original stops at the first empty tcMod
        }
        switch kind {
            case 1u: {
                // a = (base, amplitude, phase, frequency).
                let now = a.z + time * a.w;
                st.x += sin_table(i32(((xyz.x + xyz.z) * (1.0 / 128.0) * 0.125 + now) * 1024.0)) * a.y;
                st.y += sin_table(i32((xyz.y * (1.0 / 128.0) * 0.125 + now) * 1024.0)) * a.y;
            }
            case 2u: { st *= a.xy; }
            case 3u: {
                var adj = a.xy * time;
                adj -= floor(adj);
                st += adj;
            }
            case 4u: {
                // b.x holds the wave function.
                let p = 1.0 / wave(u32(b.x), a);
                st = transform(st, vec4<f32>(p, 0.0, 0.0, p), vec2<f32>(0.5 - 0.5 * p));
            }
            case 5u: { st = transform(st, a, b.xy); }
            case 6u: {
                let degs = -a.x * time;
                let index = i32(degs * (1024.0 / 360.0));
                let sv = sin_table(index);
                let cv = sin_table(index + 256);
                st = transform(
                    st,
                    vec4<f32>(cv, sv, -sv, cv),
                    vec2<f32>(0.5 - 0.5 * cv + 0.5 * sv, 0.5 - 0.5 * sv - 0.5 * cv),
                );
            }
            default: {}
        }
    }
    return st;
}

// ComputeColors, in 0-255 like the original's byte colours.
fn colors(s: Stage, xyz: vec3<f32>, vertex: vec4<f32>) -> vec4<f32> {
    let identity_light = frame.params.y;
    let identity_byte = floor(255.0 * identity_light);
    var c = vec4<f32>(identity_byte);
    let rgb_gen = s.modes.x;
    switch rgb_gen {
        case 2u: { c = vec4<f32>(255.0); }
        case 5u: { c = vertex; }
        case 6u: { c = vec4<f32>(floor(vertex.rgb * identity_light), vertex.a); }
        case 7u: { c = vec4<f32>(floor((255.0 - vertex.rgb) * identity_light), c.a); }
        case 8u: {
            let glow = clamp(wave(s.funcs.x, s.rgb_wave) * identity_light, 0.0, 1.0);
            c = vec4<f32>(vec3<f32>(floor(255.0 * glow)), 255.0);
        }
        // The world entity is white.
        case 3u: { c = vec4<f32>(255.0); }
        case 4u: { c = vec4<f32>(0.0, 0.0, 0.0, c.a); }
        // No light grid for the world entity: ambient and directed light are zero.
        case 9u, 10u: { c = vec4<f32>(0.0, 0.0, 0.0, 255.0); }
        case 11u: { c = s.constant; }
        case 12u: { c = s.style; }
        default: {}
    }
    switch s.modes.y {
        case 0u: {
            if rgb_gen != RGB_IDENTITY && !(rgb_gen == RGB_VERTEX && identity_light == 1.0) {
                c.a = 255.0;
            }
        }
        case 2u: { c.a = 255.0; }
        case 3u: { c.a = 0.0; }
        case 4u: { if rgb_gen != RGB_VERTEX { c.a = vertex.a; } }
        case 5u: { c.a = 255.0 - vertex.a; }
        case 7u: { c.a = floor(255.0 * clamp(wave(s.funcs.y, s.alpha_wave), 0.0, 1.0)); }
        case 8u: {
            // AGEN_PORTAL needs the shader's portal range; not used by world surfaces yet.
            c.a = 255.0;
        }
        case 9u: { if rgb_gen != RGB_CONST { c.a = s.constant.a; } }
        default: {}
    }
    return c;
}

// RB_TestZFlare: on screen, and no more than 24 units behind what the depth buffer
// held when flares started drawing.
fn flare_visible(origin: vec3<f32>) -> bool {
    let clip = frame.view_proj * vec4<f32>(origin, 1.0);
    // The original's near/far test: here, in front of the near plane.
    if clip.w <= frame.params.w || abs(clip.x) >= clip.w || abs(clip.y) >= clip.w {
        return false;
    }
    let size = vec2<f32>(f32(frame.misc.y), f32(frame.misc.z));
    let pixel = vec2<i32>((clip.xy / clip.w * 0.5 + 0.5) * size);
    let depth = textureLoad(textures[frame.misc.x], pixel, 0).r;
    // Reversed depth is z_near / distance; 0 is nothing drawn.
    var buffer_distance = 1.0e30;
    if depth > 0.0 {
        buffer_distance = frame.params.w / depth;
    }
    return clip.w - buffer_distance < 24.0;
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) tc0: vec2<f32>,
    @location(1) tc1: vec2<f32>,
    @location(2) color: vec4<f32>,
    // Eye-space depth, for GL fog.
    @location(3) eye_z: f32,
}

@vertex
fn vs_main(
    @location(0) xyz: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) st: vec2<f32>,
    @location(3) lm01: vec4<f32>,
    @location(4) lm23: vec4<f32>,
    @location(5) color: vec4<f32>,
) -> VertexOut {
    let s = stages[push.stage];
    var out: VertexOut;
    var world_xyz = xyz;
    if s.fog.y == 1u {
        world_xyz = frame.view_origin.xyz + xyz;
    }
    out.position = frame.view_proj * vec4<f32>(world_xyz, 1.0);
    out.eye_z = out.position.w;
    if s.fog.y == 1u {
        out.position.z = 0.0; // reversed depth: 0 is infinitely far
    }
    if s.fog.z == 1u && !flare_visible(lm23.xyz) {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0); // outside the view: not drawn
    }
    out.tc0 = tc_mods(s, 0u, world_xyz, tc_gen(s, 0u, world_xyz, normal, st, lm01, lm23));
    out.tc1 = tc_mods(s, 1u, world_xyz, tc_gen(s, 1u, world_xyz, normal, st, lm01, lm23));
    out.color = colors(s, xyz, round(color * 255.0)) / 255.0;
    return out;
}

fn sample(slot: u32, clamp_mode: u32, tc: vec2<f32>) -> vec4<f32> {
    if clamp_mode != 0u {
        return textureSample(textures[slot], clamp_sampler, tc);
    }
    return textureSample(textures[slot], repeat_sampler, tc);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let s = stages[push.stage];
    let t0 = sample(push.texture0, s.funcs.z, in.tc0);
    var c = in.color * t0;
    switch s.modes.z {
        case 1u: { c *= sample(push.texture1, s.funcs.w, in.tc1); }
        case 2u: {
            let t1 = sample(push.texture1, s.funcs.w, in.tc1);
            c = vec4<f32>(c.rgb + t1.rgb, c.a * t1.a);
        }
        default: {}
    }
    c = clamp(c, vec4<f32>(0.0), vec4<f32>(1.0));
    // GL_EXP2 fog on the fragment's colour, before alpha test and blending: the global
    // fog for opaque materials (RB_IterateStagesGeneric with r_drawfog 2).
    if s.fog.x != 0u {
        let d = frame.params.z * in.eye_z;
        let f = clamp(exp(-(d * d)), 0.0, 1.0);
        var fog = frame.fog_color.rgb;
        if s.fog.x == 2u { fog = vec3<f32>(0.0); }
        if s.fog.x == 3u { fog = vec3<f32>(1.0); }
        c = vec4<f32>(mix(fog, c.rgb, f), c.a);
    }
    switch s.modes.w {
        case 1u: { if !(c.a > 0.0) { discard; } }
        case 2u: { if !(c.a < 0.5) { discard; } }
        case 3u: { if !(c.a >= 0.5) { discard; } }
        case 4u: { if !(c.a >= 0.75) { discard; } }
        default: {}
    }
    return c;
}
