//! Console variables, with the original's flag rules (OpenJK `qcommon/cvar.cpp`).

use std::collections::HashMap;

/// Cvar flags (`q_shared.h`).
pub mod flags {
    /// Saved to the config file.
    pub const ARCHIVE: u32 = 0x1;
    pub const USERINFO: u32 = 0x2;
    pub const SERVERINFO: u32 = 0x4;
    pub const SYSTEMINFO: u32 = 0x8;
    /// Only settable from the command line.
    pub const INIT: u32 = 0x10;
    /// A change waits until the code registers the cvar again.
    pub const LATCH: u32 = 0x20;
    /// Read-only for the user.
    pub const ROM: u32 = 0x40;
    /// Created by a `set` before any code registered it.
    pub const USER_CREATED: u32 = 0x80;
    pub const TEMP: u32 = 0x100;
    /// Only changeable with cheats enabled.
    pub const CHEAT: u32 = 0x200;
    pub const NORESTART: u32 = 0x400;
    pub const INTERNAL: u32 = 0x800;
    pub const PROTECTED: u32 = 0x8000;
    /// Not written to the config when it equals its default.
    pub const NODEFAULT: u32 = 0x1_0000;
    pub const ARCHIVE_ND: u32 = ARCHIVE | NODEFAULT;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cvar {
    pub name: String,
    pub string: String,
    /// The value the code registered it with (`resetString`).
    pub default: String,
    /// A change waiting for the next registration of a `LATCH` cvar.
    pub latched: Option<String>,
    pub flags: u32,
    pub modified: bool,
    pub modification_count: u32,
    pub value: f32,
    pub integer: i32,
}

impl Cvar {
    fn new(name: &str, value: &str, flags: u32) -> Self {
        Self {
            name: name.to_owned(),
            string: value.to_owned(),
            default: value.to_owned(),
            latched: None,
            flags,
            modified: true,
            modification_count: 1,
            value: atof(value),
            integer: atoi(value),
        }
    }

    fn assign(&mut self, value: &str) {
        self.string = value.to_owned();
        self.value = atof(value);
        self.integer = atoi(value);
        self.modified = true;
        self.modification_count += 1;
    }
}

/// Which cvar unlocks `CHEAT` cvars: `helpUsObi` (single-player, default 0) or
/// `sv_cheats` (multiplayer, default 1, read-only, the server overrides it).
#[derive(Debug, Clone, Copy)]
pub struct CheatCvar {
    pub name: &'static str,
    pub default: &'static str,
    pub flags: u32,
}

pub const SINGLE_PLAYER_CHEATS: CheatCvar = CheatCvar {
    name: "helpUsObi",
    default: "0",
    flags: flags::SYSTEMINFO,
};

pub const MULTIPLAYER_CHEATS: CheatCvar = CheatCvar {
    name: "sv_cheats",
    default: "1",
    flags: flags::ROM | flags::SYSTEMINFO,
};

#[derive(Debug)]
pub struct Cvars {
    vars: HashMap<String, Cvar>,
    cheats: String,
}

impl Cvars {
    pub fn new(cheats: CheatCvar) -> Self {
        let mut c = Self {
            vars: HashMap::new(),
            cheats: cheats.name.to_ascii_lowercase(),
        };
        c.get(cheats.name, cheats.default, cheats.flags);
        c
    }

    pub fn find(&self, name: &str) -> Option<&Cvar> {
        self.vars.get(&name.to_ascii_lowercase())
    }

    /// The cvar's value, or "" if it does not exist (`Cvar_VariableString`).
    pub fn string(&self, name: &str) -> &str {
        self.find(name).map_or("", |c| &c.string)
    }

    pub fn integer(&self, name: &str) -> i32 {
        self.find(name).map_or(0, |c| c.integer)
    }

    pub fn value(&self, name: &str) -> f32 {
        self.find(name).map_or(0.0, |c| c.value)
    }

    /// All cvars, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &Cvar> {
        self.vars.values()
    }

    /// `Cvar_Get`: registers a cvar for code, or adopts one the user created with
    /// `set` (its default becomes the code's). Applies a latched change.
    pub fn get(&mut self, name: &str, default: &str, new_flags: u32) -> &Cvar {
        let name = valid_name(name);
        let key = name.to_ascii_lowercase();
        let Some(var) = self.vars.get_mut(&key) else {
            self.vars
                .insert(key.clone(), Cvar::new(name, default, new_flags));
            return &self.vars[&key];
        };
        if var.flags & flags::USER_CREATED != 0 {
            var.flags &= !flags::USER_CREATED;
            var.default = default.to_owned();
            if new_flags & flags::ROM != 0 {
                // Code-only now: the code's default replaces what the user set.
                var.latched = Some(default.to_owned());
            }
        }
        var.flags |= new_flags;
        if var.default.is_empty() {
            var.default = default.to_owned();
        }
        if let Some(latched) = var.latched.take() {
            let name = var.name.clone();
            self.set(&name, Some(&latched), true);
        }
        &self.vars[&key]
    }

