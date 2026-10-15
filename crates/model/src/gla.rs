//! Ghoul2 skeletons and their animation frames, format "2LGA" version 6 (OpenJK
//! `mdx_format.h`), read the way `rd-vanilla/tr_ghoul2.cpp` reads them
//! (`R_LoadMDXA`), and the pose of one frame (`G2_TransformBone`) from the root
//! every model hangs from ([`ROOT`]), with bones turned by angle overrides
//! (`BONE_ANGLES_POSTMULT`) or none.
//!
//! [`Gla::parse`] checks every offset and index in the file, so code that uses a
//! parsed skeleton can index without further checks.

use crate::{Error, Reader, bail};

/// A bone's transform: three rows of rotation and translation (`mdxaBone_t`).
pub type Mat34 = [[f32; 4]; 3];

/// Where a bone is in its animation (`G2_TimingModel`'s current frame, new frame
/// and lerp): frame `frame`, `lerp` of the way to frame `next`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Frame {
    pub frame: usize,
    pub next: usize,
    pub lerp: f32,
}

impl From<usize> for Frame {
    fn from(frame: usize) -> Self {
        Self {
            frame,
            next: frame,
            lerp: 0.0,
        }
    }
}

/// A bone at `frame` blending from `from` (`G2_TransformBone`'s blend): `weight`
/// of the way from `from` to `frame`, 1 once the blend is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blend {
    pub frame: Frame,
    pub from: Frame,
    pub weight: f32,
}

impl Default for Blend {
    fn default() -> Self {
        Frame::default().into()
    }
}

impl From<Frame> for Blend {
    fn from(frame: Frame) -> Self {
        Self {
            frame,
            from: frame,
            weight: 1.0,
        }
    }
}

impl From<usize> for Blend {
    fn from(frame: usize) -> Self {
        Frame::from(frame).into()
    }
}

pub const IDENTITY: Mat34 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// The matrix every pose starts from (`RootMatrix` returns `tr_ghoul2.cpp`'s
/// `identityMatrix`, despite its name a quarter turn about z): a mesh's -y becomes
/// the entity's forward (+x), its +x the entity's left (+y).
pub const ROOT: Mat34 = [
    [0.0, -1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

const IDENT: &[u8; 4] = b"2LGA";
pub const VERSION: i32 = 6;
/// The skeleton a mesh names for one still bone (`sDEFAULT_GLA_NAME`): the original
/// does not read it from disk but serves [`DEFAULT_FILE`].
pub const DEFAULT_NAME: &str = "*default";

const HEADER_BYTES: usize = 100;
const MATRIX_BYTES: usize = 48;
/// A compressed bone: a quaternion and a translation, seven 16-bit fields.
const COMP_BONE_BYTES: usize = 14;

/// `FakeGLAFile` (`tr_model.cpp`): the "*default" skeleton, one bone at rest.
#[rustfmt::skip]
const DEFAULT_FILE: [u8; 294] = [
    0x32, 0x4c, 0x47, 0x41, 0x06, 0x00, 0x00, 0x00, 0x2a, 0x64, 0x65, 0x66, 0x61, 0x75, 0x6c, 0x74,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x01, 0x00, 0x00, 0x00,
    0x14, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x18, 0x01, 0x00, 0x00, 0x68, 0x00, 0x00, 0x00,
    0x26, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x4d, 0x6f, 0x64, 0x56, 0x69, 0x65, 0x77, 0x20,
    0x69, 0x6e, 0x74, 0x65, 0x72, 0x6e, 0x61, 0x6c, 0x20, 0x64, 0x65, 0x66, 0x61, 0x75, 0x6c, 0x74,
    0x00, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd,
    0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd,
    0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0xcd, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfd, 0xbf, 0xfe, 0x7f, 0xfe, 0x7f, 0xfe, 0x7f,
    0x00, 0x80, 0x00, 0x80, 0x00, 0x80,
];

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub flags: u32,
    /// `None` for bone 0, the root; every other bone descends from it.
    pub parent: Option<usize>,
    pub base_pose: Mat34,
    pub base_pose_inv: Mat34,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gla {
    pub name: String,
    /// The scale the skeleton was built with (0 before the field existed).
    pub scale: f32,
    /// At least one.
    pub bones: Vec<Bone>,
    pub frame_count: usize,
    /// Each frame's compressed bone per bone, frame after frame: indexes into `pool`.
    frames: Vec<u32>,
    pool: Vec<[u8; COMP_BONE_BYTES]>,
}

/// `MC_UnCompressQuat`: a rotation from a quaternion (w, x, y, z, each a 16-bit
/// field over 16383, minus 2) and a translation (each a 16-bit field over 64,
/// minus 512).
pub fn uncompress(comp: &[u8; COMP_BONE_BYTES]) -> Mat34 {
    let field = |i: usize| f32::from(u16::from_le_bytes([comp[2 * i], comp[2 * i + 1]]));
    let [w, x, y, z] = [0, 1, 2, 3].map(|i| field(i) / 16383.0 - 2.0);
    let [tx, ty, tz] = [4, 5, 6].map(|i| field(i) / 64.0 - 512.0);
    let (fx, fy, fz) = (2.0 * x, 2.0 * y, 2.0 * z);
    let (wx, wy, wz) = (fx * w, fy * w, fz * w);
    let (xx, xy, xz) = (fx * x, fy * x, fz * x);
    let (yy, yz, zz) = (fy * y, fz * y, fz * z);
    [
        [1.0 - (yy + zz), xy - wz, xz + wy, tx],
        [xy + wz, 1.0 - (xx + zz), yz - wx, ty],
        [xz - wy, yz + wx, 1.0 - (xx + yy), tz],
    ]
}

/// `Multiply_3x4Matrix`: `a` after `b`, as 4x4 matrices with a last row of 0 0 0 1.
pub fn multiply(a: &Mat34, b: &Mat34) -> Mat34 {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            let rot = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
            if c == 3 { rot + a[r][3] } else { rot }
        })
    })
}

