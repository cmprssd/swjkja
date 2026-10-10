//! Ghoul2 skins (`.skin` files), read the way `rd-vanilla/tr_skin.cpp` reads them
//! (`RE_RegisterSkin`): which shader each named surface draws with, "*off" for a
//! surface the skin hides.

use swjkja_fs::Vfs;

/// The shader a skin gives a surface it hides (`G2API_SetSurfaceOnOffFromSkin`).
pub const OFF: &str = "*off";
/// `skin_t`'s surface limit (`MAX_SKIN_SURFACES`); the rest of a skin is dropped.
const MAX_SURFACES: usize = 128;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skin {
    /// Surface names, lowercased, and their shaders, in file order: the first entry
    /// for a name wins.
    pub surfaces: Vec<(String, String)>,
}

/// `CommaParse`: the next token, ending at whitespace or a comma (a quoted one at its
/// closing quote), after whitespace and comments; `None` at the end.
fn comma_token(text: &[u8], pos: &mut usize) -> Option<String> {
    loop {
        while text.get(*pos).is_some_and(|&c| c <= b' ') {
            *pos += 1;
        }
        let rest = &text[*pos..];
        if rest.starts_with(b"//") {
            while text.get(*pos).is_some_and(|&c| c != b'\n') {
                *pos += 1;
            }
        } else if rest.starts_with(b"/*") {
            match rest.windows(2).position(|w| w == b"*/") {
                Some(end) => *pos += end + 2,
                None => *pos = text.len(),
            }
        } else {
            break;
        }
    }
    let start = *pos;
    match text.get(start)? {
        b'"' => {
            let end = text[start + 1..]
                .iter()
                .position(|&c| c == b'"')
                .map_or(text.len(), |e| start + 1 + e);
            *pos = (end + 1).min(text.len());
            Some(String::from_utf8_lossy(&text[start + 1..end]).into_owned())
        }
        _ => {
            *pos += 1;
            while text.get(*pos).is_some_and(|&c| c > b' ' && c != b',') {
                *pos += 1;
            }
            Some(String::from_utf8_lossy(&text[start..*pos]).into_owned())
        }
    }
}

impl Skin {
    /// `RE_RegisterIndividualSkin`: adds `text`'s "surface,shader" pairs. Tags
    /// ("tag_...") are skipped; a name ending in "_off" loses it, unless its shader is
    /// "*off" (the surface is off already), when the pair is skipped.
    pub fn add(&mut self, text: &[u8]) {
        let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
        let mut pos = 0;
        while pos < text.len() {
            let Some(token) = comma_token(text, &mut pos).filter(|t| !t.is_empty()) else {
                break;
            };
            if text.get(pos) == Some(&b',') {
                pos += 1;
            }
            // Checked before lowercasing, as the original does.
            if token.starts_with("tag_") {
                continue;
            }
            let mut name = token.to_ascii_lowercase();
            let shader = comma_token(text, &mut pos).unwrap_or_default();
            if let Some(stem) = name.strip_suffix("_off") {
                if shader == OFF {
                    continue;
                }
                name = stem.to_owned();
            }
            if self.surfaces.len() >= MAX_SURFACES {
                break;
            }
            self.surfaces.push((name, shader));
        }
    }

    /// `RE_RegisterSkin`: the skin `name` names, or `None` for the model's own shaders
    /// (a missing file, or no surfaces). A name with '|' joins three files, a head,
    /// torso and lower body: "models/players/jedi_tf/|head_a1|torso_a1|lower_a1" reads
    /// `head_a1.skin`, `torso_a1.skin` and `lower_a1.skin` in that folder, a file
    /// named twice once; any of them missing leaves the model's own shaders.
    pub fn load(vfs: &Vfs, name: &str) -> Option<Self> {
        let mut files = vec![name.to_owned()];
        if let Some((dir, parts)) = name.split_once('|') {
            // RE_SplitSkins; with fewer than three parts the name is one file.
            if let [head, torso, lower] = parts.splitn(3, '|').collect::<Vec<_>>()[..] {
                let [head, torso, lower] = [head, torso, lower].map(|p| format!("{dir}{p}.skin"));
                files = vec![head.clone()];
                if torso != head {
                    files.push(torso.clone());
                }
                if lower != head && lower != torso {
                    files.push(lower);
                }
            }
        }
        let mut skin = Self::default();
        for file in files {
            skin.add(&vfs.read(&file).ok()?);
            if skin.surfaces.is_empty() {
                return None;
            }
        }
        Some(skin)
    }

    /// The shader the skin gives surface `name` (lowercase), if it names it.
    pub fn shader(&self, name: &str) -> Option<&str> {
        self.surfaces
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, s)| s.as_str())
    }
}
