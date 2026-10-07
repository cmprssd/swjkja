//! Runs the retail default configs. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;

use swjkja_console::{Console, Handled, MULTIPLAYER_CHEATS, SINGLE_PLAYER_CHEATS};

fn commands(roots: &[PathBuf], cheats: swjkja_console::CheatCvar, file: &str) -> Vec<Vec<String>> {
    let vfs = swjkja_fs::Vfs::mount_game(roots, &["base"]).unwrap();
    let mut c = Console::new(cheats);
    c.buffer.add(&format!("exec {file}\n"));
    let mut seen = Vec::new();
    c.execute(&vfs, &mut |_, args| {
        seen.push(args.to_vec());
        Handled::Yes
    });
    seen
}

#[test]
fn retail_default_configs_run() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let roots = [gamedata];
    let mp = commands(&roots, MULTIPLAYER_CHEATS, "mpdefault");
    // "unbindall" then 72 binds (lines starting with "bind", counted in Python).
    assert_eq!(mp[0], ["unbindall"]);
    assert_eq!(mp.iter().filter(|c| c[0] == "bind").count(), 72);
    assert_eq!(mp.len(), 73);
    assert!(mp.contains(&vec!["bind".into(), "1".into(), "weapon 1".into()]));
    assert!(mp.contains(&vec!["bind".into(), "CTRL".into(), "+attack".into()]));
    // The backslash key, which the original's tokenizer also takes as a plain word.
    assert!(mp.iter().any(|c| c[0] == "bind" && c[1] == "\\"));

    let sp = commands(&roots, SINGLE_PLAYER_CHEATS, "default");
    assert_eq!(sp[0], ["unbindall"]);
    assert!(sp.contains(&vec!["bind".into(), "1".into(), "weapon 1".into()]));
}
