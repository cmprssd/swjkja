//! MD3 meshes, format "IDP3" version 15 (OpenJK `qfiles.h`), read the way
//! `rd-vanilla/tr_model.cpp` reads them (`R_LoadMD3`), and the levels of detail
//! `RE_RegisterModel` loads with them.
//!
//! [`Md3::parse`] checks every offset and index in the file, so code that uses a
//! parsed mesh can index without further checks.

use swjkja_fs::Vfs;

pub type Vec3 = [f32; 3];

const IDENT: &[u8; 4] = b"IDP3";
pub const VERSION: i32 = 15;
/// Levels of detail: `name.md3`, `name_1.md3` and `name_2.md3` (`MD3_MAX_LODS`).
pub const MAX_LODS: usize = 3;
/// Vertex positions are stored as 16-bit integers in 1/64 units (`MD3_XYZ_SCALE`).
pub const XYZ_SCALE: f32 = 1.0 / 64.0;
/// A surface needs fewer vertices than this (`SHADER_MAX_VERTEXES`)...
pub const MAX_VERTEXES: usize = 1000;
/// ...and fewer indexes (`SHADER_MAX_INDEXES`), or the original stops with an error.
pub const MAX_INDEXES: usize = 6 * MAX_VERTEXES;

const HEADER_BYTES: usize = 108;
const FRAME_BYTES: usize = 56;
const TAG_BYTES: usize = 112;
const SURFACE_BYTES: usize = 108;
const SHADER_BYTES: usize = 68;

pub use crate::Error;
use crate::{Reader, bail};

/// One animation frame's bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub bounds: [Vec3; 2],
    pub local_origin: Vec3,
    pub radius: f32,
    pub name: String,
}

/// An attachment point, per frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub name: String,
    pub origin: Vec3,
    pub axis: [Vec3; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// Lowercased, with a trailing `_1`/`_2` (any `_x`) cut off, as skins name it.
    pub name: String,
    pub flags: i32,
    /// The shader names (a skin number picks one); usually exactly one.
    pub shaders: Vec<String>,
    /// Vertex indexes, three per triangle, each below [`Surface::vertex_count`].
    pub indexes: Vec<u32>,
    /// Texture coordinates, one per vertex.
    pub st: Vec<[f32; 2]>,
    /// Positions, `vertex_count` per frame, frame after frame.
    pub xyz: Vec<Vec3>,
    /// Normals, decoded like `LerpMeshVertexes`, laid out like `xyz`.
    pub normals: Vec<Vec3>,
}

impl Surface {
    pub fn vertex_count(&self) -> usize {
        self.st.len()
    }

