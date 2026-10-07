//! Decodes every texture file in a game install and prints one tab-separated line
//! per file: path, "ok" or the error, width, height, CRC-32 of the RGBA pixels.
//! With --dump DIR and a list of paths on stdin, writes each decoded image to
//! DIR/<path>.rgba instead (for comparing pixels against a reference decoder).
//! Usage: decode_all <GameData folder> [--dump DIR]
use std::io::{BufRead, Write};

use swjkja_image::{Format, decode};

fn main() {
    let mut args = std::env::args().skip(1);
    let gamedata = args
        .next()
        .expect("usage: decode_all <GameData> [--dump DIR]");
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let format = |path: &str| {
        path.rsplit_once('.')
            .and_then(|(_, e)| Format::from_extension(e))
    };

    if args.next().as_deref() == Some("--dump") {
        let dir = std::path::PathBuf::from(args.next().expect("--dump needs a folder"));
        for path in std::io::stdin().lock().lines() {
            let path = path.unwrap();
            let image = decode(format(&path).unwrap(), &vfs.read(&path).unwrap()).unwrap();
            let out = dir.join(format!("{path}.rgba"));
            std::fs::create_dir_all(out.parent().unwrap()).unwrap();
            std::fs::write(out, &image.rgba).unwrap();
        }
        return;
    }

    let mut files: Vec<&str> = vfs.files().filter(|f| format(f).is_some()).collect();
    files.sort_unstable();
    let out = std::io::stdout();
    let mut out = out.lock();
    let start = std::time::Instant::now();
    for path in files {
        let bytes = vfs.read(path).unwrap();
        match decode(format(path).unwrap(), &bytes) {
            Ok(img) => writeln!(
                out,
                "{path}\tok\t{}\t{}\t{:08x}",
                img.width,
                img.height,
                crc32fast::hash(&img.rgba)
            ),
            Err(e) => writeln!(out, "{path}\t{e}\t\t\t"),
        }
        .unwrap();
    }
    eprintln!("decoded in {:?}", start.elapsed());
}
