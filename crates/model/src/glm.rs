//! Ghoul2 meshes, format "2LGM" version 6 (OpenJK `mdx_format.h`), read the way
//! `rd-vanilla/tr_ghoul2.cpp` reads them (`R_LoadMDXM`), the surfaces it draws
//! (`RenderSurfaces`) and their vertices skinned by a pose (`RB_SurfaceGhoul`).
//!
//! [`Glm::parse`] checks every offset and index in the file, so code that uses a
//! parsed mesh can index without further checks.

use swjkja_fs::Vfs;

use crate::gla::{self, Gla, Mat34};
use crate::md3::{MAX_INDEXES, MAX_VERTEXES, Vec3};
use crate::skin::{self, Skin};
use crate::{Error, Reader, bail};

const IDENT: &[u8; 4] = b"2LGM";
pub const VERSION: i32 = 6;

/// A tag, not drawn: a point other models and effects attach to.
pub const SURFACE_IS_BOLT: u32 = 0x1;
/// Hidden until something turns it on.
pub const SURFACE_OFF: u32 = 0x2;
/// Hides the surfaces below it too.
pub const SURFACE_NO_DESCENDANTS: u32 = 0x100;

const HEADER_BYTES: usize = 164;
/// `mdxmSurfHierarchy_t` up to its child indexes.
const HIERARCHY_BYTES: usize = 140;
const SURFACE_BYTES: usize = 40;
const VERTEX_BYTES: usize = 32;

