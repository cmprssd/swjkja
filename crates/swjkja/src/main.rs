//! The swjkja executable. So far it is the map viewer (milestone 2); the engine
//! grows in milestones (see docs/ROADMAP.md).

mod fly;
mod view;

use std::path::PathBuf;

const USAGE: &str = "usage:
  swjkja view <GameData> <map> [--vsync] [--fullscreen]
      fly through a map (e.g. mp/ffa3): mouse to look, W A S D to move, Space and C
      up and down, Shift to go faster, Esc to quit
  swjkja view <GameData> <map> --replay <path> <out> [--vsync] [--fullscreen]
      draw a recorded camera path once (bench/reference/*.path) and write it with
      this viewer's frame timestamps to <out> (for `bench_path --stats`)
  --fullscreen: exclusive fullscreen at 1280x720 instead of a window
  --timings <file>: write each frame's CPU step and GPU times to <file> on quitting
  swjkja --version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("swjkja {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("view") => view_options(&args[1..]).and_then(view::run),
        _ => Err(USAGE.to_string()),
    };
    if let Err(e) = result {
        eprintln!("swjkja: {e}");
        std::process::exit(2);
    }
}

fn view_options(args: &[String]) -> Result<view::Options, String> {
    let mut positional = Vec::new();
    let (mut vsync, mut fullscreen, mut replay, mut timings) = (false, false, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--vsync" => vsync = true,
            "--fullscreen" => fullscreen = true,
            "--timings" => {
                let Some(out) = it.next() else {
                    return Err(format!("--timings needs an output file\n{USAGE}"));
                };
                timings = Some(PathBuf::from(out));
            }
            "--replay" => {
                let (Some(file), Some(out)) = (it.next(), it.next()) else {
                    return Err(format!("--replay needs a path and an output file\n{USAGE}"));
                };
                let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
                let path = swjkja_render::path::parse(&text).map_err(|e| format!("{file}: {e}"))?;
                replay = Some((path, PathBuf::from(out)));
            }
            _ if a.starts_with("--") => return Err(format!("unknown option {a}\n{USAGE}")),
            _ => positional.push(a),
        }
    }
    let [game_data, map] = positional[..] else {
        return Err(USAGE.to_string());
    };
    Ok(view::Options {
        game_data: game_data.into(),
        map: map
            .trim_start_matches("maps/")
            .trim_end_matches(".bsp")
            .to_string(),
        vsync,
        fullscreen,
        replay,
        timings,
    })
}
