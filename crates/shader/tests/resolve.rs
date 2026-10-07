use swjkja_shader::resolve::*;
use swjkja_shader::*;

const NO_STYLES: [u8; 4] = [0, STYLE_UNUSED, STYLE_UNUSED, STYLE_UNUSED];

fn text(src: &str) -> ShaderText {
    ShaderText::from_files(vec![("a.shader".into(), src.as_bytes().to_vec())])
}

/// Resolves with every image existing except names containing "missing", 4 lightmap
/// pages and no external lightmaps.
fn resolve(src: &str, name: &str, lightmaps: [i32; 4], styles: [u8; 4]) -> Material {
    let mut exists = |n: &str| !n.contains("missing");
    resolve_map_shader(
        &text(src),
        name,
        lightmaps,
        styles,
        4,
        &mut exists,
        &mut |_| false,
    )
}

fn named(n: &str) -> Image {
    Image::Named {
        name: n.into(),
        clamp: false,
    }
}

#[test]
fn an_image_without_a_script_becomes_texture_times_lightmap_in_one_pass() {
    let m = resolve("", "textures/kejim/floor.tga", [3, -1, -1, -1], NO_STYLES);
    assert_eq!(m.name, "textures/kejim/floor");
    assert_eq!(m.stages.len(), 1);
    let s = &m.stages[0];
    // Collapsed: the texture in unit 1, the lightmap in unit 2, multiplied.
    assert_eq!(s.bundle.image, named("textures/kejim/floor"));
    assert_eq!(s.bundle2.as_ref().unwrap().image, Image::Lightmap(3));
    assert_eq!(m.multitexture, Some(TexEnv::Modulate));
    assert_eq!(
        (s.blend_src, s.blend_dst, s.depth_write),
        (None, None, true)
    );
    assert_eq!(
        (s.rgb_gen, s.alpha_gen),
        (RgbGen::Identity, AlphaGen::Identity)
    );
    assert_eq!(m.sort, sort::OPAQUE);
}

#[test]
fn implicit_materials_for_vertex_lit_and_unlit_surfaces() {
    let m = resolve("", "models/x", [LIGHTMAP_BY_VERTEX; 4], NO_STYLES);
    assert_eq!(
        (m.stages.len(), m.stages[0].rgb_gen, m.stages[0].alpha_gen),
        (1, RgbGen::ExactVertex, AlphaGen::Skip)
    );
    let m = resolve("", "models/x", [LIGHTMAP_NONE; 4], NO_STYLES);
    assert_eq!(m.stages[0].rgb_gen, RgbGen::LightingDiffuse);
}

#[test]
fn missing_images_and_broken_scripts_give_the_default_material() {
    assert!(resolve("", "textures/missing", [0, -1, -1, -1], NO_STYLES).is_default);
    let m = resolve(
        "s { { map textures/missing } }",
        "s",
        [0, -1, -1, -1],
        NO_STYLES,
    );
    assert!(m.is_default);
    assert_eq!(m.stages[0].bundle.image, Image::Default);
    assert_eq!(m.stages[0].rgb_gen, RgbGen::Bad);
    assert!(resolve("s { bogus { map a } }", "s", [0, -1, -1, -1], NO_STYLES).is_default);
}

#[test]
fn a_doubling_blend_stays_two_passes() {
    let m = resolve(
        "s { { map $lightmap\nrgbGen identity } { map tex/wall\nblendFunc GL_DST_COLOR GL_SRC_COLOR } }",
        "s",
        [2, -1, -1, -1],
        NO_STYLES,
    );
    assert_eq!(m.stages.len(), 2);
    assert_eq!(m.multitexture, None);
    assert_eq!(m.stages[0].bundle.image, Image::Lightmap(2));
    assert_eq!(m.stages[0].bundle.tc_gen, TcGen::Lightmap);
    assert_eq!(
        (m.stages[1].blend_src, m.stages[1].blend_dst),
        (Some(SrcBlend::DstColor), Some(DstBlend::SrcColor))
    );
    assert_eq!(m.stages[1].bundle.tc_gen, TcGen::Texture);
    // Stage 0 is not blended, so the blended stage does not change the sort.
    assert_eq!(m.sort, sort::OPAQUE);
}

