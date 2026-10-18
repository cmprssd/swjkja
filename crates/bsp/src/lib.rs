//! Jedi Academy map files: `.bsp`, format "RBSP" version 1 (OpenJK `qfiles.h`).
//!
//! [`Bsp::parse`] copies each lump into a typed vector whose element layout matches
//! the file byte for byte, then checks every index one record holds into another
//! (surface to vertices, node to children, brush to sides, ...). Code that uses a
//! parsed map can index without further checks.

use bytemuck::{Pod, Zeroable};

#[cfg(not(target_endian = "little"))]
compile_error!("map records are read in place and assume a little-endian target");

const IDENT: &[u8; 4] = b"RBSP";
const VERSION: i32 = 1;
const LUMPS: usize = 18;
/// Lightmaps are 128x128 RGB pages packed one after another.
pub const LIGHTMAP_SIZE: usize = 128;
pub const LIGHTMAP_BYTES: usize = LIGHTMAP_SIZE * LIGHTMAP_SIZE * 3;
/// Light styles per surface and per vertex (`MAXLIGHTMAPS`).
pub const MAX_LIGHTMAPS: usize = 4;

#[repr(usize)]
#[derive(Debug, Clone, Copy)]
enum Lump {
    Entities,
    Shaders,
    Planes,
    Nodes,
    Leafs,
    LeafSurfaces,
    LeafBrushes,
    Models,
    Brushes,
    BrushSides,
    DrawVerts,
    DrawIndexes,
    Fogs,
    Surfaces,
    Lightmaps,
    LightGrid,
    Visibility,
    LightArray,
}

/// A shader (material) name with the surface and content flags the compiler stored.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Shader {
    name: [u8; 64],
    pub surface_flags: i32,
    pub content_flags: i32,
}

