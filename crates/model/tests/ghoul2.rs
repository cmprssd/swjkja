use swjkja_model::gla::{Gla, IDENTITY, Mat34, ROOT, multiply, uncompress};
use swjkja_model::glm::{Glm, SURFACE_IS_BOLT, SURFACE_NO_DESCENDANTS, SURFACE_OFF, skin};
use swjkja_model::skin::Skin;

fn ints(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|i| i.to_le_bytes()).collect()
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn name(s: &str, n: usize) -> Vec<u8> {
    let mut b = s.as_bytes().to_vec();
    b.resize(n, 0);
    b
}

fn close(a: &Mat34, b: &Mat34) -> bool {
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .all(|(x, y)| (x - y).abs() < 1e-3)
}

/// A compressed bone: quaternion (w, x, y, z) and translation, as `MC_UnCompressQuat`
/// reads them back.
fn comp(q: [f32; 4], t: [f32; 3]) -> [u8; 14] {
    let q = q.map(|v| ((v + 2.0) * 16383.0).round() as u16);
    let t = t.map(|v| ((v + 512.0) * 64.0).round() as u16);
    let mut b = [0; 14];
    for (i, v) in q.iter().chain(&t).enumerate() {
        b[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
    }
    b
}

/// A skeleton with `parents` (-1 for the root) and one frame, bone `b` posed by
/// `bones[b]`.
fn gla(parents: &[i32], bones: &[[u8; 14]]) -> Vec<u8> {
    let n = parents.len();
    let skel_bytes = 64 + 4 + 4 + 48 * 2 + 4;
    let ofs_skel = 100 + 4 * n;
    let ofs_frames = ofs_skel + n * skel_bytes;
    let ofs_pool = (ofs_frames + 3 * n + 3) & !3;
    let ofs_end = ofs_pool + 14 * n;
    let mut b = b"2LGA".to_vec();
    b.extend(ints(&[6]));
    b.extend(name("test", 64));
    b.extend(floats(&[1.0]));
    b.extend(ints(&[
        1,
        ofs_frames as i32,
        n as i32,
        ofs_pool as i32,
        ofs_skel as i32,
        ofs_end as i32,
    ]));
    b.extend(ints(
        &(0..n)
            .map(|i| (4 * n + i * skel_bytes) as i32)
            .collect::<Vec<_>>(),
    ));
    for (i, &parent) in parents.iter().enumerate() {
        b.extend(name(&format!("bone{i}"), 64));
        b.extend(ints(&[0, parent]));
        b.extend(floats(&IDENTITY.concat()));
        b.extend(floats(&IDENTITY.concat()));
        b.extend(ints(&[0]));
    }
    // Frame 0 points bone i at pool entry i.
    for i in 0..n {
        b.extend(&(i as u32).to_le_bytes()[..3]);
    }
    b.resize(ofs_pool, 0);
    for bone in bones {
        b.extend(bone);
    }
    b
}

fn translation(t: [f32; 3]) -> Mat34 {
    std::array::from_fn(|r| std::array::from_fn(|c| if c == 3 { t[r] } else { IDENTITY[r][c] }))
}

#[test]
fn compressed_bones_decode_to_rotation_and_translation() {
    // A quarter turn about z, moved (1, 2, 3).
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let m = uncompress(&comp([h, 0.0, 0.0, h], [1.0, 2.0, 3.0]));
    let expected = [
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 1.0, 3.0],
    ];
    assert!(close(&m, &expected), "{m:?}");
}

#[test]
fn the_default_skeleton_is_one_bone_at_the_root() {
    let gla = Gla::default_skeleton();
    assert_eq!((gla.bones.len(), gla.frame_count), (1, 1));
    assert_eq!(gla.bones[0].parent, None);
    assert!(close(&gla.local(0, 0), &IDENTITY));
    // Every pose starts from the renderer's quarter turn: the mesh's -y forward.
    assert!(close(&gla.pose(0)[0], &ROOT));
}

#[test]
fn poses_chain_parents_listed_after_their_children() {
    // Bone 1's parent is bone 2, listed after it.
    let t = |x| comp([1.0, 0.0, 0.0, 0.0], [x, 0.0, 0.0]);
    let gla = Gla::parse(&gla(&[-1, 2, 0], &[t(1.0), t(10.0), t(100.0)])).unwrap();
    let pose = gla.pose(0);
    // Each bone moves along the parent's x, which the root turns to +y.
    let at = |m: &Mat34| [m[0][3], m[1][3], m[2][3]];
    assert_eq!(at(&pose[0]), [0.0, 1.0, 0.0]);
    assert_eq!(at(&pose[2]), [0.0, 101.0, 0.0]);
    assert_eq!(at(&pose[1]), [0.0, 111.0, 0.0]);
    assert!(close(
        &pose[1],
        &multiply(&ROOT, &translation([111.0, 0.0, 0.0]))
    ));
}

