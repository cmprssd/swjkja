//! Reads every file the VFS provides and reports throughput.
//! Usage: read_all <GameData folder> [threads]
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let gamedata = args.next().expect("usage: read_all <GameData> [threads]");
    let threads: usize = args.next().map_or(1, |t| t.parse().unwrap());
    let t = Instant::now();
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let mount = t.elapsed();
    let mut files: Vec<&str> = vfs.files().collect();
    files.sort_unstable();
    let t = Instant::now();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let bytes: usize = std::thread::scope(|s| {
        (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut sum = 0;
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(f) = files.get(i) else { break sum };
                        sum += vfs.read(f).unwrap().len();
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .sum()
    });
    let secs = t.elapsed().as_secs_f64();
    println!(
        "mount {mount:?}; {} files, {} MB in {secs:.2} s = {:.0} MB/s on {threads} thread(s)",
        files.len(),
        bytes / 1_000_000,
        bytes as f64 / 1e6 / secs
    );
}
