//! Jedi Academy's virtual filesystem.
//!
//! Game data lives in game folders (`base`, or a mod folder) under one or more roots
//! (the install, the user's home). Each game folder holds loose files and `.pk3`
//! archives. A path such as `maps/mp/ffa3.bsp` resolves to the highest-priority copy,
//! in the same order as the original engine (`FS_Startup` and `FS_AddGameDirectory`
//! in OpenJK's `files.cpp`):
//!
//! - game folders added later win: the mod folder over `base`, and within one game
//!   folder, later roots over earlier ones;
//! - within a game folder, `.pk3` files win over loose files, and a `.pk3` sorting
//!   later by name wins (`assets3.pk3` over `assets0.pk3`), with `dl_*.pk3` last;
//! - within one `.pk3`, a later duplicate entry wins.
//!
//! Lookups ignore ASCII case and accept `\` as a separator, like the original.

mod pk3;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub use pk3::{Entry, Pk3};

/// Lower-cases ASCII, turns `\` into `/` and drops leading slashes: the form every
/// path is stored and looked up in.
pub fn normalize(path: &str) -> String {
    path.trim_start_matches(['/', '\\'])
        .chars()
        .map(|c| match c {
            '\\' => '/',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Corrupt {
        path: Option<PathBuf>,
        detail: String,
    },
    NotFound(String),
}

impl Error {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    fn corrupt(detail: impl Into<String>) -> Self {
        Self::Corrupt {
            path: None,
            detail: detail.into(),
        }
    }

    /// Attaches the file the error happened in, unless one is already attached.
    fn at(self, file: &Path) -> Self {
        match self {
            Self::Io { path, source } if path.as_os_str().is_empty() => Self::io(file, source),
            Self::Corrupt { path: None, detail } => Self::Corrupt {
                path: Some(file.to_path_buf()),
                detail,
            },
            other => other,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Self::Io {
            path: PathBuf::new(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Corrupt {
                path: Some(path),
                detail,
            } => write!(f, "{}: {detail}", path.display()),
            Self::Corrupt { path: None, detail } => f.write_str(detail),
            Self::NotFound(name) => write!(f, "{name}: not found"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Where the winning copy of a path lives.
#[derive(Debug, Clone)]
enum Location {
    Packed { pk3: usize, entry: usize },
    Loose(PathBuf),
}

/// The mounted game folders and an index of every file they provide.
/// One entry of the original's search path.
#[derive(Debug, Clone)]
enum SearchPath {
    /// A game folder's loose files.
    Dir(PathBuf),
    /// An index into [`Vfs::pk3s`].
    Pk3(usize),
}

#[derive(Debug, Default)]
pub struct Vfs {
    pk3s: Vec<Pk3>,
    index: HashMap<Box<str>, Location>,
    mounted: Vec<(PathBuf, String)>,
    /// Lowest priority first, in the order the original builds it.
    search: Vec<SearchPath>,
}

impl Vfs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mounts `games` (lowest priority first, e.g. `["base", "mymod"]`) under each of
    /// `roots` (lowest priority first, e.g. the install, then the user's home), the
    /// way the original engine starts up.
    pub fn mount_game(roots: &[PathBuf], games: &[&str]) -> Result<Self, Error> {
        let mut vfs = Self::new();
        for game in games {
            for root in roots {
                vfs.add_game_dir(root, game)?;
            }
        }
        Ok(vfs)
    }

    /// Adds `root/game` with a higher priority than everything added before. A missing
    /// folder adds nothing, and adding the same folder twice changes nothing.
    pub fn add_game_dir(&mut self, root: &Path, game: &str) -> Result<(), Error> {
        let dir = root.join(game);
        if !dir.is_dir() || self.mounted.iter().any(|(r, g)| r == root && g == game) {
            return Ok(());
        }
        self.mounted.push((root.to_path_buf(), game.to_owned()));
        self.search.push(SearchPath::Dir(dir.clone()));

        let mut loose = Vec::new();
        walk(&dir, &dir, &mut loose)?;
        for (name, path) in loose {
            self.index.insert(name.into(), Location::Loose(path));
        }

        let mut archives: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| Error::io(&dir, e))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pk3")))
            .collect();
        archives.sort_by_key(|p| pk3_sort_key(p));
        for path in archives {
            let pk3 = Pk3::open(&path)?;
            let id = self.pk3s.len();
            for (entry, e) in pk3.entries().iter().enumerate() {
                self.index
                    .insert(e.name.clone(), Location::Packed { pk3: id, entry });
            }
            self.pk3s.push(pk3);
            self.search.push(SearchPath::Pk3(id));
        }
        Ok(())
    }

    /// Lists the files under `dir` ending in `extension`, in the order the original's
    /// `FS_ListFiles` returns them, which is not sorted: search paths from highest
    /// priority down, each archive in its own directory order, then a folder's loose
    /// files; a name already listed is skipped. Archive entries one subfolder deep are
    /// included (`sub/name.ext`), loose files only directly in `dir`. Names are
    /// relative to `dir`.
    ///
    /// Some callers depend on this order: the shader scripts, for one, let the file
    /// listed last win.
    pub fn list(&self, dir: &str, extension: &str) -> Vec<String> {
        let dir = normalize(dir).trim_end_matches('/').to_owned();
        let extension = extension.to_ascii_lowercase();
        let mut out: Vec<String> = Vec::new();
        let add = |name: String, out: &mut Vec<String>| {
            if !out.contains(&name) {
                out.push(name);
            }
        };
        for search in self.search.iter().rev() {
            match search {
                SearchPath::Pk3(id) => {
                    for e in self.pk3s[*id].entries() {
                        let Some(rest) =
                            e.name.strip_prefix(&dir).and_then(|r| r.strip_prefix('/'))
                        else {
                            continue;
                        };
                        if rest.matches('/').count() <= 1 && rest.ends_with(&extension) {
                            add(rest.to_owned(), &mut out);
                        }
                    }
                }
                SearchPath::Dir(root) => {
                    let Ok(entries) = std::fs::read_dir(root.join(&dir)) else {
                        continue;
                    };
                    let mut names: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().is_file())
                        .map(|e| e.file_name().to_string_lossy().to_ascii_lowercase())
                        .filter(|n| n.ends_with(&extension))
                        .collect();
                    names.sort();
                    for name in names {
                        add(name, &mut out);
                    }
                }
            }
        }
        out
    }

    pub fn contains(&self, path: &str) -> bool {
        self.index.contains_key(normalize(path).as_str())
    }

    /// Reads the highest-priority copy of `path`.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, Error> {
        let name = normalize(path);
        match self.index.get(name.as_str()) {
            Some(Location::Packed { pk3, entry }) => {
                let pk3 = &self.pk3s[*pk3];
                pk3.read(&pk3.entries()[*entry])
            }
            Some(Location::Loose(file)) => std::fs::read(file).map_err(|e| Error::io(file, e)),
            None => Err(Error::NotFound(name)),
        }
    }

    /// Every path the filesystem provides, normalized, in no particular order.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.index.keys().map(|k| &**k)
    }

    /// The archives in priority order, lowest first.
    pub fn pk3s(&self) -> &[Pk3] {
        &self.pk3s
    }
}

/// The original's `paksort`: case-insensitive by name, except that `dl_*` archives
/// (downloaded ones) sort after all others.
fn pk3_sort_key(path: &Path) -> (bool, String) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    (name.starts_with("dl_"), name)
}

/// Collects every loose file under `dir` as (normalized path relative to `base`, path).
fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let path = entry.map_err(|e| Error::io(dir, e))?.path();
        if path.is_dir() {
            walk(base, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push((normalize(&rel.to_string_lossy()), path));
        }
    }
    Ok(())
}
