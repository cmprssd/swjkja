//! Decodes every texture in the retail data. Runs only when SWJKJA_GAMEDATA points at
//! the retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;

use swjkja_image::{Format, decode};

#[test]
fn every_retail_texture_decodes() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let format = |p: &str| {
        p.rsplit_once('.')
            .and_then(|(_, e)| Format::from_extension(e))
    };
    let mut files: Vec<&str> = vfs.files().filter(|f| format(f).is_some()).collect();
    files.sort_unstable();
    for f in &files {
        decode(format(f).unwrap(), &vfs.read(f).unwrap()).unwrap_or_else(|e| panic!("{f}: {e}"));
    }
    // 4271 .jpg + 1134 .png + 470 .tga, counted with .NET's ZipFile and Pillow.
    assert_eq!(files.len(), 5875);

    // Size and CRC-32 of the RGBA pixels as Pillow (libpng) and the reference TGA
    // decoder in bench/images/reference.py produce them: TGA and PNG match exactly.
    let crc = |path: &str| {
        let img = decode(format(path).unwrap(), &vfs.read(path).unwrap()).unwrap();
        (
            img.width,
            img.height,
            format!("{:08x}", crc32fast::hash(&img.rgba)),
        )
    };
    assert_eq!(crc("fonts/anewhope.tga"), (512, 256, "c33bb8cb".into()));
    assert_eq!(crc("gfx/2d/battery.tga"), (64, 128, "8b6894fb".into()));
    assert_eq!(crc("fonts/aurabesh.png"), (512, 256, "8ca438e3".into()));
    assert_eq!(crc("gfx/chunks/chain1.png"), (128, 128, "728db073".into()));
}
