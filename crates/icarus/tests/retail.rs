//! Reads every retail ICARUS script. Runs only when SWJKJA_GAMEDATA points at the
//! retail `GameData` folder (`.local-ci` sets it for the Windows run).

use std::path::PathBuf;

use swjkja_icarus::{Node, id, parse};

fn count(nodes: &[Node], blocks: &mut usize, bodies: &mut usize) {
    for n in nodes {
        *blocks += 1;
        if let Node::Body(_, body) = n {
            *bodies += 1;
            count(body, blocks, bodies);
        }
    }
}

#[test]
fn retail_scripts_parse() {
    let Some(gamedata) = std::env::var_os("SWJKJA_GAMEDATA").map(PathBuf::from) else {
        eprintln!("SWJKJA_GAMEDATA is not set; skipping the retail-data test");
        return;
    };
    let vfs = swjkja_fs::Vfs::mount_game(&[gamedata], &["base"]).unwrap();
    let mut files: Vec<&str> = vfs
        .files()
        .filter(|f| f.to_ascii_lowercase().ends_with(".ibi"))
        .collect();
    files.sort_unstable();
    let (mut blocks, mut bodies) = (0, 0);
    for f in &files {
        let nodes = parse(&vfs.read(f).unwrap()).unwrap_or_else(|e| panic!("{f}: {e}"));
        count(&nodes, &mut blocks, &mut bodies);
    }
    // Counted in Python: 22,860 blocks other than BLOCK_END, 3,127 of them opening a
    // body, each closed by one of the 3,127 BLOCK_ENDs.
    assert_eq!((files.len(), blocks, bodies), (1011, 22860, 3127));

    let sitting = parse(&vfs.read("scripts/yavin1/jedi_stud_sitting1.IBI").unwrap()).unwrap();
    let Node::Body(head, body) = &sitting[5] else {
        panic!("not a loop: {:?}", sitting[5]);
    };
    assert_eq!((head.id, head.members[0].float()), (id::LOOP, Some(-1.0)));
    assert_eq!(body.len(), 15);
}
