use swjkja_shader::*;

fn text(files: &[(&str, &str)]) -> ShaderText {
    ShaderText::from_files(
        files
            .iter()
            .map(|(n, t)| (n.to_string(), t.as_bytes().to_vec()))
            .collect(),
    )
}

fn one(src: &str) -> Result<ShaderDef, ParseError> {
    text(&[("a.shader", src)])
        .parse("s")
        .expect("shader s is defined")
}

fn ok(src: &str) -> ShaderDef {
    one(src).unwrap_or_else(|e| panic!("{e}"))
}

fn map_name(stage: &Stage) -> &str {
    match &stage.texture {
        Texture::Map { name, .. } => name,
        t => panic!("not a map: {t:?}"),
    }
}

#[test]
fn the_file_listed_last_wins_and_the_first_definition_in_a_file_wins() {
    let t = text(&[
        ("early.shader", "x { { map early } }\ny { { map early_y } }"),
        (
            "late.shader",
            "x { { map late } }\nx { { map late_second } }",
        ),
    ]);
    assert_eq!(map_name(&t.parse("x").unwrap().unwrap().stages[0]), "late");
    assert_eq!(
        map_name(&t.parse("Y").unwrap().unwrap().stages[0]),
        "early_y"
    );
    assert!(t.parse("missing").is_none());
}

#[test]
fn a_file_with_unbalanced_braces_is_ignored_entirely() {
    let t = text(&[
        ("good.shader", "x { { map good } }"),
        ("bad.shader", "x { { map bad } }\nz { { map z }"),
    ]);
    assert_eq!(map_name(&t.parse("x").unwrap().unwrap().stages[0]), "good");
    assert!(!t.contains("z"));
    assert_eq!(
        t.rejected_files,
        [(
            "bad.shader".to_owned(),
            "shader \"z\" is missing its closing brace".to_owned()
        )]
    );
}

#[test]
fn compress_cuts_words_at_comment_marks_and_joins_lines_across_block_comments() {
    // '#' ends the line even inside a word, so the name is "tex/a". (The per-file brace
    // check runs before compression, so the braces must balance with the '#' text in.)
    assert_eq!(
        map_name(&ok("s {\n{\nmap tex/a#1\n}\n}").stages[0]),
        "tex/a"
    );
    // A block comment spanning lines joins them: "GL_ZERO" stays on the blendfunc line.
    let s = ok("s { { map t\nblendfunc GL_DST_COLOR /* x\n */ GL_ZERO } }");
    assert_eq!(
        (s.stages[0].blend_src, s.stages[0].blend_dst),
        (Some(SrcBlend::DstColor), Some(DstBlend::Zero))
    );
}

#[test]
fn stage_defaults_follow_the_blend_mode() {
    let s = ok(
        "s { { map a } { map b\nblendfunc add } { map c\nblendfunc GL_ONE GL_ZERO } { map d\nblendfunc blend\ndepthwrite } }",
    );
    let [plain, add, one_zero, blend] = &s.stages[..] else {
        panic!()
    };
    assert_eq!(
        (plain.rgb_gen, plain.alpha_gen, plain.depth_write),
        (RgbGen::Identity, AlphaGen::Skip, true)
    );
    assert_eq!(
        (add.rgb_gen, add.blend_src, add.depth_write),
        (RgbGen::IdentityLighting, Some(SrcBlend::One), false)
    );
    // GL_ONE GL_ZERO is no blending at all, and writes depth again.
    assert_eq!(
        (one_zero.blend_src, one_zero.blend_dst, one_zero.depth_write),
        (None, None, true)
    );
    assert_eq!(
        (blend.rgb_gen, blend.depth_write),
        (RgbGen::IdentityLighting, true)
    );
}

#[test]
fn a_blendfunc_missing_its_second_word_keeps_depth_writes() {
    let s = ok("s { { map a\nblendfunc GL_SRC_ALPHA\n} }");
    assert_eq!(
        (
            s.stages[0].blend_src,
            s.stages[0].blend_dst,
            s.stages[0].depth_write
        ),
        (Some(SrcBlend::SrcAlpha), None, true)
    );
}

#[test]
fn rgbgen_vertex_also_sets_alphagen_vertex() {
    let s = ok("s { { map a\nrgbGen vertex } { map b\nalphaGen const 0.5\nrgbGen vertex } }");
    assert_eq!(
        (s.stages[0].rgb_gen, s.stages[0].alpha_gen),
        (RgbGen::Vertex, AlphaGen::Vertex)
    );
    assert_eq!(
        (s.stages[1].alpha_gen, s.stages[1].constant[3]),
        (AlphaGen::Const, 127)
    );
}

