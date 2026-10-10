use swjkja_model::md3::{Md3, decode_normal, lod_file};

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

/// One triangle surface with three vertices, `frames` frames (vertex x = 64 * frame
/// + 64 * vertex, so 1 unit per frame and vertex), and the given shaders.
fn surface(surface_name: &str, frames: usize, shaders: &[&str], indexes: [i32; 3]) -> Vec<u8> {
    let ofs_shaders = 108;
    let ofs_triangles = ofs_shaders + 68 * shaders.len();
    let ofs_st = ofs_triangles + 12;
    let ofs_xyz = ofs_st + 3 * 8;
    let ofs_end = ofs_xyz + frames * 3 * 8;
    let mut b = b"IDP3".to_vec();
    b.extend(name(surface_name, 64));
    b.extend(ints(&[
        0,
        frames as i32,
        shaders.len() as i32,
        3,
        1,
        ofs_triangles as i32,
        ofs_shaders as i32,
        ofs_st as i32,
        ofs_xyz as i32,
        ofs_end as i32,
    ]));
    for s in shaders {
        b.extend(name(s, 64));
        b.extend(ints(&[0]));
    }
    b.extend(ints(&indexes));
    b.extend(floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
    for f in 0..frames {
        for v in 0..3 {
            let x = (64 * (f + v)) as i16;
            for c in [x, -128, 32] {
                b.extend(c.to_le_bytes());
            }
            // Latitude 0, longitude 64 (a quarter turn): the +x axis.
            b.extend(64i16.to_le_bytes());
        }
    }
    assert_eq!(b.len(), ofs_end);
    b
}

fn model(frames: usize, surfaces: &[Vec<u8>]) -> Vec<u8> {
    let tags = 1;
    let ofs_frames = 108;
    let ofs_tags = ofs_frames + 56 * frames;
    let ofs_surfaces = ofs_tags + 112 * tags * frames;
    let body: usize = surfaces.iter().map(Vec::len).sum();
    let mut b = b"IDP3".to_vec();
    b.extend(ints(&[15]));
    b.extend(name("test", 64));
    b.extend(ints(&[
        0,
        frames as i32,
        tags as i32,
        surfaces.len() as i32,
        0,
        ofs_frames as i32,
        ofs_tags as i32,
        ofs_surfaces as i32,
        (ofs_surfaces + body) as i32,
    ]));
    for f in 0..frames {
        b.extend(floats(&[
            -1.0, -2.0, -3.0, 1.0, 2.0, 3.0, 0.5, 0.0, 0.0, 4.0,
        ]));
        b.extend(name(&format!("frame{f}"), 16));
    }
    for f in 0..frames {
        b.extend(name("tag_flash", 64));
        b.extend(floats(&[f as f32, 0.0, 0.0]));
        b.extend(floats(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    }
    for s in surfaces {
        b.extend(s);
    }
    b
}

#[test]
fn parses_frames_tags_and_surfaces() {
    let data = model(
        2,
        &[
            surface("Hull_1", 2, &["models/a/hull"], [0, 1, 2]),
            surface(
                "glass",
                2,
                &["models/a/glass", "models/a/glass2"],
                [2, 1, 0],
            ),
        ],
    );
    let md3 = Md3::parse(&data).unwrap();
    assert_eq!(md3.name, "test");
    assert_eq!(md3.frames.len(), 2);
    assert_eq!(md3.frames[1].name, "frame1");
    assert_eq!(md3.frames[0].bounds, [[-1.0, -2.0, -3.0], [1.0, 2.0, 3.0]]);
    assert_eq!(md3.frames[0].local_origin, [0.5, 0.0, 0.0]);
    assert_eq!(md3.frames[0].radius, 4.0);
    assert_eq!(md3.tags_per_frame, 1);
    assert_eq!(md3.tags[1].name, "tag_flash");
    assert_eq!(md3.tags[1].origin, [1.0, 0.0, 0.0]);

    let [hull, glass] = &md3.surfaces[..] else {
        panic!("two surfaces");
    };
    // Lowercased, and the "_1" cut off.
    assert_eq!(hull.name, "hull");
    assert_eq!(hull.shaders, ["models/a/hull"]);
    assert_eq!(glass.shaders, ["models/a/glass", "models/a/glass2"]);
    assert_eq!(glass.indexes, [2, 1, 0]);
    assert_eq!(hull.st, [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
    let (xyz, normals) = hull.frame(1);
    assert_eq!(xyz, [[1.0, -2.0, 0.5], [2.0, -2.0, 0.5], [3.0, -2.0, 0.5]]);
    assert_eq!(normals[0], decode_normal(64));
}

#[test]
fn surface_name_loses_any_underscore_suffix_but_not_a_short_name() {
    let one = |n: &str| {
        let data = model(1, &[surface(n, 1, &["s"], [0, 1, 2])]);
        Md3::parse(&data).unwrap().surfaces[0].name.clone()
    };
    assert_eq!(one("w_x"), "w");
    assert_eq!(one("_x"), "_x");
    assert_eq!(one("hull_10"), "hull_10");
}

#[test]
fn normals_decode_from_latitude_and_longitude() {
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
    // Longitude 0: straight up, whatever the latitude.
    assert!(close(decode_normal(0), [0.0, 0.0, 1.0]));
    assert!(close(decode_normal(0x4000), [0.0, 0.0, 1.0]));
    // Longitude 64 (90 degrees) at latitude 0 and 64: +x, then +y.
    assert!(close(decode_normal(64), [1.0, 0.0, 0.0]));
    assert!(close(decode_normal(0x4040), [0.0, 1.0, 0.0]));
    // Longitude 128: straight down.
    assert!(close(decode_normal(128), [0.0, 0.0, -1.0]));
}

#[test]
fn refuses_what_the_original_refuses() {
    let good = model(1, &[surface("s", 1, &["s"], [0, 1, 2])]);
    let mut wrong_version = good.clone();
    wrong_version[4] = 16;
    assert!(
        Md3::parse(&wrong_version)
            .unwrap_err()
            .0
            .contains("version 16")
    );

    let no_frames = model(0, &[]);
    assert!(Md3::parse(&no_frames).unwrap_err().0.contains("no frames"));

    let bad_index = model(1, &[surface("s", 1, &["s"], [0, 1, 3])]);
    assert!(
        Md3::parse(&bad_index)
            .unwrap_err()
            .0
            .contains("vertex index 3")
    );

    for cut in [50, good.len() - 1] {
        assert!(Md3::parse(&good[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn level_of_detail_file_names() {
    assert_eq!(
        lod_file("models/map_objects/a.md3", 0),
        "models/map_objects/a.md3"
    );
    assert_eq!(
        lod_file("models/map_objects/a.md3", 2),
        "models/map_objects/a_2.md3"
    );
    assert_eq!(lod_file("models/a", 1), "models/a_1.md3");
}
