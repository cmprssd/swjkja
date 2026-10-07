//! The console: the command buffer, cvars and config files, run the way the original
//! engine runs them (OpenJK `qcommon/cmd.cpp`, `cvar.cpp`, `common.cpp`).
//!
//! Text goes into a [`CommandBuffer`] (from config files, the command line, key
//! bindings); [`Console::execute`] splits it into commands and runs each one: a
//! built-in command, else a command the engine registered, else a cvar.

pub mod cmd;
pub mod cvar;

pub use cmd::{CommandBuffer, args_from, split_command_line, tokenize};
pub use cvar::{CheatCvar, Cvar, Cvars, MULTIPLAYER_CHEATS, SINGLE_PLAYER_CHEATS, flags};
use swjkja_fs::Vfs;

/// What the engine's own commands report back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    Yes,
    /// Not an engine command; the console then tries cvars.
    No,
}

/// Output the console produced (what the original prints).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A cvar's value, from `name` alone or `set name`.
    Print {
        name: String,
        value: String,
        default: String,
    },
    Exec(String),
    CouldNotExec(String),
    /// Neither a command nor a cvar.
    Unknown(String),
}

pub struct Console {
    pub cvars: Cvars,
    pub buffer: CommandBuffer,
    pub events: Vec<Event>,
}

impl Console {
    pub fn new(cheats: CheatCvar) -> Self {
        Self {
            cvars: Cvars::new(cheats),
            buffer: CommandBuffer::default(),
            events: Vec::new(),
        }
    }

    /// `Cbuf_Execute`: runs commands until the buffer is empty or a `wait` pauses it
    /// until the next frame. `engine` runs commands the engine registered.
    pub fn execute(&mut self, vfs: &Vfs, engine: &mut dyn FnMut(&mut Self, &[String]) -> Handled) {
        let mut state = cmd::SplitState::default();
        while let Some(line) = self.buffer.next(&mut state) {
            self.execute_line(&line, vfs, engine);
        }
    }

    /// `Cmd_ExecuteString` for one command line.
    pub fn execute_line(
        &mut self,
        line: &str,
        vfs: &Vfs,
        engine: &mut dyn FnMut(&mut Self, &[String]) -> Handled,
    ) {
        let args = tokenize(line);
        let Some(name) = args.first() else { return };
        if self.builtin(&args, vfs)
            || engine(self, &args) == Handled::Yes
            || self.cvar_command(&args)
        {
            return;
        }
        self.events.push(Event::Unknown(name.clone()));
    }

    fn builtin(&mut self, args: &[String], vfs: &Vfs) -> bool {
        let cmd = args[0].to_ascii_lowercase();
        match cmd.as_str() {
            "set" | "seta" | "sets" | "setu" => match args.len() {
                1 => {}
                2 => self.print(&args[1]),
                _ => {
                    let value = args_from(args, 2);
                    if self.cvars.set(&args[1], Some(&value), false).is_some() {
                        let f = match cmd.as_bytes()[3..].first() {
                            Some(b'a') => flags::ARCHIVE,
                            Some(b'u') => flags::USERINFO,
                            Some(b's') => flags::SERVERINFO,
                            _ => 0,
                        };
                        self.cvars.add_flags(&args[1], f);
                    }
                }
            },
            "reset" => {
                if args.len() == 2 {
                    self.cvars.set(&args[1], None, false);
                }
            }
            "toggle" => self.toggle(args),
            "exec" | "execq" => {
                if args.len() == 2 {
                    let mut file = args[1].clone();
                    if !file.rsplit('/').next().unwrap_or("").contains('.') {
                        file += ".cfg";
                    }
                    match vfs.read(&file) {
                        Ok(data) => {
                            let text = String::from_utf8_lossy(until_nul(&data)).into_owned();
                            self.buffer.insert(&text);
                            if cmd == "exec" {
                                self.events.push(Event::Exec(file));
                            }
                        }
                        Err(_) => self.events.push(Event::CouldNotExec(file)),
                    }
                }
            }
            "wait" => {
                self.buffer.wait = match args.get(1).map(|a| cvar::atoi(a)) {
                    Some(n) if n >= 0 => n as u32,
                    _ => 1,
                };
            }
            _ => return false,
        }
        true
    }

    /// `Cvar_Toggle_f`: `toggle name` flips 0/1; `toggle name a b c` steps through
    /// the list (to the value after the current one, else the first).
    fn toggle(&mut self, args: &[String]) {
        match args.len() {
            0..=1 | 3 => {}
            2 => {
                let v = if self.cvars.value(&args[1]) == 0.0 {
                    "1"
                } else {
                    "0"
                };
                self.cvars.set(&args[1], Some(v), false);
            }
            _ => {
                let current = self.cvars.string(&args[1]).to_owned();
                let next = args[2..args.len() - 1]
                    .iter()
                    .position(|a| *a == current)
                    .map_or(&args[2], |i| &args[i + 3])
                    .clone();
                self.cvars.set(&args[1], Some(&next), false);
            }
        }
    }

    /// `Cvar_Command`: `name` prints a cvar, `name !` flips it, `name value...` sets it.
    fn cvar_command(&mut self, args: &[String]) -> bool {
        let Some(var) = self.cvars.find(&args[0]) else {
            return false;
        };
        let name = var.name.clone();
        match args.len() {
            1 => self.print(&name),
            _ if args[1] == "!" => {
                let flipped = if var.value == 0.0 { "1" } else { "0" };
                self.cvars.set(&name, Some(flipped), false);
            }
            _ => {
                self.cvars.set(&name, Some(&args_from(args, 1)), false);
            }
        }
        true
    }

    fn print(&mut self, name: &str) {
        if let Some(v) = self.cvars.find(name) {
            self.events.push(Event::Print {
                name: v.name.clone(),
                value: v.string.clone(),
                default: v.default.clone(),
            });
        }
    }

    /// `Com_StartupVariable`: applies the command line's `set` commands (all of them,
    /// or only those for `only`) straight away, before configs run and again after,
    /// so the command line wins.
    pub fn startup_variables(&mut self, command_line: &[String], only: Option<&str>) {
        for line in command_line {
            let args = tokenize(line);
            if args.first().map(String::as_str) != Some("set") || args.len() < 2 {
                continue;
            }
            if only.is_none_or(|m| m == args[1]) {
                let value = args_from(&args, 2);
                if self.cvars.find(&args[1]).is_none() {
                    self.cvars.get(&args[1], &value, flags::USER_CREATED);
                } else {
                    self.cvars.set(&args[1], Some(&value), false);
                }
            }
        }
    }

    /// `Com_AddStartupCommands`: queues every command-line command. True if any is
    /// more than a `set` (the original then skips the intro movie).
    pub fn add_startup_commands(&mut self, command_line: &[String]) -> bool {
        let mut added = false;
        for line in command_line.iter().filter(|l| !l.is_empty()) {
            if !line.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("set")) {
                added = true;
            }
            self.buffer.add(line);
            self.buffer.add("\n");
        }
        added
    }
}

/// `Com_SafeMode`: whether the command line asks to skip the user's configs (`safe`
/// or `cvar_restart`); that entry is removed.
pub fn take_safe_mode(command_line: &mut [String]) -> bool {
    for line in command_line.iter_mut() {
        let first = tokenize(line).into_iter().next().unwrap_or_default();
        if first.eq_ignore_ascii_case("safe") || first.eq_ignore_ascii_case("cvar_restart") {
            line.clear();
            return true;
        }
    }
    false
}

fn until_nul(data: &[u8]) -> &[u8] {
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}