impl Shader {
    pub fn name(&self) -> &str {
        c_str(&self.name)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Plane {
    pub normal: [f32; 3],
    pub dist: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Node {
    pub plane: i32,
    /// A child `c >= 0` is a node; `c < 0` is leaf `-(c + 1)`.
    pub children: [i32; 2],
    pub mins: [i32; 3],
    pub maxs: [i32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Leaf {
    /// Visibility cluster; -1 for opaque leafs.
    pub cluster: i32,
    pub area: i32,
    pub mins: [i32; 3],
    pub maxs: [i32; 3],
    pub first_leaf_surface: i32,
    pub num_leaf_surfaces: i32,
    pub first_leaf_brush: i32,
    pub num_leaf_brushes: i32,
}

/// Model 0 is the world; the others are brush entities (doors, movers).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Model {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub first_surface: i32,
    pub num_surfaces: i32,
    pub first_brush: i32,
    pub num_brushes: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Brush {
    pub first_side: i32,
    pub num_sides: i32,
    pub shader: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BrushSide {
    pub plane: i32,
    pub shader: i32,
    pub draw_surface: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct DrawVert {
    pub xyz: [f32; 3],
    pub st: [f32; 2],
    pub lightmap: [[f32; 2]; MAX_LIGHTMAPS],
    pub normal: [f32; 3],
    pub color: [[u8; 4]; MAX_LIGHTMAPS],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Fog {
    shader: [u8; 64],
    pub brush: i32,
    /// The brush side ray tests clip against; -1 for none.
    pub visible_side: i32,
}

impl Fog {
    pub fn shader(&self) -> &str {
        c_str(&self.shader)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Bad,
    Planar,
    Patch,
    TriangleSoup,
    Flare,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Surface {
    pub shader: i32,
    /// Index into [`Bsp::fogs`], or -1.
    pub fog: i32,
    kind: i32,
    pub first_vert: i32,
    pub num_verts: i32,
    pub first_index: i32,
    pub num_indexes: i32,
    pub lightmap_styles: [u8; MAX_LIGHTMAPS],
    pub vertex_styles: [u8; MAX_LIGHTMAPS],
    /// Lightmap page per style, or a negative special value (-1 none, -2 white
    /// image, -3 lit by vertex colors).
    pub lightmap_num: [i32; MAX_LIGHTMAPS],
    pub lightmap_x: [i32; MAX_LIGHTMAPS],
    pub lightmap_y: [i32; MAX_LIGHTMAPS],
    pub lightmap_width: i32,
    pub lightmap_height: i32,
    pub lightmap_origin: [f32; 3],
    /// For patches, [0] and [1] are the LOD bounds.
    pub lightmap_vecs: [[f32; 3]; 3],
    pub patch_width: i32,
    pub patch_height: i32,
}

impl Surface {
    pub fn kind(&self) -> SurfaceKind {
        match self.kind {
            1 => SurfaceKind::Planar,
            2 => SurfaceKind::Patch,
            3 => SurfaceKind::TriangleSoup,
            4 => SurfaceKind::Flare,
            _ => SurfaceKind::Bad,
        }
    }
}

/// One cell of the light grid that lights models.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GridPoint {
    pub ambient: [[u8; 3]; MAX_LIGHTMAPS],
    pub direct: [[u8; 3]; MAX_LIGHTMAPS],
    pub styles: [u8; MAX_LIGHTMAPS],
    pub lat_long: [u8; 2],
}

/// Potentially visible sets: one bit row per cluster.
#[derive(Debug, Clone, Default)]
pub struct Visibility {
    pub clusters: usize,
    pub bytes_per_cluster: usize,
    pub bits: Vec<u8>,
}

impl Visibility {
    /// Whether cluster `to` may be visible from cluster `from`. Maps without
    /// visibility data, and the opaque cluster -1, see everything.
    pub fn can_see(&self, from: i32, to: i32) -> bool {
        let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
            return true;
        };
        if self.clusters == 0 || from >= self.clusters || to >= self.clusters {
            return true;
        }
        self.bits[from * self.bytes_per_cluster + to / 8] & (1 << (to % 8)) != 0
    }

    /// Multiplayer's `R_inPVS` (codemp `tr_world.cpp`) for two points' clusters: the
    /// bit of `to` in `from`'s row (`CM_ClusterPVS`, row 0 for a cluster outside the
    /// map). It does not check `to` for -1, so a point in solid reads bit 7 of the
    /// byte before the row: the last byte of the previous row, which holds only
    /// padding past the last cluster (clear) unless the row is full. Before row 0
    /// lies other memory; taken as clear.
    pub fn r_in_pvs(&self, from: i32, to: i32) -> bool {
        if self.clusters == 0 {
            return true;
        }
        let row = usize::try_from(from)
            .ok()
            .filter(|&c| c < self.clusters)
            .unwrap_or(0)
            * self.bytes_per_cluster;
        let (byte, bit) = match usize::try_from(to) {
            Ok(to) => (row + to / 8, to % 8),
            Err(_) if row == 0 => return false,
            Err(_) => (row - 1, 7),
        };
        self.bits.get(byte).is_some_and(|b| b & (1 << bit) != 0)
    }
}

/// One `{ "key" "value" ... }` block of the entity lump.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entity {
    pub pairs: Vec<(String, String)>,
}

impl Entity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Bsp {
    pub entities: Vec<Entity>,
    pub shaders: Vec<Shader>,
    pub planes: Vec<Plane>,
    pub nodes: Vec<Node>,
    pub leafs: Vec<Leaf>,
    pub leaf_surfaces: Vec<i32>,
    pub leaf_brushes: Vec<i32>,
    pub models: Vec<Model>,
    pub brushes: Vec<Brush>,
    pub brush_sides: Vec<BrushSide>,
    pub verts: Vec<DrawVert>,
    pub indexes: Vec<i32>,
    pub fogs: Vec<Fog>,
    pub surfaces: Vec<Surface>,
    /// [`LIGHTMAP_BYTES`] per page.
    pub lightmaps: Vec<u8>,
    pub light_grid: Vec<GridPoint>,
    pub visibility: Visibility,
    /// Indexes into [`Bsp::light_grid`], one per grid cell (cells share points).
    pub light_array: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

macro_rules! bail {
    ($($t:tt)*) => { return Err(Error(format!($($t)*))) };
}

impl Bsp {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        if data.len() < 8 + LUMPS * 8 {
            bail!("too short for a map header ({} bytes)", data.len());
        }
        if &data[0..4] != IDENT {
            bail!(
                "not an RBSP map (ident {:?})",
                String::from_utf8_lossy(&data[0..4])
            );
        }
        let version = i32::from_le_bytes(data[4..8].try_into().unwrap());
        if version != VERSION {
            bail!("RBSP version {version}, expected {VERSION}");
        }
        let lump = |l: Lump| -> Result<&[u8], Error> {
            let at = 8 + l as usize * 8;
            let ofs = i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
            let len = i32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap());
            usize::try_from(ofs)
                .ok()
                .zip(usize::try_from(len).ok())
                .and_then(|(o, n)| data.get(o..o.checked_add(n)?))
                .ok_or_else(|| Error(format!("{l:?} lump ({ofs}, {len}) is outside the file")))
        };
        fn records<T: Pod>(l: Lump, bytes: &[u8]) -> Result<Vec<T>, Error> {
            let size = size_of::<T>();
            if !bytes.len().is_multiple_of(size) {
                bail!(
                    "{l:?} lump is {} bytes, not a multiple of {size}",
                    bytes.len()
                );
            }
            Ok(bytemuck::pod_collect_to_vec(bytes))
        }
        macro_rules! read {
            ($l:ident) => {
                records(Lump::$l, lump(Lump::$l)?)?
            };
        }

        let lightmaps = lump(Lump::Lightmaps)?.to_vec();
        if !lightmaps.len().is_multiple_of(LIGHTMAP_BYTES) {
            bail!(
                "lightmap lump is {} bytes, not whole 128x128 pages",
                lightmaps.len()
            );
        }
        let bsp = Self {
            entities: parse_entities(lump(Lump::Entities)?)?,
            shaders: read!(Shaders),
            planes: read!(Planes),
            nodes: read!(Nodes),
            leafs: read!(Leafs),
            leaf_surfaces: read!(LeafSurfaces),
            leaf_brushes: read!(LeafBrushes),
            models: read!(Models),
            brushes: read!(Brushes),
            brush_sides: read!(BrushSides),
            verts: read!(DrawVerts),
            indexes: read!(DrawIndexes),
            fogs: read!(Fogs),
            surfaces: read!(Surfaces),
            lightmaps,
            light_grid: read!(LightGrid),
            visibility: parse_visibility(lump(Lump::Visibility)?)?,
            light_array: read!(LightArray),
        };
        bsp.validate()?;
        Ok(bsp)
    }

    pub fn lightmap_pages(&self) -> usize {
        self.lightmaps.len() / LIGHTMAP_BYTES
    }

    pub fn worldspawn(&self) -> Option<&Entity> {
        self.entities
            .iter()
            .find(|e| e.get("classname") == Some("worldspawn"))
    }

    /// Checks every cross-reference, so later code can index without checking.
    /// Messages are only formatted on failure: this runs over every index of a map.
    fn validate(&self) -> Result<(), Error> {
        fn index(i: i32, len: usize) -> Result<(), String> {
            match usize::try_from(i) {
                Ok(i) if i < len => Ok(()),
                _ => Err(format!("{i} out of range (0..{len})")),
            }
        }
        fn range(first: i32, count: i32, len: usize) -> Result<(), String> {
            match (usize::try_from(first), usize::try_from(count)) {
                (Ok(f), Ok(c)) if f + c <= len => Ok(()),
                _ => Err(format!("{first}+{count} out of range (0..{len})")),
            }
        }
        /// Prefixes a failed check with what was being checked.
        fn at<T>(r: Result<(), String>, what: impl FnOnce() -> T) -> Result<(), Error>
        where
            T: std::fmt::Display,
        {
            r.map_err(|e| Error(format!("{} {e}", what())))
        }

        for (i, s) in self.surfaces.iter().enumerate() {
            at(index(s.shader, self.shaders.len()), || {
                format!("surface {i}: shader")
            })?;
            if s.fog != -1 {
                at(index(s.fog, self.fogs.len()), || {
                    format!("surface {i}: fog")
                })?;
            }
            at(range(s.first_vert, s.num_verts, self.verts.len()), || {
                format!("surface {i}: vertices")
            })?;
            at(
                range(s.first_index, s.num_indexes, self.indexes.len()),
                || format!("surface {i}: indexes"),
            )?;
            let first = s.first_index as usize;
            let verts = s.num_verts as usize;
            let indexes = &self.indexes[first..first + s.num_indexes as usize];
            if let Some(&v) = indexes.iter().find(|&&v| v < 0 || v as usize >= verts) {
                at(index(v, verts), || format!("surface {i}: vertex index"))?;
            }
            if s.kind() == SurfaceKind::Patch
                && (s.patch_width < 0
                    || s.patch_height < 0
                    || i64::from(s.patch_width) * i64::from(s.patch_height)
                        > i64::from(s.num_verts))
            {
                bail!(
                    "surface {i}: patch {}x{} exceeds its {} vertices",
                    s.patch_width,
                    s.patch_height,
                    s.num_verts
                );
            }
        }
        if self.models.is_empty() {
            bail!("no world model");
        }
        for (i, m) in self.models.iter().enumerate() {
            at(
                range(m.first_surface, m.num_surfaces, self.surfaces.len()),
                || format!("model {i}: surfaces"),
            )?;
            at(
                range(m.first_brush, m.num_brushes, self.brushes.len()),
                || format!("model {i}: brushes"),
            )?;
        }
        for (i, node) in self.nodes.iter().enumerate() {
            at(index(node.plane, self.planes.len()), || {
                format!("node {i}: plane")
            })?;
            for c in node.children {
                if c >= 0 {
                    at(index(c, self.nodes.len()), || {
                        format!("node {i}: child node")
                    })?;
                } else {
                    at(index(-(c + 1), self.leafs.len()), || {
                        format!("node {i}: child leaf")
                    })?;
                }
            }
        }
        for (i, l) in self.leafs.iter().enumerate() {
            at(
                range(
                    l.first_leaf_surface,
                    l.num_leaf_surfaces,
                    self.leaf_surfaces.len(),
                ),
                || format!("leaf {i}: surfaces"),
            )?;
            at(
                range(
                    l.first_leaf_brush,
                    l.num_leaf_brushes,
                    self.leaf_brushes.len(),
                ),
                || format!("leaf {i}: brushes"),
            )?;
        }
        for &s in &self.leaf_surfaces {
            at(index(s, self.surfaces.len()), || "leaf surface")?;
        }
        for &b in &self.leaf_brushes {
            at(index(b, self.brushes.len()), || "leaf brush")?;
        }
        for (i, b) in self.brushes.iter().enumerate() {
            at(
                range(b.first_side, b.num_sides, self.brush_sides.len()),
                || format!("brush {i}: sides"),
            )?;
            at(index(b.shader, self.shaders.len()), || {
                format!("brush {i}: shader")
            })?;
        }
        for (i, s) in self.brush_sides.iter().enumerate() {
            at(index(s.plane, self.planes.len()), || {
                format!("brush side {i}: plane")
            })?;
            at(index(s.shader, self.shaders.len()), || {
                format!("brush side {i}: shader")
            })?;
        }
        for (i, f) in self.fogs.iter().enumerate() {
            // The global fog (worldspawn "fog") is stored with brush -1.
            if f.brush != -1 {
                at(index(f.brush, self.brushes.len()), || {
                    format!("fog {i}: brush")
                })?;
            }
        }
        for &g in &self.light_array {
            at(
                index(i32::from(g), self.light_grid.len()),
                || "light array entry",
            )?;
        }
        Ok(())
    }
}

fn parse_visibility(bytes: &[u8]) -> Result<Visibility, Error> {
    if bytes.is_empty() {
        return Ok(Visibility::default());
    }
    if bytes.len() < 8 {
        bail!(
            "visibility lump is {} bytes, too short for its header",
            bytes.len()
        );
    }
    let clusters = i32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let row = i32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let (Ok(clusters), Ok(row)) = (usize::try_from(clusters), usize::try_from(row)) else {
        bail!("visibility header ({clusters}, {row}) is negative");
    };
    if row < clusters.div_ceil(8) || bytes.len() - 8 < clusters * row {
        bail!("visibility lump too small for {clusters} clusters of {row} bytes");
    }
    Ok(Visibility {
        clusters,
        bytes_per_cluster: row,
        bits: bytes[8..8 + clusters * row].to_vec(),
    })
}

/// Parses the entity lump: `{ "key" "value" ... }` blocks, with `//` comments
/// allowed between tokens, as the original's COM_Parse reads it.
pub fn parse_entities(bytes: &[u8]) -> Result<Vec<Entity>, Error> {
    let text = String::from_utf8_lossy(bytes);
    let mut tokens = Tokens(text.trim_end_matches('\0'));
    let mut entities = Vec::new();
    while let Some(tok) = tokens.next() {
        if tok != "{" {
            bail!("entity lump: expected '{{', found {tok:?}");
        }
        let mut entity = Entity::default();
        loop {
            match tokens.next() {
                Some("}") => break,
                Some(key) => {
                    // Like the original's G_ParseSpawnVars, a "}" where a value
                    // belongs is an error, not a value.
                    let Some(value) = tokens.next().filter(|v| *v != "}") else {
                        bail!("entity lump: key {key:?} has no value");
                    };
                    entity.pairs.push((key.to_owned(), value.to_owned()));
                }
                None => bail!("entity lump: unterminated entity"),
            }
        }
        entities.push(entity);
    }
    Ok(entities)
}

struct Tokens<'a>(&'a str);

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        loop {
            self.0 = self.0.trim_start();
            if let Some(rest) = self.0.strip_prefix("//") {
                self.0 = rest.find('\n').map_or("", |i| &rest[i..]);
            } else {
                break;
            }
        }
        if self.0.is_empty() {
            return None;
        }
        if let Some(rest) = self.0.strip_prefix('"') {
            let end = rest.find('"').unwrap_or(rest.len());
            self.0 = rest.get(end + 1..).unwrap_or("");
            return Some(&rest[..end]);
        }
        let end = self.0.find(char::is_whitespace).unwrap_or(self.0.len());
        let (tok, rest) = self.0.split_at(end);
        self.0 = rest;
        Some(tok)
    }
}

fn c_str(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or("")
}

#[cfg(test)]
mod layout {
    use super::*;

    /// Record sizes from qfiles.h; a mismatch would misread every lump.
    #[test]
    fn record_sizes_match_the_file_format() {
        assert_eq!(size_of::<Shader>(), 72);
        assert_eq!(size_of::<Plane>(), 16);
        assert_eq!(size_of::<Node>(), 36);
        assert_eq!(size_of::<Leaf>(), 48);
        assert_eq!(size_of::<Model>(), 40);
        assert_eq!(size_of::<Brush>(), 12);
        assert_eq!(size_of::<BrushSide>(), 12);
        assert_eq!(size_of::<DrawVert>(), 80);
        assert_eq!(size_of::<Fog>(), 72);
        assert_eq!(size_of::<Surface>(), 148);
        assert_eq!(size_of::<GridPoint>(), 30);
    }
}