    /// The positions and normals of one frame.
    pub fn frame(&self, frame: usize) -> (&[Vec3], &[Vec3]) {
        let n = self.vertex_count();
        let range = frame * n..(frame + 1) * n;
        (&self.xyz[range.clone()], &self.normals[range])
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Md3 {
    pub name: String,
    pub flags: i32,
    /// At least one.
    pub frames: Vec<Frame>,
    /// `tags_per_frame` tags per frame, frame after frame.
    pub tags: Vec<Tag>,
    pub tags_per_frame: usize,
    pub surfaces: Vec<Surface>,
}

/// `tr.sinTable`: `FUNCTABLE_SIZE` (1024) samples of one sine period.
fn sin_table(i: usize) -> f32 {
    (i as f64 * std::f64::consts::TAU / 1024.0).sin() as f32
}

/// `LerpMeshVertexes`: a normal packed as latitude (high byte) and longitude (low
/// byte), each 256 steps per turn, looked up in the 1024-entry sine table.
pub fn decode_normal(packed: i16) -> Vec3 {
    let lat = ((packed as u16 >> 8) & 0xff) as usize * 4;
    let lng = (packed as u16 & 0xff) as usize * 4;
    [
        sin_table((lat + 256) & 1023) * sin_table(lng),
        sin_table(lat) * sin_table(lng),
        sin_table((lng + 256) & 1023),
    ]
}

impl Md3 {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let r = Reader { data, what: "md3" };
        if r.bytes(0, 4)? != IDENT {
            bail!(
                "not an MD3 (ident {:?})",
                String::from_utf8_lossy(r.bytes(0, 4)?)
            );
        }
        let version = r.i32(4)?;
        if version != VERSION {
            bail!("MD3 version {version}, expected {VERSION}");
        }
        if data.len() < HEADER_BYTES {
            bail!("too short for an MD3 header ({} bytes)", data.len());
        }
        let name = r.name(8, 64)?;
        let flags = r.i32(72)?;
        let frame_count = r.count(76, "numFrames")?;
        let tags_per_frame = r.count(80, "numTags")?;
        let surface_count = r.count(84, "numSurfaces")?;
        let ofs_frames = r.count(92, "ofsFrames")?;
        let ofs_tags = r.count(96, "ofsTags")?;
        let ofs_surfaces = r.count(100, "ofsSurfaces")?;
        if frame_count < 1 {
            bail!("no frames");
        }

        let mut frames = Vec::with_capacity(frame_count);
        for f in 0..frame_count {
            let at = ofs_frames + f * FRAME_BYTES;
            frames.push(Frame {
                bounds: [r.vec3(at)?, r.vec3(at + 12)?],
                local_origin: r.vec3(at + 24)?,
                radius: r.f32(at + 36)?,
                name: r.name(at + 40, 16)?,
            });
        }
        let tag_count = tags_per_frame
            .checked_mul(frame_count)
            .ok_or_else(|| Error("too many tags".into()))?;
        r.bytes(ofs_tags, tag_count.saturating_mul(TAG_BYTES))?;
        let mut tags = Vec::with_capacity(tag_count);
        for t in 0..tag_count {
            let at = ofs_tags + t * TAG_BYTES;
            tags.push(Tag {
                name: r.name(at, 64)?,
                origin: r.vec3(at + 64)?,
                axis: [r.vec3(at + 76)?, r.vec3(at + 88)?, r.vec3(at + 100)?],
            });
        }

        let mut surfaces = Vec::with_capacity(surface_count.min(64));
        let mut at = ofs_surfaces;
        for s in 0..surface_count {
            let surface_data = data.get(at..).unwrap_or(&[]);
            let what = format!("surface {s}");
            let surface = parse_surface(
                &Reader {
                    data: surface_data,
                    what: &what,
                },
                frame_count,
            )?;
            surfaces.push(surface.0);
            at += surface.1;
        }
        Ok(Self {
            name,
            flags,
            frames,
            tags,
            tags_per_frame,
            surfaces,
        })
    }
}

/// One surface, and where the next one starts relative to it (`ofsEnd`).
fn parse_surface(r: &Reader, model_frames: usize) -> Result<(Surface, usize), Error> {
    if r.bytes(0, SURFACE_BYTES).is_err() {
        bail!("{}: past the end of the file", r.what);
    }
    let mut name = r.name(4, 64)?.to_ascii_lowercase();
    // R_LoadMD3: "strip off a trailing _1 or _2" (any character after the '_').
    let bytes = name.as_bytes();
    if bytes.len() > 2 && bytes[bytes.len() - 2] == b'_' {
        name.truncate(bytes.len() - 2);
    }
    let flags = r.i32(68)?;
    let frame_count = r.count(72, "numFrames")?;
    let shader_count = r.count(76, "numShaders")?;
    let vertex_count = r.count(80, "numVerts")?;
    let triangle_count = r.count(84, "numTriangles")?;
    let ofs_triangles = r.count(88, "ofsTriangles")?;
    let ofs_shaders = r.count(92, "ofsShaders")?;
    let ofs_st = r.count(96, "ofsSt")?;
    let ofs_xyz = r.count(100, "ofsXyzNormals")?;
    let ofs_end = r.count(104, "ofsEnd")?;
    if vertex_count >= MAX_VERTEXES {
        bail!(
            "{name}: {vertex_count} vertices, the limit is {}",
            MAX_VERTEXES - 1
        );
    }
    if triangle_count * 3 >= MAX_INDEXES {
        bail!(
            "{name}: {triangle_count} triangles, the limit is {}",
            MAX_INDEXES / 3 - 1
        );
    }
    // The renderer draws a surface's frames by the model's frame number.
    if frame_count < model_frames {
        bail!("{name}: {frame_count} frames, the model has {model_frames}");
    }
    if ofs_end == 0 {
        bail!("{name}: ofsEnd is 0");
    }

    let shaders = (0..shader_count)
        .map(|i| r.name(ofs_shaders + i * SHADER_BYTES, 64))
        .collect::<Result<Vec<_>, _>>()?;
    r.bytes(ofs_triangles, triangle_count * 12)?;
    let mut indexes = Vec::with_capacity(triangle_count * 3);
    for i in 0..triangle_count * 3 {
        let v = r.i32(ofs_triangles + i * 4)?;
        match u32::try_from(v) {
            Ok(v) if (v as usize) < vertex_count => indexes.push(v),
            _ => bail!("{name}: vertex index {v} of {vertex_count}"),
        }
    }
    r.bytes(ofs_st, vertex_count * 8)?;
    let st = (0..vertex_count)
        .map(|i| Ok([r.f32(ofs_st + i * 8)?, r.f32(ofs_st + i * 8 + 4)?]))
        .collect::<Result<Vec<_>, Error>>()?;
    let packed = model_frames * vertex_count;
    r.bytes(ofs_xyz, packed * 8)?;
    let mut xyz = Vec::with_capacity(packed);
    let mut normals = Vec::with_capacity(packed);
    for i in 0..packed {
        let at = ofs_xyz + i * 8;
        xyz.push([
            f32::from(r.i16(at)?) * XYZ_SCALE,
            f32::from(r.i16(at + 2)?) * XYZ_SCALE,
            f32::from(r.i16(at + 4)?) * XYZ_SCALE,
        ]);
        normals.push(decode_normal(r.i16(at + 6)?));
    }
    Ok((
        Surface {
            name,
            flags,
            shaders,
            indexes,
            st,
            xyz,
            normals,
        },
        ofs_end,
    ))
}

/// A model's levels of detail, most detailed first, as `RE_RegisterModel` loads an
/// `.md3`: `name_2.md3`, `name_1.md3`, then `name.md3`, each only if the file exists.
#[derive(Debug, Clone)]
pub struct Md3Model {
    pub lods: Vec<Md3>,
}

/// The file of level of detail `lod`: `models/x.md3`, `models/x_1.md3`, ...
pub fn lod_file(name: &str, lod: usize) -> String {
    if lod == 0 {
        return name.to_owned();
    }
    let stem = name.rfind('.').map_or(name, |dot| &name[..dot]);
    format!("{stem}_{lod}.md3")
}

impl Md3Model {
    /// Loads `name` and its levels of detail. `Err` if `name` itself is missing or a
    /// level does not parse.
    pub fn load(vfs: &Vfs, name: &str) -> Result<Self, Error> {
        let mut lods: Vec<Md3> = Vec::new();
        for lod in (0..MAX_LODS).rev() {
            let file = lod_file(name, lod);
            let Ok(data) = vfs.read(&file) else {
                if lod == 0 {
                    bail!("{file}: not found");
                }
                continue;
            };
            // A level that does not load fails the model, or (past a level that did)
            // leaves the original with an empty level slot; both are refused here.
            let md3 = Md3::parse(&data).map_err(|e| Error(format!("{file}: {e}")))?;
            lods.insert(0, md3);
        }
        Ok(Self { lods })
    }
}
