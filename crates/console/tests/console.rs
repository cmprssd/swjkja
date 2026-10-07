use swjkja_console::*;

fn vfs_with(files: &[(&str, &str)]) -> (tempfile::TempDir, swjkja_fs::Vfs) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("base")).unwrap();
    for (name, text) in files {
        let path = dir.path().join("base").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let vfs = swjkja_fs::Vfs::mount_game(&[dir.path().to_path_buf()], &["base"]).unwrap();
    (dir, vfs)
}

/// Runs `text` through the buffer; engine commands are recorded, not handled.
fn run(console: &mut Console, vfs: &swjkja_fs::Vfs, text: &str) -> Vec<Vec<String>> {
    console.buffer.add(text);
    let mut seen = Vec::new();
    console.execute(vfs, &mut |_, args| {
        if args[0] == "engine" {
            seen.push(args.to_vec());
            Handled::Yes
        } else {
            Handled::No
        }
    });
    seen
}

#[test]
fn tokenize_like_cmd_tokenizestring() {
    assert_eq!(
        tokenize("bind x \"say hi; jump\""),
        ["bind", "x", "say hi; jump"]
    );
    assert_eq!(tokenize("a\"b c\"d // gone"), ["a", "b c", "d"]);
    assert_eq!(tokenize("a /* skip */ b/*c*/"), ["a", "b"]);
    assert_eq!(tokenize("  "), Vec::<String>::new());
    assert_eq!(
        args_from(&tokenize("set name Kyle  Katarn"), 2),
        "Kyle Katarn"
    );
}

#[test]
fn the_buffer_splits_at_semicolons_and_line_breaks_outside_quotes_and_comments() {
    let (_d, vfs) = vfs_with(&[]);
    let mut c = Console::new(SINGLE_PLAYER_CHEATS);
    let seen = run(
        &mut c,
        &vfs,
        "engine a; engine \"b;c\"\nengine d // ; engine e\nengine f /* ;\n */ x\n",
    );
    assert_eq!(
        seen,
        [
            vec!["engine", "a"],
            vec!["engine", "b;c"],
            vec!["engine", "d"],
            vec!["engine", "f"]
        ]
    );
    // The end of a /* */ comment also ends the command, so "x" runs on its own.
    assert_eq!(c.events, [Event::Unknown("x".into())]);
}

#[test]
fn wait_pauses_until_the_next_frame() {
    let (_d, vfs) = vfs_with(&[]);
    let mut c = Console::new(SINGLE_PLAYER_CHEATS);
    assert_eq!(run(&mut c, &vfs, "engine 1; wait 2; engine 2\n").len(), 1);
    // Each execute is one frame. "wait 2" uses up one count in the frame it runs in
    // (as Cbuf_Execute does), so one more frame is skipped and the next one runs.
    assert_eq!(run(&mut c, &vfs, "").len(), 0);
    assert_eq!(run(&mut c, &vfs, ""), [vec!["engine", "2"]]);
}

#[test]
fn set_variants_create_and_flag_cvars() {
    let (_d, vfs) = vfs_with(&[]);
    let mut c = Console::new(SINGLE_PLAYER_CHEATS);
    run(
        &mut c,
        &vfs,
        "seta name Kyle Katarn\nsetu model kyle\nset plain 1\nplain 2\nnobody\n",
    );
    let name = c.cvars.find("NAME").unwrap();
    assert_eq!(
        (name.string.as_str(), name.flags),
        ("Kyle Katarn", flags::USER_CREATED | flags::ARCHIVE)
    );
    assert_eq!(
        c.cvars.find("model").unwrap().flags & flags::USERINFO,
        flags::USERINFO
    );
    assert_eq!(c.cvars.string("plain"), "2");
    assert_eq!(c.events, [Event::Unknown("nobody".into())]);
}

#[test]
fn registering_a_user_created_cvar_keeps_its_value_and_takes_the_code_default() {
    let mut c = Cvars::new(SINGLE_PLAYER_CHEATS);
    c.set("r_mode", Some("6"), false);
    let v = c.get("r_mode", "3", flags::ARCHIVE | flags::LATCH);
    assert_eq!(
        (v.string.as_str(), v.default.as_str(), v.flags),
        ("6", "3", flags::ARCHIVE | flags::LATCH)
    );
}