/// A surface's place in the hierarchy, shared by every level of detail.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceInfo {
    /// Lowercased, a trailing "_off" cut off, as skins name it.
    pub name: String,
    pub flags: u32,
    /// "" for none (the default shader).
    pub shader: String,
    /// `None` for a root; a child's parent is the surface listing it.
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// One vertex in the model's space and the bones that move it.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub xyz: Vec3,
    pub normal: Vec3,
    /// 1 to 4 skeleton bone numbers and their weights; the last weight is what the
    /// others leave of 1.
    pub bones: Vec<(usize, f32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// Vertex indexes, three per triangle, each below the vertex count.
    pub indexes: Vec<u32>,
    pub verts: Vec<Vertex>,
    /// Texture coordinates, one per vertex.
    pub st: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glm {
    pub name: String,
    /// The skeleton's name, without ".gla" ([`gla::DEFAULT_NAME`] for one still bone).
    pub anim_name: String,
    /// The bones the skeleton must have.
    pub bone_count: usize,
    /// At least one; surface 0 is the root drawing starts from.
    pub surfaces: Vec<SurfaceInfo>,
    /// At least one level, most detailed first, each with one entry per surface.
    pub lods: Vec<Vec<Surface>>,
}

/// `G2_GetVertWeights`, `G2_GetVertBoneIndex` and `G2_GetVertBoneWeightNotSlow`: a
/// vertex's weights (count - 1 in bits 30-31), 5-bit indexes into the surface's bone
/// references, and 10-bit weights (a byte, its top two bits at bit 20 + 2k).
fn decode_weights(packed: u32, low: [u8; 4]) -> Vec<(usize, f32)> {
    let count = (packed >> 30) as usize + 1;
    let mut total = 0.0;
    (0..count)
        .map(|k| {
            let reference = (packed >> (5 * k)) & 31;
            let weight = if k == count - 1 {
                1.0 - total
            } else {
                let bits = u32::from(low[k]) | (packed >> (12 + 2 * k)) & 0x300;
                let w = bits as f32 * (1.0 / 1023.0);
                total += w;
                w
            };
            (reference as usize, weight)
        })
        .collect()
}

impl Glm {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let r = Reader { data, what: "glm" };
        if r.bytes(0, 4)? != IDENT {
            bail!(
                "not a GLM (ident {:?})",
                String::from_utf8_lossy(r.bytes(0, 4)?)
            );
        }
        let version = r.i32(4)?;
        if version != VERSION {
            bail!("GLM version {version}, expected {VERSION}");
        }
        r.bytes(0, HEADER_BYTES)?;
        let name = r.name(8, 64)?;
        let anim_name = r.name(72, 64)?;
        let bone_count = r.count(140, "numBones")?;
        let lod_count = r.count(144, "numLODs")?;
        let ofs_lods = r.count(148, "ofsLODs")?;
        let surface_count = r.count(152, "numSurfaces")?;
        if lod_count < 1 {
            bail!("no levels of detail");
        }
        if surface_count < 1 {
            bail!("no surfaces");
        }

        // Each surface's offset counts from the end of the header
        // (mdxmHierarchyOffsets_t).
        let mut surfaces = Vec::with_capacity(surface_count);
        for s in 0..surface_count {
            let at = HEADER_BYTES + r.count(HEADER_BYTES + s * 4, "surface offset")?;
            r.bytes(at, HIERARCHY_BYTES)?;
            let mut name = r.name(at, 64)?.to_ascii_lowercase();
            if let Some(stem) = name.strip_suffix("_off") {
                name = stem.to_owned();
            }
            let mut shader = r.name(at + 68, 64)?;
            // "[nomaterial]": no shader.
            if shader.starts_with('[') {
                shader.clear();
            }
            let parent = r.i32(at + 136)?;
            let child_count = r.count(at + 140, "numChildren")?;
            let children = (0..child_count)
                .map(|c| r.count(at + 144 + c * 4, "child index"))
                .collect::<Result<Vec<_>, _>>()?;
            surfaces.push(SurfaceInfo {
                name,
                flags: r.u32(at + 64)?,
                shader,
                parent: usize::try_from(parent).ok(),
                children,
            });
        }
        // A tree: each child lists its parent back, so drawing from a root ends.
        for (s, info) in surfaces.iter().enumerate() {
            for &c in &info.children {
                if surfaces.get(c).and_then(|child| child.parent) != Some(s) {
                    bail!("surface {s} ({}): child {c} has another parent", info.name);
                }
            }
        }

        let mut lods = Vec::with_capacity(lod_count);
        let mut at = ofs_lods;
        for l in 0..lod_count {
            let ofs_end = r.count(at, "LOD ofsEnd")?;
            // Each surface's offset counts from the start of the offsets (after
            // mdxmLOD_t), and its surface number must be its place.
            let table = at + 4;
            let mut lod = Vec::with_capacity(surface_count);
            for s in 0..surface_count {
                let surf_at = table + r.count(table + s * 4, "LOD surface offset")?;
                let what = format!("level {l} surface {s}");
                let sr = Reader {
                    data: data.get(surf_at..).unwrap_or(&[]),
                    what: &what,
                };
                lod.push(parse_surface(&sr, s, bone_count)?);
            }
            lods.push(lod);
            if ofs_end == 0 {
                bail!("level {l}: ofsEnd is 0");
            }
            at += ofs_end;
        }
        Ok(Self {
            name,
            anim_name,
            bone_count,
            surfaces,
            lods,
        })
    }

    /// Each surface's own flags, before any override.
    pub fn surface_flags(&self) -> Vec<u32> {
        self.surfaces.iter().map(|s| s.flags).collect()
    }

    /// The first surface named `name` (`G2_IsSurfaceLegal`, ignoring case).
    pub fn find_surface(&self, name: &str) -> Option<usize> {
        self.surfaces
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// `G2_SetSurfaceOnOff`: turns surface `name` off or on in `flags`, which also
    /// clears its hiding of descendants. `false` if no surface has that name.
    pub fn set_surface_off(&self, flags: &mut [u32], name: &str, off: bool) -> bool {
        let Some(s) = self.find_surface(name) else {
            return false;
        };
        flags[s] &= !(SURFACE_OFF | SURFACE_NO_DESCENDANTS);
        if off {
            flags[s] |= SURFACE_OFF;
        }
        true
    }

    /// The flags with `skin` on (`G2API_SetSurfaceOnOffFromSkin`): each surface the
    /// skin gives "*off" is turned off, unless it is off already.
    pub fn skinned_flags(&self, skin: &Skin) -> Vec<u32> {
        let mut flags = self.surface_flags();
        for (name, shader) in &skin.surfaces {
            let on = self
                .find_surface(name)
                .is_some_and(|s| self.surfaces[s].flags & SURFACE_OFF == 0);
            if on && shader == skin::OFF {
                self.set_surface_off(&mut flags, name, true);
            }
        }
        flags
    }

    /// The surfaces drawn with no overrides, in drawing order.
    pub fn drawn_surfaces(&self) -> Vec<usize> {
        self.drawn_with(&self.surface_flags())
    }

    /// The surfaces drawn with `flags` (one per surface), in drawing order
    /// (`RenderSurfaces` from surface 0): a surface with any flag set (a tag, or off)
    /// is not drawn, and one that hides its descendants stops the walk below it.
    pub fn drawn_with(&self, flags: &[u32]) -> Vec<usize> {
        let mut drawn = Vec::new();
        let mut stack = vec![0];
        while let Some(s) = stack.pop() {
            if flags[s] == 0 {
                drawn.push(s);
            }
            if flags[s] & SURFACE_NO_DESCENDANTS == 0 {
                stack.extend(self.surfaces[s].children.iter().rev());
            }
        }
        drawn
    }
}

/// One surface of one level of detail.
fn parse_surface(r: &Reader, index: usize, bone_count: usize) -> Result<Surface, Error> {
    r.bytes(0, SURFACE_BYTES)?;
    let this = r.i32(4)?;
    let vertex_count = r.count(12, "numVerts")?;
    let ofs_verts = r.count(16, "ofsVerts")?;
    let triangle_count = r.count(20, "numTriangles")?;
    let ofs_triangles = r.count(24, "ofsTriangles")?;
    let reference_count = r.count(28, "numBoneReferences")?;
    let ofs_references = r.count(32, "ofsBoneReferences")?;
    if this != index as i32 {
        bail!("{}: numbered {this}", r.what);
    }
    if vertex_count > MAX_VERTEXES {
        bail!(
            "{}: {vertex_count} vertices, the limit is {MAX_VERTEXES}",
            r.what
        );
    }
    if triangle_count * 3 > MAX_INDEXES {
        bail!(
            "{}: {triangle_count} triangles, the limit is {}",
            r.what,
            MAX_INDEXES / 3
        );
    }

    let references = (0..reference_count)
        .map(|i| {
            let bone = r.i32(ofs_references + i * 4)?;
            match usize::try_from(bone) {
                Ok(b) if b < bone_count => Ok(b),
                _ => bail!("{}: bone reference {bone} of {bone_count}", r.what),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    r.bytes(ofs_triangles, triangle_count * 12)?;
    let mut indexes = Vec::with_capacity(triangle_count * 3);
    for i in 0..triangle_count * 3 {
        let v = r.i32(ofs_triangles + i * 4)?;
        match u32::try_from(v) {
            Ok(v) if (v as usize) < vertex_count => indexes.push(v),
            _ => bail!("{}: vertex index {v} of {vertex_count}", r.what),
        }
    }
    r.bytes(ofs_verts, vertex_count * (VERTEX_BYTES + 8))?;
    let mut verts = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let at = ofs_verts + i * VERTEX_BYTES;
        let low = r.bytes(at + 28, 4)?.try_into().unwrap();
        let bones = decode_weights(r.u32(at + 24)?, low)
            .into_iter()
            .map(|(reference, weight)| match references.get(reference) {
                Some(&bone) => Ok((bone, weight)),
                None => bail!(
                    "{}: vertex {i} weights bone reference {reference} of {reference_count}",
                    r.what
                ),
            })
            .collect::<Result<Vec<_>, _>>()?;
        verts.push(Vertex {
            normal: r.vec3(at)?,
            xyz: r.vec3(at + 12)?,
            bones,
        });
    }
    // The texture coordinates follow the vertices (mdxmVertexTexCoord_t).
    let ofs_st = ofs_verts + vertex_count * VERTEX_BYTES;
    let st = (0..vertex_count)
        .map(|i| Ok([r.f32(ofs_st + i * 8)?, r.f32(ofs_st + i * 8 + 4)?]))
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(Surface { indexes, verts, st })
}

/// `RB_SurfaceGhoul`: each vertex's position, the weighted sum of its bones'
/// transforms of it, and its normal, turned by its first bone alone.
pub fn skin(surface: &Surface, pose: &[Mat34]) -> (Vec<Vec3>, Vec<Vec3>) {
    let apply = |m: &Mat34, v: Vec3, w: f32| {
        [0, 1, 2].map(|i| w * (m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] + m[i][3]))
    };
    surface
        .verts
        .iter()
        .map(|v| {
            let mut xyz = [0.0; 3];
            for &(bone, weight) in &v.bones {
                let p = apply(&pose[bone], v.xyz, weight);
                xyz = [0, 1, 2].map(|i| xyz[i] + p[i]);
            }
            let m = &pose[v.bones[0].0];
            let normal = [0, 1, 2]
                .map(|i| m[i][0] * v.normal[0] + m[i][1] * v.normal[1] + m[i][2] * v.normal[2]);
            (xyz, normal)
        })
        .unzip()
}

/// A mesh and its skeleton, as `R_LoadMDXM` registers them.
#[derive(Debug, Clone)]
pub struct Ghoul2Model {
    pub glm: Glm,
    pub gla: Gla,
}

impl Ghoul2Model {
    /// Loads `name` and the skeleton it names. `Err` if either is missing or does not
    /// parse, or the skeleton has another number of bones (the original refuses the
    /// mesh, unless it is an old 72-bone humanoid it converts).
    pub fn load(vfs: &Vfs, name: &str) -> Result<Self, Error> {
        let data = vfs
            .read(name)
            .map_err(|_| Error(format!("{name}: not found")))?;
        let glm = Glm::parse(&data).map_err(|e| Error(format!("{name}: {e}")))?;
        let gla = if glm.anim_name == gla::DEFAULT_NAME {
            Gla::default_skeleton()
        } else {
            let file = format!("{}.gla", glm.anim_name);
            let data = vfs
                .read(&file)
                .map_err(|_| Error(format!("{name}: skeleton {file} not found")))?;
            Gla::parse(&data).map_err(|e| Error(format!("{file}: {e}")))?
        };
        if gla.bones.len() != glm.bone_count {
            bail!(
                "{name}: {} bones, its skeleton {} has {}",
                glm.bone_count,
                glm.anim_name,
                gla.bones.len()
            );
        }
        Ok(Self { glm, gla })
    }
}
