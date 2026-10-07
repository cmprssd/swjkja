use std::io::Write;
use std::path::{Path, PathBuf};

use swjkja_fs::{Error, Pk3, Vfs, normalize};

/// One zip entry for [`write_pk3`]: name, contents, deflate it?
type Item<'a> = (&'a str, &'a [u8], bool);

/// Writes a minimal zip archive, as the original tools produce for .pk3 files.
fn write_pk3(path: &Path, items: &[Item]) {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, deflate) in items {
        let packed = if *deflate {
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        } else {
            data.to_vec()
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let crc = crc32fast::hash(data);
        let offset = out.len() as u32;
        let mut local = Vec::new();
        local.extend(0x0403_4b50u32.to_le_bytes());
        local.extend(20u16.to_le_bytes()); // version needed
        local.extend(0u16.to_le_bytes()); // flags
        local.extend(method.to_le_bytes());
        local.extend([0u8; 4]); // time, date
        local.extend(crc.to_le_bytes());
        local.extend((packed.len() as u32).to_le_bytes());
        local.extend((data.len() as u32).to_le_bytes());
        local.extend((name.len() as u16).to_le_bytes());
        local.extend(0u16.to_le_bytes()); // extra
        out.extend(&local);
        out.extend(name.as_bytes());
        out.extend(&packed);

        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes()); // version made by
        central.extend(&local[4..]); // version needed .. extra length
        central.extend(0u16.to_le_bytes()); // comment length
        central.extend([0u8; 8]); // disk, internal attrs, external attrs
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let dir_offset = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // disk numbers
    out.extend((items.len() as u16).to_le_bytes());
    out.extend((items.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(dir_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // comment length
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, out).unwrap();
}

fn write_file(path: &Path, data: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

fn read_str(vfs: &Vfs, path: &str) -> String {
    String::from_utf8(vfs.read(path).unwrap()).unwrap()
}

#[test]
fn normalize_matches_the_original_lookup_rules() {
    assert_eq!(normalize("\\Maps\\MP\\FFA3.bsp"), "maps/mp/ffa3.bsp");
    assert_eq!(
        normalize("/textures/Kejim/Floor.JPG"),
        "textures/kejim/floor.jpg"
    );
}

#[test]
fn reads_stored_and_deflated_entries() {
    let dir = tempfile::tempdir().unwrap();
    let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    write_pk3(
        &dir.path().join("base/assets0.pk3"),
        &[
            ("scripts/a.shader", b"stored", false),
            ("maps/big.bsp", &big, true),
            ("maps/", b"", false),
        ],
    );
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    assert_eq!(read_str(&vfs, "scripts/a.shader"), "stored");
    assert_eq!(vfs.read("maps/big.bsp").unwrap(), big);
    let mut files: Vec<&str> = vfs.files().collect();
    files.sort();
    assert_eq!(files, ["assets0.pk3", "maps/big.bsp", "scripts/a.shader"]);
}

#[test]
fn lookups_ignore_case_and_backslashes() {
    let dir = tempfile::tempdir().unwrap();
    write_pk3(
        &dir.path().join("base/a.pk3"),
        &[("Models/Players/Kyle/Model.GLM", b"kyle", true)],
    );
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    assert_eq!(read_str(&vfs, "MODELS\\players\\kyle\\model.glm"), "kyle");
}

#[test]
fn later_pk3_wins_and_pk3_beats_loose_files_in_the_same_folder() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    write_pk3(
        &base.join("assets0.pk3"),
        &[("x.txt", b"assets0", false), ("y.txt", b"assets0", false)],
    );
    write_pk3(&base.join("Assets3.pk3"), &[("x.txt", b"assets3", true)]);
    write_file(&base.join("x.txt"), b"loose");
    write_file(&base.join("only/loose.txt"), b"loose");
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    assert_eq!(read_str(&vfs, "x.txt"), "assets3");
    assert_eq!(read_str(&vfs, "y.txt"), "assets0");
    assert_eq!(read_str(&vfs, "only/loose.txt"), "loose");
}

#[test]
fn downloaded_pk3s_sort_last() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    write_pk3(&base.join("dl_aaa.pk3"), &[("x.txt", b"downloaded", false)]);
    write_pk3(&base.join("zzz.pk3"), &[("x.txt", b"zzz", false)]);
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    assert_eq!(read_str(&vfs, "x.txt"), "downloaded");
}

#[test]
fn later_duplicate_inside_one_pk3_wins() {
    let dir = tempfile::tempdir().unwrap();
    write_pk3(
        &dir.path().join("base/a.pk3"),
        &[("x.txt", b"first", false), ("X.TXT", b"second", false)],
    );
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    assert_eq!(read_str(&vfs, "x.txt"), "second");
}

#[test]
fn mod_folder_beats_base_and_later_roots_beat_earlier_ones() {
    let install = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    write_pk3(
        &install.path().join("base/assets0.pk3"),
        &[
            ("a.txt", b"install base", false),
            ("b.txt", b"install base", false),
            ("c.txt", b"install base", false),
        ],
    );
    write_file(&home.path().join("base/b.txt"), b"home base");
    write_pk3(
        &install.path().join("mymod/m.pk3"),
        &[("c.txt", b"install mod", false)],
    );
    let roots: Vec<PathBuf> = vec![install.path().into(), home.path().into()];
    let vfs = Vfs::mount_game(&roots, &["base", "mymod"]).unwrap();
    assert_eq!(read_str(&vfs, "a.txt"), "install base");
    assert_eq!(read_str(&vfs, "b.txt"), "home base");
    assert_eq!(read_str(&vfs, "c.txt"), "install mod");
}

#[test]
fn mounting_the_same_folder_twice_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write_pk3(&dir.path().join("base/a.pk3"), &[("x.txt", b"x", false)]);
    let root = dir.path().to_path_buf();
    let vfs = Vfs::mount_game(&[root.clone(), root], &["base", "base"]).unwrap();
    assert_eq!(vfs.pk3s().len(), 1);
}