#[test]
fn rom_init_latch_and_cheat_rules() {
    let mut c = Cvars::new(SINGLE_PLAYER_CHEATS);
    c.get("version", "1.0", flags::ROM);
    c.get("fs_game", "", flags::INIT);
    c.get("r_picmip", "1", flags::LATCH);
    c.get("g_gravity", "800", flags::CHEAT);

    c.set("version", Some("2"), false);
    c.set("fs_game", Some("mymod"), false);
    c.set("r_picmip", Some("0"), false);
    c.set("g_gravity", Some("100"), false);
    assert_eq!(
        (
            c.string("version"),
            c.string("fs_game"),
            c.string("g_gravity")
        ),
        ("1.0", "", "800")
    );
    // A latched change waits for the next registration.
    assert_eq!(
        (
            c.string("r_picmip"),
            c.find("r_picmip").unwrap().latched.as_deref()
        ),
        ("1", Some("0"))
    );
    assert_eq!(c.get("r_picmip", "1", flags::LATCH).string, "0");

    // Code (force) may set anything.
    c.set("version", Some("2"), true);
    assert_eq!(c.string("version"), "2");

    // With cheats on, cheat cvars change; turning them off resets them.
    c.set("helpUsObi", Some("1"), false);
    c.set("g_gravity", Some("100"), false);
    assert_eq!(c.integer("g_gravity"), 100);
    c.reset_cheats();
    assert_eq!(c.integer("g_gravity"), 800);
}

#[test]
fn multiplayer_cheats_are_on_by_default_until_the_server_says_otherwise() {
    let mut c = Cvars::new(MULTIPLAYER_CHEATS);
    c.get("g_gravity", "800", flags::CHEAT);
    c.set("g_gravity", Some("100"), false);
    assert_eq!(c.integer("g_gravity"), 100);
}

#[test]
fn reset_toggle_and_bang() {
    let (_d, vfs) = vfs_with(&[]);
    let mut c = Console::new(SINGLE_PLAYER_CHEATS);
    c.cvars.get("cg_fov", "80", flags::ARCHIVE);
    c.cvars.get("cg_draw2d", "1", 0);
    c.cvars.get("color", "red", 0);
    run(
        &mut c,
        &vfs,
        "cg_fov 97\ntoggle cg_draw2d\ntoggle color red green blue\n",
    );
    assert_eq!(
        (
            c.cvars.string("cg_fov"),
            c.cvars.string("cg_draw2d"),
            c.cvars.string("color")
        ),
        ("97", "0", "green")
    );
    run(
        &mut c,
        &vfs,
        "reset cg_fov\ncg_draw2d !\ntoggle color red green blue\ntoggle color red green blue\n",
    );
    assert_eq!(
        (
            c.cvars.string("cg_fov"),
            c.cvars.string("cg_draw2d"),
            c.cvars.string("color")
        ),
        ("80", "1", "red")
    );
}

#[test]
fn exec_runs_the_file_before_the_rest_of_the_buffer() {
    let (_d, vfs) = vfs_with(&[
        ("my.cfg", "engine from_file\nset x 1"),
        ("other.txt", "engine txt"),
    ]);
    let mut c = Console::new(SINGLE_PLAYER_CHEATS);
    let seen = run(
        &mut c,
        &vfs,
        "exec my; engine after\nexecq other.txt\nexec missing\n",
    );
    assert_eq!(
        seen,
        [
            vec!["engine", "from_file"],
            vec!["engine", "after"],
            vec!["engine", "txt"]
        ]
    );
    assert_eq!(c.cvars.string("x"), "1");
    assert_eq!(
        c.events,
        [
            Event::Exec("my.cfg".into()),
            Event::CouldNotExec("missing.cfg".into())
        ]
    );
}

#[test]
fn archive_text_is_sorted_and_skips_unchanged_nodefault_cvars() {
    let mut c = Cvars::new(SINGLE_PLAYER_CHEATS);
    c.get("z_last", "1", flags::ARCHIVE);
    c.get("a_first", "x y", flags::ARCHIVE);
    c.get("nd_same", "5", flags::ARCHIVE_ND);
    c.get("nd_changed", "5", flags::ARCHIVE_ND);
    c.get("latched", "1", flags::ARCHIVE | flags::LATCH);
    c.get("not_archived", "1", 0);
    c.set("nd_changed", Some("6"), false);
    c.set("latched", Some("2"), false);
    assert_eq!(
        c.archive_text(),
        "seta a_first \"x y\"\nseta latched \"2\"\nseta nd_changed \"6\"\nseta z_last \"1\"\n"
    );
}

#[test]
fn command_line_startup() {
    let mut lines = split_command_line("+set fs_game \"my+mod\" +set r_mode 4 +safe +map mp/ffa3");
    assert_eq!(
        lines,
        [
            "",
            "set fs_game \"my+mod\" ",
            "set r_mode 4 ",
            "safe ",
            "map mp/ffa3"
        ]
    );
    assert!(take_safe_mode(&mut lines));
    let mut c = Console::new(MULTIPLAYER_CHEATS);
    c.startup_variables(&lines, None);
    assert_eq!(
        (c.cvars.string("fs_game"), c.cvars.string("r_mode")),
        ("my+mod", "4")
    );
    // The map command means the intro is skipped.
    assert!(c.add_startup_commands(&lines));
    assert!(
        !Console::new(MULTIPLAYER_CHEATS).add_startup_commands(&split_command_line("+set a 1"))
    );
}
