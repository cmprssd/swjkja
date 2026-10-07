# Decisions

Product and technical decisions, newest last. Each entry: the question, the choice,
and why.

## 2026-10-07: Project direction (chosen by the owner)

- **Goal of "faster".** All of: higher and steadier FPS, faster loading, running well
  on weak hardware, and a modern memory-safe codebase. Every milestone is measured
  against the original game (see the baseline milestone in ROADMAP.md).
- **Starting point: a new engine in Rust,** not incremental work on OpenJK. Accepted
  trade-off: far more work before anything is playable, for a clean modern design.
- **Scope: single-player and multiplayer.**
- **Renderer: Vulkan, looking the same as the original.** Vulkan does not by itself
  change the look; the original's shader scripts, lightmaps, overbright lighting, fog
  and filtering are reproduced and checked by automated screenshot comparison against
  the original game. Visual extras are settings, off by default.
- **Game logic: all Rust from day one.** No original C code is linked in, not even
  temporarily, so the game is playable end to end only once gameplay is ported.
- **Multiplayer protocol: our own.** No compatibility with original or OpenJK servers
  and clients; the netcode is free to change for speed.
- **Platforms: Windows and Linux.**
- **Mods: asset mods (.pk3 maps, models, skins, sounds) load unchanged. Code mods are
  rewritten in Rust** against a Rust mod interface rather than run through the
  original 32-bit DLL interface: a mod must never cost performance, and rewriting a
  slow mod is preferred over supporting it as is.
- **License: GPLv2, porting from the released source.** Using Raven's and OpenJK's
  code as the reference makes this a derivative work. Game assets are never copied
  into the repository.
- **Repository: cmprssd/swjkja. Toolchain: Rust on Windows (MSVC, real GPU) and on
  Linux (WSL).**

## 2026-10-07: Reference source (decided by Claude, pending review)

The port follows OpenJK (github.com/JACoders/OpenJK) rather than the raw 2013 Raven
release: it is the same code with years of crash and portability fixes, and its
`code/` (single-player) and `codemp/` (multiplayer) trees map directly to the two
halves of the game. The reference revision is recorded in ROADMAP.md.

## 2026-10-07: Benchmark reference is OpenJK, not the retail executables (decided by Claude, pending review)

The retail `jamp.exe` crashes on every map load on the owner's PC (access violation
at `jamp.exe+0x91666`, three times out of three, with or without bots): the Intel
driver's GL_EXTENSIONS string is about 4 KB and overflows a fixed buffer in the 2003
engine, which OpenJK fixed with `TruncateGLExtensionsString`. The baseline is
therefore OpenJK built from the pinned source: the same engine and renderer with that
fix. It also never writes to the Steam install: it reads the retail assets through
`fs_cdpath` and keeps config, logs and demos under `%USERPROFILE%\.swjkja`.

Launching the retail executables directly also needs `SteamAppId=6020` in the
environment, or Steam's copy protection stops them with "Application load error
5:0000065434".

## 2026-10-07: JPEG decoding is close, not bit-exact (decided by Claude, pending review)

zune-jpeg (pure Rust) decodes the retail JPEGs within 5 of 255 levels of libjpeg
(mean 0.077, 13% of pixels touched); the other pure-Rust decoder, `jpeg-decoder`,
is not exact either. Bit-exact output would need libjpeg-turbo, a C library, in the
engine. Not worth it: the original renders textures re-compressed to S3TC/DXT by
default, which moves pixels far more than this. TGA and PNG decoding are exact.
If screenshot comparisons ever trace a visible difference to JPEG decoding, revisit.

## 2026-10-07: Shader parsing is split from resolving (decided by Claude, pending review)

The original parses a shader and loads its textures in one pass, failing the shader
when a texture is missing. swjkja-shader only parses: it fails exactly where the
original fails for a reason in the text, and returns texture names. The renderer
resolves a parsed shader against the loaded map and applies the remaining failure
rules (missing textures, `$lightmap` on a surface without a lightmap), which keeps
both halves testable without game data.

Not carried over: OpenJK's two hard-coded fixes for retail shaders (`gfx/2d/wedge`
and `gfx/menus/radar/arrow_w`, recognised by a hash of their text). They change how
those HUD elements look compared with the original; whether to keep them is a
question for the renderer milestone.