fn matrix(r: &Reader, at: usize) -> Result<Mat34, Error> {
    r.bytes(at, MATRIX_BYTES)?;
    let mut m = IDENTITY;
    for (i, v) in m.iter_mut().flatten().enumerate() {
        *v = r.f32(at + i * 4)?;
    }
    Ok(m)
}

impl Gla {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let r = Reader { data, what: "gla" };
        if r.bytes(0, 4)? != IDENT {
            bail!(
                "not a GLA (ident {:?})",
                String::from_utf8_lossy(r.bytes(0, 4)?)
            );
        }
        let version = r.i32(4)?;
        if version != VERSION {
            bail!("GLA version {version}, expected {VERSION}");
        }
        r.bytes(0, HEADER_BYTES)?;
        let name = r.name(8, 64)?;
        let scale = r.f32(72)?;
        let frame_count = r.count(76, "numFrames")?;
        let ofs_frames = r.count(80, "ofsFrames")?;
        let bone_count = r.count(84, "numBones")?;
        let ofs_pool = r.count(88, "ofsCompBonePool")?;
        if frame_count < 1 {
            bail!("no frames");
        }
        if bone_count < 1 {
            bail!("no bones");
        }

        // Each bone's offset counts from the end of the header (mdxaSkelOffsets_t).
        let mut bones = Vec::with_capacity(bone_count);
        for b in 0..bone_count {
            let at = HEADER_BYTES + r.count(HEADER_BYTES + b * 4, "bone offset")?;
            let bone_name = r.name(at, 64)?;
            let parent = r.i32(at + 68)?;
            // G2_TransformBone roots bone 0 and only bone 0.
            let parent = match (b, parent) {
                (0, -1) => None,
                (b, p) if b > 0 && (0..bone_count as i32).contains(&p) => Some(p as usize),
                _ => bail!("bone {b} ({bone_name}): parent {parent}"),
            };
            bones.push(Bone {
                name: bone_name,
                flags: r.u32(at + 64)?,
                parent,
                base_pose: matrix(&r, at + 72)?,
                base_pose_inv: matrix(&r, at + 72 + MATRIX_BYTES)?,
            });
        }

        // Parents may come after their children, but every chain must reach the root.
        for b in 0..bone_count {
            let (mut at, mut steps) = (b, 0);
            while let Some(p) = bones[at].parent {
                at = p;
                steps += 1;
                if steps > bone_count {
                    bail!("bone {b} ({}): its parents loop", bones[b].name);
                }
            }
        }

