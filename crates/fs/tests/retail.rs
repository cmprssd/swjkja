//! Checks against the retail game data. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;
use std::time::Instant;

use swjkja_fs::Vfs;

fn gamedata() -> Option<PathBuf> {
    match std::env::var_os("SWJKJA_GAMEDATA") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            assert!(
                dir.join("base").is_dir(),
                "SWJKJA_GAMEDATA={} has no base folder",
                dir.display()
            );
            Some(dir)
        }
        None => {
            eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
            None
        }
    }
}

#[test]
fn every_retail_file_reads_and_overrides_resolve() {
    let Some(gamedata) = gamedata() else { return };
    let start = Instant::now();
    let vfs = Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let mount = start.elapsed();

    let archives: Vec<(String, usize)> = vfs
        .pk3s()
        .iter()
        .map(|p| {
            (
                p.path().file_name().unwrap().to_string_lossy().into_owned(),
                p.entries().len(),
            )
        })
        .collect();
    // File entries only (folder entries are skipped); counted independently with .NET's
    // ZipFile: 14753 + 593 folders, 7973 + 347, 53 + 9, 10 + 6.
    assert_eq!(
        archives,
        [
            ("assets0.pk3".into(), 14753),
            ("assets1.pk3".into(), 7973),
            ("assets2.pk3".into(), 53),
            ("assets3.pk3".into(), 10)
        ]
        .map(|(n, c): (String, usize)| (n, c))
    );

    // assets3.pk3 (a patch) replaces ctf1.bsp from assets0.pk3.
    assert_eq!(vfs.read("maps/mp/ctf1.bsp").unwrap().len(), 8_269_312);

    let mut files: Vec<&str> = vfs.files().collect();
    files.sort_unstable();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let start = Instant::now();
    let bytes: usize = std::thread::scope(|s| {
        files
            .chunks(files.len().div_ceil(threads))
            .map(|chunk| {
                let vfs = &vfs;
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|f| vfs.read(f).unwrap_or_else(|e| panic!("{e}")).len())
                        .sum::<usize>()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .sum()
    });
    eprintln!(
        "mounted {} archives in {mount:?}; read {} files ({} MB) in {:?} on {threads} threads",
        archives.len(),
        files.len(),
        bytes / 1_000_000,
        start.elapsed()
    );
}