#[test]
fn skeletons_with_a_second_root_or_a_parent_loop_are_refused() {
    let t = comp([1.0, 0.0, 0.0, 0.0], [0.0; 3]);
    let err = Gla::parse(&gla(&[-1, -1], &[t, t])).unwrap_err();
    assert_eq!(err.0, "bone 1 (bone1): parent -1");
    let err = Gla::parse(&gla(&[-1, 2, 1], &[t, t, t])).unwrap_err();
    assert_eq!(err.0, "bone 1 (bone1): its parents loop");
}

/// A hierarchy entry: name, flags, shader, parent, children.
type Hier<'a> = (&'a str, u32, &'a str, i32, &'a [i32]);

/// One triangle: vertex `v` at x = v, weighted by `weights` (packed word and low
/// bytes), with `refs` as the surface's bone references.
fn surface(index: i32, refs: &[i32], weights: (u32, [u8; 4])) -> Vec<u8> {
    let ofs_refs = 40;
    let ofs_triangles = ofs_refs + 4 * refs.len();
    let ofs_verts = ofs_triangles + 12;
    let ofs_end = ofs_verts + 3 * (32 + 8);
    let mut b = ints(&[
        0,
        index,
        0,
        3,
        ofs_verts as i32,
        1,
        ofs_triangles as i32,
        refs.len() as i32,
        ofs_refs as i32,
        ofs_end as i32,
    ]);
    b.extend(ints(refs));
    b.extend(ints(&[0, 1, 2]));
    for v in 0..3 {
        b.extend(floats(&[0.0, 0.0, 1.0, v as f32, 0.0, 0.0]));
        b.extend(weights.0.to_le_bytes());
        b.extend(weights.1);
    }
    b.extend(floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
    b
}

/// A mesh on the default skeleton of `bones` bones, with one level of `surfaces`.
fn glm(hierarchy: &[Hier], bones: i32, surfaces: &[Vec<u8>]) -> Vec<u8> {
    let n = hierarchy.len();
    let mut hier: Vec<u8> = Vec::new();
    let mut offsets = Vec::new();
    for &(surface_name, flags, shader, parent, children) in hierarchy {
        offsets.push((4 * n + hier.len()) as i32);
        hier.extend(name(surface_name, 64));
        hier.extend(flags.to_le_bytes());
        hier.extend(name(shader, 64));
        hier.extend(ints(&[0, parent, children.len() as i32]));
        hier.extend(ints(children));
    }
    let ofs_lods = 164 + 4 * n + hier.len();
    let mut lod: Vec<u8> = Vec::new();
    let mut surf_offsets = Vec::new();
    for s in surfaces {
        surf_offsets.push((4 * n + lod.len()) as i32);
        lod.extend(s);
    }
    let lod_end = 4 + 4 * n + lod.len();
    let mut b = b"2LGM".to_vec();
    b.extend(ints(&[6]));
    b.extend(name("test.glm", 64));
    b.extend(name("*default", 64));
    b.extend(ints(&[
        0,
        bones,
        1,
        ofs_lods as i32,
        n as i32,
        164 + 4 * n as i32,
        (ofs_lods + lod_end) as i32,
    ]));
    b.extend(ints(&offsets));
    b.extend(hier);
    b.extend(ints(&[lod_end as i32]));
    b.extend(ints(&surf_offsets));
    b.extend(lod);
    b
}

const ONE_WEIGHT: (u32, [u8; 4]) = (0, [0; 4]);

#[test]
fn drawing_walks_the_hierarchy_skipping_tags_and_hidden_branches() {
    let hierarchy: [Hier; 6] = [
        ("Body", 0, "models/body", -1, &[1, 2, 4]),
        ("*flash", SURFACE_IS_BOLT, "[nomaterial]", 0, &[]),
        ("arm_off", SURFACE_OFF, "", 0, &[3]),
        ("hand", 0, "models/hand", 2, &[]),
        ("cap", SURFACE_OFF | SURFACE_NO_DESCENDANTS, "", 0, &[5]),
        ("feather", 0, "", 4, &[]),
    ];
    let surfaces: Vec<_> = (0..6).map(|i| surface(i, &[0], ONE_WEIGHT)).collect();
    let glm = Glm::parse(&glm(&hierarchy, 1, &surfaces)).unwrap();
    // Lowercased, "_off" cut off; "[nomaterial]" is no shader.
    let names: Vec<_> = glm.surfaces.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["body", "*flash", "arm", "hand", "cap", "feather"]);
    assert_eq!(glm.surfaces[1].shader, "");
    // An off surface still leads to its children, unless it hides them.
    assert_eq!(glm.drawn_surfaces(), [0, 3]);
}

