//! Prints a map's spawn points as "x y z yaw" (multiplayer and single-player starts).
//! Usage: spawns <GameData folder> <map>
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let vfs = swjkja_fs::Vfs::mount_game(&[args[0].clone().into()], &["base"]).unwrap();
    let bsp = swjkja_bsp::Bsp::parse(&vfs.read(&args[1]).unwrap()).unwrap();
    for e in &bsp.entities {
        let class = e.get("classname").unwrap_or("");
        if matches!(
            class,
            "info_player_deathmatch"
                | "info_player_start"
                | "team_CTF_redspawn"
                | "team_CTF_bluespawn"
        ) {
            println!(
                "{class}\t{}\t{}",
                e.get("origin").unwrap_or(""),
                e.get("angle").unwrap_or("0")
            );
        }
    }
}
