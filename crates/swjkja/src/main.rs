//! The swjkja executable. For now it only reports its version; the engine grows in
//! milestones (see docs/ROADMAP.md).

fn main() {
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("--version") | Some("-V") => println!("swjkja {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!(
                "swjkja {}: nothing to run yet; see docs/ROADMAP.md",
                env!("CARGO_PKG_VERSION")
            );
            std::process::exit(2);
        }
    }
}