    /// `Cvar_Set2`. `value` `None` means the default. Without `force` (that is, from
    /// the console or a config) `ROM` and `INIT` cvars are left alone, `LATCH` ones
    /// store the change for later, and `CHEAT` ones need cheats on. Setting a cvar
    /// that does not exist creates it, flagged `USER_CREATED` unless forced.
    pub fn set(&mut self, name: &str, value: Option<&str>, force: bool) -> Option<&Cvar> {
        let name = valid_name(name);
        let key = name.to_ascii_lowercase();
        let cheats_on = self.vars.get(&self.cheats).is_some_and(|c| c.integer != 0);
        if !self.vars.contains_key(&key) {
            let value = value?;
            let f = if force { 0 } else { flags::USER_CREATED };
            return Some(self.get(name, value, f));
        }
        let var = self.vars.get_mut(&key).unwrap();
        let value = value.unwrap_or(&var.default).to_owned();

        if var.flags & flags::LATCH != 0 && var.latched.is_some() {
            if value == var.string {
                var.latched = None;
                return Some(var);
            }
            if Some(&value) == var.latched.as_ref() {
                return Some(var);
            }
        } else if value == var.string {
            return Some(var);
        }

        if !force {
            if var.flags & (flags::ROM | flags::INIT) != 0 {
                return Some(var);
            }
            if var.flags & flags::LATCH != 0 {
                var.latched = Some(value);
                var.modified = true;
                var.modification_count += 1;
                return Some(var);
            }
            if var.flags & flags::CHEAT != 0 && !cheats_on {
                return Some(var);
            }
        } else {
            var.latched = None;
        }
        if value != var.string {
            var.assign(&value);
        }
        Some(var)
    }

    /// `Cvar_SetCheatState`: puts every `CHEAT` cvar back to its default.
    pub fn reset_cheats(&mut self) {
        let names: Vec<String> = self
            .vars
            .values()
            .filter(|v| v.flags & flags::CHEAT != 0)
            .map(|v| v.name.clone())
            .collect();
        for name in names {
            let var = self.vars.get_mut(&name.to_ascii_lowercase()).unwrap();
            var.latched = None;
            if var.string != var.default {
                let default = var.default.clone();
                self.set(&name, Some(&default), true);
            }
        }
    }

    pub fn add_flags(&mut self, name: &str, f: u32) {
        if let Some(var) = self.vars.get_mut(&name.to_ascii_lowercase()) {
            var.flags |= f;
        }
    }

    /// `Cvar_WriteVariables`: a `seta name "value"` line for every `ARCHIVE` cvar
    /// (the latched value if one is waiting), sorted by name, skipping `NODEFAULT`
    /// ones at their default.
    pub fn archive_text(&self) -> String {
        let mut vars: Vec<&Cvar> = self
            .vars
            .values()
            .filter(|v| v.flags & flags::ARCHIVE != 0 && !v.name.eq_ignore_ascii_case("cl_cdkey"))
            .collect();
        vars.sort_by(|a, b| a.name.cmp(&b.name));
        let mut out = String::new();
        for v in vars {
            let value = v.latched.as_deref().unwrap_or(&v.string);
            if v.flags & flags::NODEFAULT != 0 && value == v.default {
                continue;
            }
            if v.name.len() + value.len() + 10 > 1024 {
                continue;
            }
            out += &format!("seta {} \"{}\"\n", v.name, value);
        }
        out
    }
}

/// Names containing `\`, `"` or `;` become "BADNAME" (`Cvar_ValidateString`).
fn valid_name(name: &str) -> &str {
    if name.contains(['\\', '"', ';']) {
        "BADNAME"
    } else {
        name
    }
}

/// C's `atof` on a cvar string: the longest numeric prefix, 0 if none.
pub fn atof(s: &str) -> f32 {
    let s = s.trim_start();
    let b = s.as_bytes();
    let sign = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let digits = |from: usize| from + b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int_end = digits(sign);
    let mut end = if b.get(int_end) == Some(&b'.') {
        digits(int_end + 1)
    } else {
        int_end
    };
    if end == sign || (end == sign + 1 && b.get(sign) == Some(&b'.')) {
        return 0.0;
    }
    if matches!(b.get(end), Some(b'e' | b'E')) {
        let e = end + 1 + usize::from(matches!(b.get(end + 1), Some(b'+' | b'-')));
        if digits(e) > e {
            end = digits(e);
        }
    }
    s[..end].parse::<f64>().map_or(0.0, |v| v as f32)
}

/// C's `atoi`: the longest integer prefix, 0 if none.
pub fn atoi(s: &str) -> i32 {
    let s = s.trim_start();
    let b = s.as_bytes();
    let sign = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let end = sign + b[sign..].iter().take_while(|c| c.is_ascii_digit()).count();
    s[..end].parse::<i64>().map_or(0, |v| v as i32)
}
