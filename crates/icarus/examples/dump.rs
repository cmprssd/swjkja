//! Prints scripts as the sequencer nests them, one block per line: its id and its
//! members (strings quoted, floats as numbers, other tokens by id).
//! Usage: dump <GameData folder> <script>... (paths under scripts/, no extension)
use swjkja_icarus::{Node, parse};

fn print(nodes: &[Node], depth: usize) {
    for n in nodes {
        let (b, body) = match n {
            Node::Command(b) => (b, None),
            Node::Body(b, body) => (b, Some(body)),
        };
        let members: Vec<String> = b
            .members
            .iter()
            .map(|m| match (m.text(), m.float()) {
                (Some(t), _) => format!("{t:?}"),
                (_, Some(f)) => f.to_string(),
                _ => format!("<{}>", m.id),
            })
            .collect();
        println!("{}{} {}", "  ".repeat(depth), b.id, members.join(" "));
        if let Some(body) = body {
            print(body, depth + 1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let vfs = swjkja_fs::Vfs::mount_game(&[args[0].clone().into()], &["base"]).unwrap();
    for name in &args[1..] {
        println!("== {name}");
        match vfs.read(&format!("scripts/{name}.IBI")).map(|b| parse(&b)) {
            Ok(Ok(nodes)) => print(&nodes, 0),
            Ok(Err(e)) => println!("error: {e}"),
            Err(e) => println!("not found: {e:?}"),
        }
    }
}
