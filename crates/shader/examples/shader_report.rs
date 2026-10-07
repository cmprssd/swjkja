//! Loads and parses every shader script in a game install; prints a summary and
//! every shader the original would reject.
//! Usage: shader_report <GameData folder>
fn main() {
    let gamedata = std::env::args()
        .nth(1)
        .expect("usage: shader_report <GameData>");
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata.into()], &["base"]).unwrap();
    let start = std::time::Instant::now();
    let files = vfs.list("shaders", ".shader");
    let text = swjkja_shader::ShaderText::load(&vfs).unwrap();
    let loaded = start.elapsed();
    let mut names: Vec<&str> = text.names().collect();
    names.sort_unstable();
    let start = std::time::Instant::now();
    let mut failed = Vec::new();
    let mut slowest = Vec::new();
    for name in &names {
        let t = std::time::Instant::now();
        let result = text.parse(name).unwrap();
        slowest.push((t.elapsed(), *name));
        if let Err(e) = result {
            failed.push(format!("{name}: {e}"));
        }
    }
    slowest.sort_unstable_by(|a, b| b.cmp(a));
    println!(
        "{} files ({} rejected), {} shaders, {} rejected; load {loaded:?}, parse {:?}",
        files.len(),
        text.rejected_files.len(),
        names.len(),
        failed.len(),
        start.elapsed()
    );
    println!("slowest: {:?}", &slowest[..3]);
    println!("first files listed: {:?}", &files[..files.len().min(5)]);
    for (file, why) in &text.rejected_files {
        println!("rejected file {file}: {why}");
    }
    for f in failed {
        println!("rejected shader {f}");
    }
}