#[test]
fn skins_and_switches_turn_surfaces_off_and_on() {
    let hierarchy: [Hier; 6] = [
        ("Body", 0, "models/body", -1, &[1, 2, 4]),
        ("*flash", SURFACE_IS_BOLT, "[nomaterial]", 0, &[]),
        ("arm_off", SURFACE_OFF, "", 0, &[3]),
        ("hand", 0, "models/hand", 2, &[]),
        ("cap", SURFACE_OFF | SURFACE_NO_DESCENDANTS, "", 0, &[5]),
        ("feather", 0, "", 4, &[]),
    ];
    let surfaces: Vec<_> = (0..6).map(|i| surface(i, &[0], ONE_WEIGHT)).collect();
    let glm = Glm::parse(&glm(&hierarchy, 1, &surfaces)).unwrap();
    // A skin hides "hand"; its "*off" for "arm", off already, changes nothing.
    let mut skin = Skin::default();
    skin.add(b"body,models/skin/body.tga\nhand,*off\narm,*off\n");
    let mut flags = glm.skinned_flags(&skin);
    assert_eq!(glm.drawn_with(&flags), [0]);
    // Turning a surface on clears its hiding of descendants too: "cap" and below.
    assert!(glm.set_surface_off(&mut flags, "CAP", false));
    assert_eq!(glm.drawn_with(&flags), [0, 4, 5]);
    // Turning off hides the surface alone; its children still draw.
    assert!(glm.set_surface_off(&mut flags, "body", true));
    assert_eq!(glm.drawn_with(&flags), [4, 5]);
    // A name no surface has (switch lists are split on whitespace, not commas).
    assert!(!glm.set_surface_off(&mut flags, "hand,cap", true));
}

#[test]
fn skin_files_pair_surfaces_with_shaders_like_the_original() {
    let mut skin = Skin::default();
    skin.add(
        b"// comment\nHead,models/a/head.tga\n\"torso\",\"models/a/t.tga\"\n\
          l_cap_off,models/a/caps.tga\nr_cap_off,*off\nhead,models/b/head.tga\n\
          tag_weapon,models/a/x.tga\n",
    );
    // Lowercased names; an "_off" name loses the suffix, or is skipped when its
    // shader is "*off"; a tag is skipped and its shader read as the next name.
    assert_eq!(
        skin.surfaces,
        [
            ("head".into(), "models/a/head.tga".into()),
            ("torso".into(), "models/a/t.tga".into()),
            ("l_cap".into(), "models/a/caps.tga".into()),
            ("head".into(), "models/b/head.tga".into()),
            ("models/a/x.tga".into(), "".into()),
        ]
    );
    // The first entry for a name wins.
    assert_eq!(skin.shader("head"), Some("models/a/head.tga"));
    assert_eq!(skin.shader("legs"), None);
    // A token starting at a comma keeps it (`CommaParse`): spaces before a comma
    // shift the pairs.
    let mut skin = Skin::default();
    skin.add(b"hips , models/a/h.tga\n");
    assert_eq!(
        skin.surfaces,
        [
            ("hips".into(), ",".into()),
            ("models/a/h.tga".into(), "".into())
        ]
    );
}

#[test]
fn vertices_blend_their_bones_the_last_taking_what_is_left() {
    // Two weights (count - 1 = 1 in bits 30-31), on references 1 then 0 (5 bits
    // each); the first weight is 0x1ff / 1023: low byte 0xff, top bits 01 at bit 20.
    let packed = (1 << 30) | (1 << 20) | 1;
    let hierarchy: [Hier; 1] = [("body", 0, "", -1, &[])];
    let surfaces = [surface(0, &[0, 2], (packed, [0xff, 0, 0, 0]))];
    let glm = Glm::parse(&glm(&hierarchy, 3, &surfaces)).unwrap();
    let w = 0x1ff as f32 / 1023.0;
    let v = &glm.lods[0][0].verts[1];
    assert_eq!(v.bones, [(2, w), (0, 1.0 - w)]);
    // Bone 2 moves 10 units up; bone 0 stays: the vertex rises by the first weight.
    let pose = [IDENTITY, IDENTITY, translation([0.0, 0.0, 10.0])];
    let (xyz, normals) = skin(&glm.lods[0][0], &pose);
    assert!((xyz[1][2] - 10.0 * w).abs() < 1e-5, "{:?}", xyz[1]);
    assert!((xyz[1][0] - 1.0).abs() < 1e-6, "{:?}", xyz[1]);
    assert_eq!(normals[1], [0.0, 0.0, 1.0]);
}

#[test]
fn meshes_with_bad_references_are_refused() {
    let hierarchy: [Hier; 1] = [("body", 0, "", -1, &[])];
    // A weight on reference 1 of a surface with one reference.
    let surfaces = [surface(0, &[0], (1, [0; 4]))];
    let err = Glm::parse(&glm(&hierarchy, 1, &surfaces)).unwrap_err();
    assert_eq!(
        err.0,
        "level 0 surface 0: vertex 0 weights bone reference 1 of 1"
    );
    // A bone reference past the skeleton.
    let surfaces = [surface(0, &[4], ONE_WEIGHT)];
    let err = Glm::parse(&glm(&hierarchy, 4, &surfaces)).unwrap_err();
    assert_eq!(err.0, "level 0 surface 0: bone reference 4 of 4");
    // A child that names another parent.
    let hierarchy: [Hier; 2] = [("body", 0, "", -1, &[1]), ("arm", 0, "", -1, &[])];
    let surfaces: Vec<_> = (0..2).map(|i| surface(i, &[0], ONE_WEIGHT)).collect();
    let err = Glm::parse(&glm(&hierarchy, 1, &surfaces)).unwrap_err();
    assert_eq!(err.0, "surface 0 (body): child 1 has another parent");
}