#[test]
fn missing_files_and_folders() {
    let dir = tempfile::tempdir().unwrap();
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    assert!(matches!(vfs.read("Nope.txt"), Err(Error::NotFound(n)) if n == "nope.txt"));
    assert!(!vfs.contains("nope.txt"));
}

#[test]
fn corrupt_data_is_reported_not_returned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("base/a.pk3");
    write_pk3(&path, &[("x.txt", b"hello world", false)]);
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.windows(5).position(|w| w == b"hello").unwrap();
    bytes[at] = b'j';
    std::fs::write(&path, bytes).unwrap();
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    let err = vfs.read("x.txt").unwrap_err().to_string();
    assert!(err.ends_with("a.pk3: x.txt: CRC mismatch"), "{err}");
}

#[test]
fn non_zip_files_are_rejected_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fake.pk3");
    std::fs::write(&path, b"short").unwrap();
    let err = Pk3::open(&path).unwrap_err().to_string();
    assert!(
        err.ends_with("fake.pk3: not a zip file (too short)"),
        "{err}"
    );
    std::fs::write(&path, vec![7u8; 100]).unwrap();
    let err = Pk3::open(&path).unwrap_err().to_string();
    assert!(
        err.ends_with("not a zip file (no end of central directory)"),
        "{err}"
    );
}

#[test]
fn list_follows_the_original_unsorted_search_order() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base");
    write_pk3(
        &base.join("assets0.pk3"),
        &[
            ("shaders/zeta.shader", b"", false),
            ("shaders/alpha.shader", b"", false),
            ("shaders/sub/deep.shader", b"", false),
            ("shaders/a/b/too_deep.shader", b"", false),
            ("shaders/notes.txt", b"", false),
        ],
    );
    write_pk3(
        &base.join("assets1.pk3"),
        &[
            ("Shaders/Beta.shader", b"", false),
            ("shaders/alpha.shader", b"", false),
        ],
    );
    write_file(&base.join("shaders/loose.shader"), b"");
    write_file(&base.join("shaders/aaa.shader"), b"");
    let vfs = Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    // assets1 (higher priority) first, then assets0 in its directory order, then the
    // loose folder; alpha is listed once, where it first appears.
    assert_eq!(
        vfs.list("shaders", ".shader"),
        [
            "beta.shader",
            "alpha.shader",
            "zeta.shader",
            "sub/deep.shader",
            "aaa.shader",
            "loose.shader"
        ]
    );
}
