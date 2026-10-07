use swjkja_bsp::{Bsp, SurfaceKind, parse_entities};

/// Builds a map file from raw lumps (index = lump number; missing lumps are empty).
fn map(lumps: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let mut header = b"RBSP".to_vec();
    header.extend(1i32.to_le_bytes());
    let mut body: Vec<u8> = Vec::new();
    let mut table = [(0i32, 0i32); 18];
    for (i, data) in lumps {
        table[*i] = ((8 + 18 * 8 + body.len()) as i32, data.len() as i32);
        body.extend(data);
    }
    for (ofs, len) in table {
        header.extend(ofs.to_le_bytes());
        header.extend(len.to_le_bytes());
    }
    header.extend(body);
    header
}

fn ints(v: &[i32]) -> Vec<u8> {
    v.iter().flat_map(|i| i.to_le_bytes()).collect()
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn shader(name: &str) -> Vec<u8> {
    let mut b = name.as_bytes().to_vec();
    b.resize(64, 0);
    b.extend(ints(&[0, 1]));
    b
}

fn vert(x: f32) -> Vec<u8> {
    let mut b = floats(&[x, 0.0, 0.0, 0.5, 0.25]);
    b.resize(80, 0xff);
    b
}

/// A planar surface: shader, fog, first_vert, num_verts, first_index, num_indexes.
fn surface(shader: i32, fog: i32, verts: (i32, i32), indexes: (i32, i32)) -> Vec<u8> {
    let mut b = ints(&[shader, fog, 1, verts.0, verts.1, indexes.0, indexes.1]);
    b.resize(148, 0);
    b
}

fn model(surfaces: (i32, i32)) -> Vec<u8> {
    let mut b = floats(&[0.0; 6]);
    b.extend(ints(&[surfaces.0, surfaces.1, 0, 0]));
    b
}

/// One triangle with one shader, as the world model.
fn triangle_map(indexes: &[i32]) -> Vec<u8> {
    map(&[
        (
            0,
            b"{\n\"classname\" \"worldspawn\"\n\"message\" \"Test\"\n}\n\0".to_vec(),
        ),
        (1, shader("textures/test/floor")),
        (7, model((0, 1))),
        (10, [vert(0.0), vert(1.0), vert(2.0)].concat()),
        (11, ints(indexes)),
        (13, surface(0, -1, (0, 3), (0, 3))),
    ])
}

fn parse_err(data: &[u8]) -> String {
    Bsp::parse(data).unwrap_err().to_string()
}

#[test]
fn parses_a_minimal_map() {
    let bsp = Bsp::parse(&triangle_map(&[0, 1, 2])).unwrap();
    assert_eq!(bsp.shaders[0].name(), "textures/test/floor");
    assert_eq!(bsp.shaders[0].content_flags, 1);
    assert_eq!(
        bsp.verts.iter().map(|v| v.xyz[0]).collect::<Vec<_>>(),
        [0.0, 1.0, 2.0]
    );
    assert_eq!(bsp.verts[1].st, [0.5, 0.25]);
    assert_eq!(bsp.indexes, [0, 1, 2]);
    assert_eq!(bsp.surfaces[0].kind(), SurfaceKind::Planar);
    assert_eq!(bsp.worldspawn().unwrap().get("Message"), Some("Test"));
    assert_eq!(bsp.lightmap_pages(), 0);
    assert!(bsp.visibility.can_see(0, 5));
}

#[test]
fn rejects_wrong_ident_version_and_truncation() {
    let mut data = triangle_map(&[0, 1, 2]);
    assert_eq!(
        parse_err(&data[..100]),
        "too short for a map header (100 bytes)"
    );
    data[4] = 46;
    assert_eq!(parse_err(&data), "RBSP version 46, expected 1");
    data[0..4].copy_from_slice(b"IBSP");
    assert_eq!(parse_err(&data), "not an RBSP map (ident \"IBSP\")");
}

#[test]
fn rejects_lumps_outside_the_file_or_of_the_wrong_size() {
    let mut data = triangle_map(&[0, 1, 2]);
    let len = data.len();
    data.truncate(len - 10);
    let err = parse_err(&data);
    assert!(
        err.starts_with("Surfaces lump (") && err.ends_with(", 148) is outside the file"),
        "{err}"
    );

    let bad = map(&[(2, vec![0; 17])]);
    assert_eq!(
        parse_err(&bad),
        "Planes lump is 17 bytes, not a multiple of 16"
    );
}

#[test]
fn rejects_out_of_range_references() {
    assert_eq!(
        parse_err(&triangle_map(&[0, 1, 3])),
        "surface 0: vertex index 3 out of range (0..3)"
    );
    let data = map(&[
        (1, shader("a")),
        (7, model((0, 1))),
        (10, vert(0.0)),
        (13, surface(1, -1, (0, 1), (0, 0))),
    ]);
    assert_eq!(parse_err(&data), "surface 0: shader 1 out of range (0..1)");
    let data = map(&[
        (1, shader("a")),
        (7, model((0, 2))),
        (10, vert(0.0)),
        (13, surface(0, -1, (0, 1), (0, 0))),
    ]);
    assert_eq!(
        parse_err(&data),
        "model 0: surfaces 0+2 out of range (0..1)"
    );
    let data = map(&[
        (1, shader("a")),
        (10, vert(0.0)),
        (13, surface(0, 0, (0, 1), (0, 0))),
    ]);
    assert_eq!(parse_err(&data), "surface 0: fog 0 out of range (0..0)");
}

#[test]
fn visibility_bits() {
    // 3 clusters, 1 byte each: 0 sees 0 and 2; 1 sees only 1; 2 sees all.
    let mut vis = ints(&[3, 1]);
    vis.extend([0b101, 0b010, 0b111]);
    let data = map(&[(1, shader("a")), (7, model((0, 0))), (16, vis)]);
    let bsp = Bsp::parse(&data).unwrap();
    let row = |from| {
        (0..3)
            .map(|to| bsp.visibility.can_see(from, to))
            .collect::<Vec<_>>()
    };
    assert_eq!(row(0), [true, false, true]);
    assert_eq!(row(1), [false, true, false]);
    assert_eq!(row(2), [true, true, true]);
    assert!(bsp.visibility.can_see(-1, 1));
}

#[test]
fn entity_text_with_comments_and_spaces() {
    let ents = parse_entities(
        b"// leading comment\n{\n\"classname\" \"light\" // trailing\n\"origin\" \"1 2 3\"\n}\n{\n}\n",
    )
    .unwrap();
    assert_eq!(ents.len(), 2);
    assert_eq!(
        ents[0].pairs,
        [
            ("classname".to_owned(), "light".to_owned()),
            ("origin".to_owned(), "1 2 3".to_owned())
        ]
    );
    assert_eq!(
        parse_entities(b"{ \"a\" }").unwrap_err().to_string(),
        "entity lump: key \"a\" has no value"
    );
    assert_eq!(
        parse_entities(b"{ \"a\" \"b\"").unwrap_err().to_string(),
        "entity lump: unterminated entity"
    );
}
