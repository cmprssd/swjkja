//! Lists the files a game install provides, or counts them by extension.
//! Usage: list <GameData folder> [--by-extension]
fn main() {
    let mut args = std::env::args().skip(1);
    let gamedata = args
        .next()
        .expect("usage: list <GameData> [--by-extension]");
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let mut files: Vec<&str> = vfs.files().collect();
    files.sort_unstable();
    if args.next().as_deref() == Some("--by-extension") {
        let mut counts = std::collections::BTreeMap::<&str, usize>::new();
        for f in &files {
            let ext = f.rsplit_once('.').map_or("", |(_, e)| e);
            *counts.entry(ext).or_default() += 1;
        }
        for (ext, n) in counts {
            println!("{n:6} .{ext}");
        }
    } else {
        for f in files {
            println!("{f}");
        }
    }
}
