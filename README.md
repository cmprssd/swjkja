# swjkja

A new engine for **STAR WARS Jedi Knight: Jedi Academy**, written in Rust, ported from
the GPLv2 source Raven Software released in 2013 (and the community's OpenJK fork).
The goal is the same game, looking the same, running much faster: higher and steadier
frame rates, faster loading, and good performance on weak hardware such as integrated
GPUs and handhelds.

It ships no game data. It reads the assets (`GameData/base/*.pk3`) from your own
copy of the game.

Status: early. See [docs/ROADMAP.md](docs/ROADMAP.md) for the milestones and
[docs/DECISIONS.md](docs/DECISIONS.md) for why things are the way they are.

## Building

    cargo build --release          # Linux
    scripts/cargo-msvc.sh build    # Windows (MSVC), from WSL

Checks run locally with `local-ci` (see `.local-ci`), not on GitHub Actions.

## License

GPLv2 (see [LICENSE](LICENSE)), as required for a work derived from the released
Jedi Academy source. STAR WARS and Jedi Academy are trademarks of Lucasfilm; this
project is not affiliated with or endorsed by Lucasfilm, Disney, Raven Software or
Activision.