        // Three bytes per frame and bone (mdxaIndex_t), little-endian.
        let entries = frame_count
            .checked_mul(bone_count)
            .ok_or_else(|| Error("too many frames".into()))?;
        let index_bytes = r.bytes(ofs_frames, entries.saturating_mul(3))?;
        let frames: Vec<u32> = index_bytes
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[a, b, c]| u32::from(a) | u32::from(b) << 8 | u32::from(c) << 16)
            .collect();
        // The pool's size is not stored: it reaches as far as the frames index it.
        let pool_count = frames.iter().max().map_or(0, |&m| m as usize + 1);
        let pool_bytes = r.bytes(ofs_pool, pool_count * COMP_BONE_BYTES)?;
        let pool = pool_bytes.as_chunks::<COMP_BONE_BYTES>().0.to_vec();
        Ok(Self {
            name,
            scale,
            bones,
            frame_count,
            frames,
            pool,
        })
    }

    /// The "*default" skeleton.
    pub fn default_skeleton() -> Self {
        Self::parse(&DEFAULT_FILE).expect("the built-in default skeleton parses")
    }

    /// Bone `bone`'s transform relative to its parent in frame `frame` (below
    /// `frame_count`).
    pub fn local(&self, frame: usize, bone: usize) -> Mat34 {
        uncompress(&self.pool[self.frames[frame * self.bones.len() + bone] as usize])
    }

    /// Every bone's transform in frame `frame`, from the mesh's space to the
    /// entity's: each bone's parent's after its own, the root bone's [`ROOT`]
    /// (`G2_TransformBone`). These skin the mesh's vertices directly.
    pub fn pose(&self, frame: usize) -> Vec<Mat34> {
        self.pose_frames(&ROOT, &vec![frame; self.bones.len()])
    }

    /// `G2_Generate_Matrix` for `BONE_ANGLES_POSTMULT`: the override that turns bone
    /// `bone` by `rotation` (its columns the turned axes, as `Create_Matrix` builds
    /// it) about the bone's base pose: the base pose after the rotation after the
    /// base pose's inverse.
    pub fn angles_override(&self, bone: usize, rotation: &Mat34) -> Mat34 {
        let b = &self.bones[bone];
        multiply(&b.base_pose, &multiply(rotation, &b.base_pose_inv))
    }

    /// The bone named `name`, ignoring case (`G2API_GetBoneIndex`).
    pub fn find_bone(&self, name: &str) -> Option<usize> {
        self.bones
            .iter()
            .position(|b| b.name.eq_ignore_ascii_case(name))
    }

    /// Bone `bone`'s transform relative to its parent at `frame`: its frame's,
    /// lerped component by component toward the next one's (`CBoneCache::EvalLow`).
    pub fn local_at(&self, frame: Frame, bone: usize) -> Mat34 {
        let current = self.local(frame.frame, bone);
        if frame.lerp == 0.0 {
            return current;
        }
        let next = self.local(frame.next, bone);
        let back = 1.0 - frame.lerp;
        std::array::from_fn(|r| {
            std::array::from_fn(|c| frame.lerp * next[r][c] + back * current[r][c])
        })
    }

    /// Bone `bone`'s transform relative to its parent at `blend`: its frame's,
    /// lerped component by component from the frame it blends from by the blend's
    /// weight (`G2_TransformBone`).
    pub fn local_blended(&self, blend: Blend, bone: usize) -> Mat34 {
        let current = self.local_at(blend.frame, bone);
        if blend.weight >= 1.0 {
            return current;
        }
        let from = self.local_at(blend.from, bone);
        let back = 1.0 - blend.weight;
        std::array::from_fn(|r| {
            std::array::from_fn(|c| blend.weight * current[r][c] + back * from[r][c])
        })
    }

    /// Each bone's frame when the bones of `anims` play their frames (bone, frame;
    /// the last of a bone wins) and every other bone plays its parent's
    /// (`CBoneCache::EvalLow`), the root bone frame 0 unless set.
    pub fn frames<T: Copy + Default>(&self, anims: &[(usize, T)]) -> Vec<T> {
        let mut own = vec![None; self.bones.len()];
        for &(bone, frame) in anims {
            own[bone] = Some(frame);
        }
        (0..self.bones.len())
            .map(|mut b| {
                loop {
                    match (own[b], self.bones[b].parent) {
                        (Some(frame), _) => break frame,
                        (None, Some(parent)) => b = parent,
                        (None, None) => break T::default(),
                    }
                }
            })
            .collect()
    }

    /// Every bone's transform with each bone in its own frame of `frames` (one per
    /// bone, each below `frame_count`, a [`Frame`] or a [`Blend`]), the root bone's parent `root`
    /// (`G2_TransformGhoulBones`; an attached model's root is its bolt).
    pub fn pose_frames<F: Copy + Into<Blend>>(&self, root: &Mat34, frames: &[F]) -> Vec<Mat34> {
        self.pose_overridden(root, frames, &[])
    }

    /// [`Gla::pose_frames`] with the bones of `overrides` (bone, override; from
    /// [`Gla::angles_override`]) turned: each such bone's transform after its
    /// override (`BONE_ANGLES_POSTMULT`), which the bones below it inherit.
    pub fn pose_overridden<F: Copy + Into<Blend>>(
        &self,
        root: &Mat34,
        frames: &[F],
        overrides: &[(usize, Mat34)],
    ) -> Vec<Mat34> {
        let mut pose: Vec<Option<Mat34>> = vec![None; self.bones.len()];
        for b in 0..self.bones.len() {
            // Pose the unposed chain above this bone, from the top down.
            let mut chain = vec![b];
            while let Some(p) = self.bones[*chain.last().unwrap()].parent {
                if pose[p].is_some() {
                    break;
                }
                chain.push(p);
            }
            for &c in chain.iter().rev() {
                if pose[c].is_some() {
                    continue;
                }
                let parent = self.bones[c].parent.map_or(*root, |p| pose[p].unwrap());
                let mut m = multiply(&parent, &self.local_blended(frames[c].into(), c));
                if let Some((_, o)) = overrides.iter().find(|(b, _)| *b == c) {
                    m = multiply(&m, o);
                }
                pose[c] = Some(m);
            }
        }
        pose.into_iter().map(Option::unwrap).collect()
    }
}