#[test]
fn texture_kinds() {
    // animMap and videoMap take every word to the end of their line, braces included.
    let s = ok(
        "s {\n{ map $lightmap }\n{ clampmap A.TGA }\n{ animMap 10 a b c\n}\n{ map $whiteimage }\n{ videoMap v.roq\n} }",
    );
    assert_eq!(s.stages[0].texture, Texture::Lightmap);
    assert_eq!(
        s.stages[1].texture,
        Texture::Map {
            name: "a.tga".into(),
            clamp: true
        }
    );
    assert_eq!(
        s.stages[2].texture,
        Texture::Animated {
            frames_per_second: 10.0,
            names: vec!["a".into(), "b".into(), "c".into()],
            clamp: false,
            one_shot: false
        }
    );
    assert_eq!(s.stages[3].texture, Texture::White);
    assert_eq!(s.stages[4].texture, Texture::Video("v.roq".into()));
}

#[test]
fn tc_mods_waves_and_deforms() {
    let s = ok(
        "s { deformVertexes wave 64 sin 0 3 0 0.5\ndeformVertexes texture\n{ map a\ntcMod scroll 0.5 -1\ntcMod turb 0 0.1\ntcMod rotate 30\nrgbGen wave square 0.5 0.5 0 2 } }",
    );
    assert_eq!(
        s.deforms,
        [
            Deform::Wave {
                spread: 1.0 / 64.0,
                wave: Wave {
                    func: Func::Sin,
                    base: 0.0,
                    amplitude: 3.0,
                    phase: 0.0,
                    frequency: 0.5
                }
            },
            // Any "text*" keyword is a text deform; a non-digit fifth character means text0.
            Deform::Text(0),
        ]
    );
    // The incomplete turb still counts, as an inert entry.
    assert_eq!(
        s.stages[0].tc_mods,
        [
            TexMod::Scroll([0.5, -1.0]),
            TexMod::None,
            TexMod::Rotate(30.0)
        ]
    );
    assert_eq!(
        s.stages[0].rgb_gen,
        RgbGen::Wave(Wave {
            func: Func::Square,
            base: 0.5,
            amplitude: 0.5,
            phase: 0.0,
            frequency: 2.0
        })
    );
}

#[test]
fn surface_parms_and_materials() {
    let s = ok("s { surfaceparm nonsolid\nsurfaceparm trans\nmaterial solidmetal\n{ map a } }");
    assert_eq!(s.content_flags, contents::OPAQUE | contents::TRANSLUCENT);
    assert_eq!(s.surface_flags, 3);
    let s = ok("s { surfaceparm playerclip\nsurfaceparm nodraw\n{ map a } }");
    assert_eq!(
        (s.content_flags, s.surface_flags),
        (contents::PLAYERCLIP, surface::NODRAW)
    );
}

#[test]
fn shader_level_keywords() {
    let s = ok(
        "s { qer_editorimage x y z\nq3map_lightimage foo\ncull none\nsort additive\nnopicmip\nskyparms env/sky 0 -\nfogparms ( 0.5 0.25 1 ) 300 trailing\n}",
    );
    assert_eq!(
        (s.cull, s.sort, s.no_picmip, s.no_mipmaps),
        (Cull::TwoSided, Some(sort::BLEND1), true, false)
    );
    assert_eq!(
        s.sky,
        Some(Sky {
            outer_box: Some("env/sky".into()),
            cloud_height: 512.0
        })
    );
    assert_eq!(
        s.fog_parms,
        Some(FogParms {
            color: [0.5, 0.25, 1.0],
            depth_for_opaque: 300.0
        })
    );
    assert!(s.stages.is_empty());
}

#[test]
fn failures_match_the_original() {
    assert_eq!(
        one("s { foo\n{ map a } }"),
        Err(ParseError::UnknownKeyword("foo".into()))
    );
    assert_eq!(
        one("s { { map a\nbar } }"),
        Err(ParseError::UnknownStageKeyword("bar".into()))
    );
    assert_eq!(
        one("s { { map\n} }"),
        Err(ParseError::MissingArgument("map"))
    );
    assert_eq!(one("s { cull none }"), Err(ParseError::NoStages));
    assert_eq!(
        one("s { fogparms 1 2 3\n{ map a } }"),
        Err(ParseError::BadFogParms)
    );
    assert_eq!(
        one(&format!("s {{ {} }}", "{ map a } ".repeat(9))),
        Err(ParseError::TooManyStages)
    );
    assert_eq!(
        one(&format!(
            "s {{ {{ map a\n{} }} }}",
            "tcMod scroll 1 1\n".repeat(5)
        )),
        Err(ParseError::TooManyTexMods)
    );
    // Fog volumes need no stages.
    assert!(one("s { surfaceparm fog\nfogparms ( 1 1 1 ) 100\n}").is_ok());
    // fogparms skips the rest of its line once it has read the depth, braces included.
    assert_eq!(
        one("s { surfaceparm fog\nfogparms ( 1 1 1 ) 100 }"),
        Err(ParseError::MissingClosingBrace)
    );
}

#[test]
fn a_nul_byte_ends_a_file_like_a_c_string() {
    let t = ShaderText::from_files(vec![(
        "a.shader".into(),
        b"s { { map a } }\0t { { map b } }".to_vec(),
    )]);
    assert!(t.contains("s"));
    assert!(!t.contains("t"));
}