#[test]
fn light_styles_add_a_stage_per_extra_style() {
    let m = resolve(
        "",
        "textures/a",
        [3, 5, -1, -1],
        [0, 32, STYLE_UNUSED, STYLE_UNUSED],
    );
    // Lightmap, the style's lightmap added on top, then the texture (not collapsed:
    // the first two stages' colour generators differ).
    assert_eq!(m.stages.len(), 3);
    assert_eq!(m.stages[0].bundle.image, Image::Lightmap(3));
    let style = &m.stages[1];
    assert_eq!(style.bundle.image, Image::Lightmap(5));
    assert_eq!(
        (style.bundle.lightmap_set, style.rgb_gen),
        (1, RgbGen::LightmapStyle)
    );
    assert_eq!(
        (style.blend_src, style.blend_dst),
        (Some(SrcBlend::One), Some(DstBlend::One))
    );
    assert_eq!(
        (m.stages[0].lightmap_style, m.stages[1].lightmap_style),
        (0, 32)
    );
    assert_eq!(m.stages[2].bundle.image, named("textures/a"));
}

#[test]
fn a_vertex_lit_surface_drops_a_leading_lightmap_stage() {
    let m = resolve(
        "s { { map $lightmap } { map tex/a\nblendFunc filter } }",
        "s",
        [LIGHTMAP_BY_VERTEX; 4],
        NO_STYLES,
    );
    assert_eq!(m.stages.len(), 1);
    let s = &m.stages[0];
    assert_eq!(s.bundle.image, named("tex/a"));
    assert_eq!(
        (s.blend_src, s.blend_dst, s.depth_write),
        (None, None, true)
    );
    assert_eq!(
        (s.rgb_gen, s.alpha_gen),
        (RgbGen::ExactVertex, AlphaGen::Skip)
    );
}

#[test]
fn sort_comes_from_the_blends_when_the_script_sets_none() {
    let sort_of = |src: &str| resolve(src, "s", [LIGHTMAP_NONE; 4], NO_STYLES).sort;
    assert_eq!(
        sort_of("s { { map a\nblendFunc add } { map b\nblendFunc add } }"),
        sort::BLEND1
    );
    assert_eq!(
        sort_of("s { { map a\nblendFunc blend } { map b\nblendFunc blend } }"),
        14.0
    );
    assert_eq!(
        sort_of("s { { map a\nblendFunc blend\ndepthWrite } { map b\nblendFunc blend } }"),
        sort::SEE_THROUGH
    );
    // A blended first stage sets it on its own; an unblended first stage never lets
    // later blended stages set it.
    assert_eq!(sort_of("s { { map a\nblendFunc add } }"), sort::BLEND1);
    assert_eq!(
        sort_of("s { { map a } { map b\nblendFunc add } }"),
        sort::OPAQUE
    );
    assert_eq!(sort_of("s { skyParms env/sky 512 -\n}"), sort::ENVIRONMENT);
    assert_eq!(
        sort_of("s { surfaceparm fog\nfogparms ( 1 1 1 ) 100\n}"),
        12.0
    );
    assert_eq!(sort_of("s { sort 7\n{ map a } }"), 7.0);
}

#[test]
fn drawing_stops_at_a_stage_without_an_image_but_later_stages_still_set_the_sort() {
    let m = resolve(
        "s { { map a\nblendFunc add } { videoMap missing.roq\n} { map c\nblendFunc add } }",
        "s",
        [LIGHTMAP_NONE; 4],
        NO_STYLES,
    );
    assert_eq!(m.stages.len(), 1);
    assert_eq!(m.sort, sort::BLEND1);
}

#[test]
fn names_lose_their_extension_and_out_of_range_lightmaps_become_vertex_lighting() {
    assert_eq!(strip_extension("textures/a.tga"), "textures/a");
    assert_eq!(strip_extension("textures/v1.5/a"), "textures/v1.5/a");
    let m = resolve("", "textures/a", [9, -1, -1, -1], NO_STYLES);
    assert_eq!(m.stages[0].rgb_gen, RgbGen::ExactVertex);
    // A $lightmap stage on a surface without a lightmap uses the white image.
    let m = resolve(
        "s { { map $lightmap\nblendFunc add } { map b\nblendFunc add } }",
        "s",
        [LIGHTMAP_NONE; 4],
        NO_STYLES,
    );
    assert_eq!(m.stages[0].bundle.image, Image::White);
}
