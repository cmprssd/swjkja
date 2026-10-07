use swjkja_image::{Format, decode_png, decode_tga, load};

/// A TGA header: image type, width, height, bits per pixel, descriptor.
fn tga(kind: u8, w: u16, h: u16, bits: u8, descriptor: u8, pixels: &[u8]) -> Vec<u8> {
    let mut b = vec![0, 0, kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    b.extend(w.to_le_bytes());
    b.extend(h.to_le_bytes());
    b.extend([bits, descriptor]);
    b.extend(pixels);
    b
}

#[test]
fn tga_bottom_up_bgr_becomes_top_down_rgba() {
    // 1x2, bottom row first: bottom = blue, top = red (stored as BGR).
    let img = decode_tga(&tga(2, 1, 2, 24, 0x00, &[255, 0, 0, 0, 0, 255])).unwrap();
    assert_eq!((img.width, img.height), (1, 2));
    assert_eq!(img.rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
}

#[test]
fn tga_top_down_with_alpha() {
    let img = decode_tga(&tga(2, 2, 1, 32, 0x20, &[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
    assert_eq!(img.rgba, [3, 2, 1, 4, 7, 6, 5, 8]);
}

#[test]
fn tga_right_to_left_flag_is_ignored_like_the_original() {
    // Descriptor 0x30 = right to left, top to bottom. The original never mirrors.
    let img = decode_tga(&tga(2, 2, 1, 24, 0x30, &[0, 0, 1, 0, 0, 2])).unwrap();
    assert_eq!(img.rgba, [1, 0, 0, 255, 2, 0, 0, 255]);
}

#[test]
fn tga_grey_in_both_type_2_and_type_3() {
    for kind in [2, 3] {
        let img = decode_tga(&tga(kind, 2, 1, 8, 0x20, &[10, 200])).unwrap();
        assert_eq!(
            img.rgba,
            [10, 10, 10, 255, 200, 200, 200, 255],
            "type {kind}"
        );
    }
}

#[test]
fn tga_rle_runs_and_raw_packets_cross_rows() {
    // 3x2 bottom-up: a run of 4 red pixels (bottom row + first of top row), then 2 raw.
    let data = [0x83, 0, 0, 255, 0x01, 0, 255, 0, 255, 0, 0];
    let img = decode_tga(&tga(10, 3, 2, 24, 0x00, &data)).unwrap();
    let px: Vec<[u8; 4]> = img.rgba.chunks(4).map(|p| p.try_into().unwrap()).collect();
    let (r, g, b) = ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]);
    assert_eq!(px, [r, g, b, r, r, r]);
}

#[test]
fn tga_rejections_match_the_original() {
    let err = |b: Vec<u8>| decode_tga(&b).unwrap_err().to_string();
    assert_eq!(
        err(tga(1, 1, 1, 8, 0, &[0])),
        "TGA: type 1 not supported (only 2, 3 and 10)"
    );
    assert_eq!(
        err(tga(2, 1, 1, 16, 0, &[0, 0])),
        "TGA: 16-bit type 2 images not supported"
    );
    assert_eq!(
        err(tga(10, 1, 1, 24, 0x20, &[0x80, 0, 0, 0])),
        "TGA: RLE images (type 10) must be bottom to top"
    );
    assert_eq!(
        err(tga(10, 1, 1, 8, 0, &[0x80, 0])),
        "TGA: 8-bit type 10 images not supported"
    );
    assert_eq!(
        err(tga(2, 4, 4, 24, 0, &[0; 10])),
        "TGA: pixel data truncated"
    );
    assert_eq!(
        err(tga(10, 4, 4, 24, 0, &[0x81])),
        "TGA: pixel data truncated"
    );
    assert_eq!(err(vec![0; 5]), "TGA: header truncated");
}

/// A PNG with the given IHDR fields and one zero-filled image (filter byte 0 per row).
fn png(w: u32, h: u32, depth: u8, colour: u8) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32fast::hash(&body).to_be_bytes());
    }
    let channels = match colour {
        2 => 3,
        6 => 4,
        0 => 1,
        _ => 1,
    };
    let row = 1 + w as usize * channels * usize::from(depth / 8).max(1);
    let raw = vec![0u8; row * h as usize];
    // zlib stream with stored (uncompressed) deflate blocks.
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(u8::from(last));
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(block);
    }
    let adler = {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in &raw {
            a = (a + u32::from(x)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    };
    z.extend(adler.to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([depth, colour, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[test]
fn png_rgb_gets_opaque_alpha() {
    let img = decode_png(&png(2, 2, 8, 2)).unwrap();
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(img.rgba, [0, 0, 0, 255].repeat(4));
}

#[test]
fn png_rejections_match_the_original() {
    let err = |b: Vec<u8>| decode_png(&b).unwrap_err().to_string();
    assert_eq!(err(png(3, 2, 8, 6)), "PNG: 3x2 is not a power of two");
    assert_eq!(
        err(png(2, 2, 8, 0)),
        "PNG: only 8-bit RGB and RGBA are supported (depth 8, colour type 0)"
    );
    assert_eq!(
        err(png(2, 2, 16, 2)),
        "PNG: only 8-bit RGB and RGBA are supported (depth 16, colour type 2)"
    );
}

fn pk3_vfs(files: &[(&str, &[u8])]) -> (tempfile::TempDir, swjkja_fs::Vfs) {
    let dir = tempfile::tempdir().unwrap();
    for (name, data) in files {
        let path = dir.path().join("base").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }
    let vfs = swjkja_fs::Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    (dir, vfs)
}

#[test]
fn load_tries_the_given_extension_then_jpg_png_tga() {
    let red = tga(2, 1, 1, 24, 0, &[0, 0, 255]);
    let green = png(1, 1, 8, 2);
    // Asked for .tga, which exists: it wins over the .png.
    let (_d, vfs) = pk3_vfs(&[("tex/a.tga", &red), ("tex/a.png", &green)]);
    assert_eq!(load(&vfs, "tex/a.tga").unwrap().path, "tex/a.tga");
    // Asked for .jpg, which is missing: .png comes before .tga.
    assert_eq!(load(&vfs, "tex/a.jpg").unwrap().path, "tex/a.png");
    // No extension at all.
    assert_eq!(load(&vfs, "tex/a").unwrap().path, "tex/a.png");
}

#[test]
fn load_skips_broken_candidates_and_reports_them() {
    let red = tga(2, 1, 1, 24, 0, &[0, 0, 255]);
    let (_d, vfs) = pk3_vfs(&[("tex/a.png", b"not a png"), ("tex/a.tga", &red)]);
    let loaded = load(&vfs, "tex/a.png").unwrap();
    assert_eq!(loaded.path, "tex/a.tga");
    assert_eq!(loaded.image.rgba, [255, 0, 0, 255]);
    assert_eq!(loaded.failures.len(), 1);
    assert_eq!(loaded.failures[0].0, "tex/a.png");
    assert!(load(&vfs, "tex/missing").unwrap_err().is_empty());
    assert_eq!(Format::from_extension("JPG"), Some(Format::Jpg));
}
